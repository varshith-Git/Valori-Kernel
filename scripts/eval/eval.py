#!/usr/bin/env python3
"""
Valori C0 Eval Harness

Measures recall@k, citation existence, and provenance integrity against a live
Valori node. Numbers from the bootstrap corpus are labeled [bootstrap] and must
not be presented as product truth on a real target corpus.

Subcommands
-----------
probe       Health + citation sanity check. No embedding needed.
seed-eval   Seeds test data, embeds, searches, measures recall@k.
            CI gate: fails if recall@1 < --recall-threshold (default 0.8).
verify      Given saved receipt JSON files, verifies content_sha256 values
            match the text fetched from the live node.

Examples
--------
# Quick sanity check:
python scripts/eval/eval.py probe --url http://localhost:3000

# Full recall eval with ollama:
python scripts/eval/eval.py seed-eval \\
    --url http://localhost:3000 \\
    --embed-provider ollama --embed-model nomic-embed-text

# Custom QA file (entries must have "question" field; "gold_record_ids" optional):
python scripts/eval/eval.py seed-eval \\
    --qa-file scripts/eval/qa_sets/bootstrap.jsonl \\
    --embed-provider openai --embed-model text-embedding-3-small \\
    --embed-api-key $OPENAI_API_KEY

# Verify saved receipts against live node:
python scripts/eval/eval.py verify --url http://localhost:3000 receipts/*.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
from pathlib import Path
from typing import Optional

try:
    import httpx
except ImportError:
    sys.exit("Missing dependency: pip install httpx")

# ── Embedding ──────────────────────────────────────────────────────────────────

def embed(text: str, provider: str, model: str, api_key: str, endpoint: str) -> list[float]:
    """Embed a text string using the configured provider."""
    if provider == "ollama":
        url = (endpoint or "http://localhost:11434").rstrip("/")
        r = httpx.post(f"{url}/api/embeddings",
                       json={"model": model, "prompt": text}, timeout=60)
        r.raise_for_status()
        return r.json()["embedding"]

    elif provider in ("openai", "custom"):
        url = (endpoint or "https://api.openai.com/v1").rstrip("/")
        headers = {
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        }
        r = httpx.post(f"{url}/embeddings",
                       json={"input": text, "model": model},
                       headers=headers, timeout=60)
        r.raise_for_status()
        return r.json()["data"][0]["embedding"]

    else:
        raise ValueError(f"Unknown embed provider: {provider!r}. Use ollama, openai, or custom.")


# ── Pure metric functions ──────────────────────────────────────────────────────

def recall_at_k(gold_ids: list[int], retrieved_ids: list[int], k: int) -> float:
    """1.0 if any gold_id is in the top-k results, else 0.0."""
    top_k = set(retrieved_ids[:k])
    return 1.0 if any(g in top_k for g in gold_ids) else 0.0


def sha256hex(text: str) -> str:
    """SHA-256 of UTF-8 text, prefixed for self-describing hashes."""
    return "sha256:" + hashlib.sha256(text.encode()).hexdigest()


def citation_existence(result_ids: list[int], known_ids: set[int]) -> float:
    """Fraction of result IDs that exist in the known set."""
    if not result_ids:
        return 1.0
    return sum(1 for i in result_ids if i in known_ids) / len(result_ids)


def provenance_integrity(
    result_ids: list[int],
    base_url: str,
    auth_headers: dict,
) -> Optional[float]:
    """
    For each result ID, fetch text metadata from the node and verify that:
    1. The text is fetchable.
    2. Its SHA-256 is stable (two fetches, same hash).

    Returns fraction of records with stable metadata, or None if no records
    had metadata to check.
    """
    stable, total = 0, 0
    for rid in result_ids:
        r1 = httpx.get(
            f"{base_url}/v1/memory/meta/get",
            params={"target_id": f"record:{rid}"},
            headers=auth_headers, timeout=10,
        )
        if not r1.is_success:
            continue
        meta1 = r1.json().get("metadata") or {}
        text1 = meta1.get("text") or meta1.get("value")
        if not text1:
            continue
        h1 = sha256hex(str(text1))
        total += 1

        r2 = httpx.get(
            f"{base_url}/v1/memory/meta/get",
            params={"target_id": f"record:{rid}"},
            headers=auth_headers, timeout=10,
        )
        if r2.is_success:
            meta2 = r2.json().get("metadata") or {}
            text2 = meta2.get("text") or meta2.get("value")
            if text2 and sha256hex(str(text2)) == h1:
                stable += 1

    return stable / total if total > 0 else None


# ── Valori client ──────────────────────────────────────────────────────────────

class ValoriClient:
    def __init__(self, url: str, auth_token: Optional[str] = None):
        self.base = url.rstrip("/")
        self.headers: dict[str, str] = {}
        if auth_token:
            self.headers["Authorization"] = f"Bearer {auth_token}"

    def health(self) -> dict:
        r = httpx.get(f"{self.base}/health", headers=self.headers, timeout=10)
        r.raise_for_status()
        return r.json()

    def search(self, vector: list[float], k: int, collection: str) -> list[dict]:
        r = httpx.post(
            f"{self.base}/search",
            json={"query": vector, "k": k, "collection": collection},
            headers=self.headers, timeout=30,
        )
        r.raise_for_status()
        return r.json().get("results", [])

    def insert(self, vector: list[float], collection: str) -> int:
        r = httpx.post(
            f"{self.base}/records",
            json={"values": vector, "collection": collection},
            headers=self.headers, timeout=30,
        )
        r.raise_for_status()
        return r.json()["id"]

    def set_meta(self, target_id: str, metadata: dict, collection: str) -> None:
        r = httpx.post(
            f"{self.base}/v1/memory/meta/set",
            json={"target_id": target_id, "metadata": metadata, "collection": collection},
            headers=self.headers, timeout=10,
        )
        r.raise_for_status()

    def get_meta(self, target_id: str) -> Optional[dict]:
        r = httpx.get(
            f"{self.base}/v1/memory/meta/get",
            params={"target_id": target_id},
            headers=self.headers, timeout=10,
        )
        return r.json().get("metadata") if r.is_success else None

    def create_collection(self, name: str) -> None:
        r = httpx.post(
            f"{self.base}/v1/namespaces",
            json={"name": name},
            headers=self.headers, timeout=10,
        )
        if not r.is_success and r.status_code != 409:
            r.raise_for_status()

    def drop_collection(self, name: str) -> None:
        r = httpx.delete(
            f"{self.base}/v1/namespaces/{name}",
            headers=self.headers, timeout=10,
        )
        if not r.is_success and r.status_code != 404:
            r.raise_for_status()

    def proof(self) -> dict:
        r = httpx.get(f"{self.base}/v1/proof/state", headers=self.headers, timeout=10)
        r.raise_for_status()
        return r.json()


# ── Subcommand: probe ──────────────────────────────────────────────────────────

def cmd_probe(args: argparse.Namespace) -> None:
    """Quick health + reachability check. Requires no embedding."""
    client = ValoriClient(args.url, args.auth_token)

    print(f"[probe] → {args.url}")
    try:
        h = client.health()
    except Exception as e:
        print(f"[probe] FAIL: cannot reach node — {e}")
        sys.exit(1)

    print(f"  version       : {h.get('version', '?')}")
    print(f"  dim           : {h.get('dim', '?')}")
    print(f"  record_count  : {h.get('record_count', '?')}")
    print(f"  status        : {h.get('status', '?')}")

    try:
        proof = client.proof()
        print(f"  state_hash    : {str(proof.get('final_state_hash','?'))[:24]}…")
    except Exception:
        print("  state_hash    : (proof endpoint unavailable)")

    # Spot-check metadata fetch on record 1 if it exists
    if (h.get("record_count") or 0) > 0:
        meta = client.get_meta("record:1")
        if meta is not None:
            print("  meta:record:1 : reachable ✓")
        else:
            print("  meta:record:1 : no metadata (may be cluster mode or no text ingested)")

    print("[probe] PASS")


# ── Subcommand: seed-eval ──────────────────────────────────────────────────────

# Built-in seed texts used when no --qa-file is provided.
SEED_TEXTS = [
    "The BLAKE3 hash function provides cryptographic integrity for the Valori audit chain.",
    "Vector embeddings are stored as Q16.16 fixed-point scalars for deterministic arithmetic.",
    "Raft consensus ensures all cluster nodes apply committed events in the same order.",
    "Namespaces provide multi-tenant isolation within a single Valori node instance.",
    "The write-ahead log enables crash recovery without data loss on node restart.",
    "Snapshots capture the full kernel state including the graph topology and vector index.",
    "GDPR right-to-erasure is implemented via the ShredKey committed event in the audit chain.",
    "The audit log is append-only and each entry is BLAKE3-chained to the previous entry.",
    "Q16.16 fixed-point arithmetic ensures bit-identical results across CPU architectures.",
    "openraft 0.9 backs Valori cluster consensus with persistent log storage via redb.",
]


def cmd_seed_eval(args: argparse.Namespace) -> None:
    """Seed test data, embed, search, measure recall@k and provenance integrity."""
    client = ValoriClient(args.url, args.auth_token)

    # Verify node is up
    try:
        h = client.health()
    except Exception as e:
        print(f"[seed-eval] FAIL: node unreachable — {e}")
        sys.exit(1)

    server_dim = h.get("dim")

    # Load QA entries or use built-in seed texts
    if args.qa_file:
        raw = [json.loads(l) for l in Path(args.qa_file).read_text().splitlines() if l.strip()]
        texts = [e["question"] for e in raw]
        gold_ids_per_entry: list[Optional[list[int]]] = [e.get("gold_record_ids") for e in raw]
        print(f"[seed-eval] loaded {len(texts)} entries from {args.qa_file}")
    else:
        texts = SEED_TEXTS
        gold_ids_per_entry = [None] * len(texts)
        print(f"[seed-eval] using {len(texts)} built-in seed texts (bootstrap corpus)")

    # Create an isolated test namespace
    ns = args.namespace or f"eval-{int(time.time())}"
    print(f"[seed-eval] namespace={ns!r}  k={args.k}  provider={args.embed_provider}/{args.embed_model}")
    client.create_collection(ns)

    # Embed + insert all seed texts, record the assigned IDs
    print(f"[seed-eval] embedding and inserting {len(texts)} records…")
    seeded_ids: list[int] = []

    for i, text in enumerate(texts):
        try:
            vec = embed(text, args.embed_provider, args.embed_model,
                        args.embed_api_key, args.embed_endpoint)
        except Exception as e:
            print(f"  [{i+1}/{len(texts)}] FAIL embed: {e}")
            if not args.keep_namespace:
                client.drop_collection(ns)
            sys.exit(1)

        if server_dim and len(vec) != server_dim:
            print(f"  [{i+1}] FAIL: dimension mismatch — model={len(vec)}  server={server_dim}")
            if not args.keep_namespace:
                client.drop_collection(ns)
            sys.exit(1)

        rid = client.insert(vec, ns)
        client.set_meta(f"record:{rid}", {"text": text, "source": f"seed:{i}", "index": i}, ns)
        seeded_ids.append(rid)

        if i == 0 or (i + 1) % 5 == 0 or (i + 1) == len(texts):
            print(f"  [{i+1}/{len(texts)}] record_id={rid}")

    print(f"[seed-eval] inserted {len(seeded_ids)} records → IDs {seeded_ids[0]}…{seeded_ids[-1]}")
    known_ids = set(seeded_ids)

    # Re-embed each text as the query and search
    print(f"\n[seed-eval] running eval…")
    r1_scores: list[float] = []
    r5_scores: list[float] = []
    ce_scores: list[float] = []
    pi_scores: list[float] = []
    auth_headers = {k: v for k, v in client.headers.items()}

    for i, (text, seeded_id) in enumerate(zip(texts, seeded_ids)):
        gold = gold_ids_per_entry[i] if gold_ids_per_entry[i] else [seeded_id]

        try:
            qvec = embed(text, args.embed_provider, args.embed_model,
                         args.embed_api_key, args.embed_endpoint)
        except Exception as e:
            print(f"  [{i+1}] SKIP embed failed: {e}")
            continue

        hits = client.search(qvec, args.k, ns)
        top_ids = [h["id"] for h in hits]

        r1 = recall_at_k(gold, top_ids, 1)
        r5 = recall_at_k(gold, top_ids, min(5, args.k))
        ce = citation_existence(top_ids, known_ids)
        pi = provenance_integrity(top_ids[:3], args.url, auth_headers)

        r1_scores.append(r1)
        r5_scores.append(r5)
        ce_scores.append(ce)
        if pi is not None:
            pi_scores.append(pi)

        r1_sym = "✓" if r1 == 1.0 else "✗"
        pi_str = f"{pi:.2f}" if pi is not None else "—"
        print(f"  [{i+1:>2}] {r1_sym} r@1={r1:.1f}  r@5={r5:.1f}  cite={ce:.2f}  prov={pi_str}")

    # Aggregate
    mean_r1 = sum(r1_scores) / len(r1_scores) if r1_scores else 0.0
    mean_r5 = sum(r5_scores) / len(r5_scores) if r5_scores else 0.0
    mean_ce = sum(ce_scores) / len(ce_scores) if ce_scores else 1.0
    mean_pi = sum(pi_scores) / len(pi_scores) if pi_scores else None
    n = len(r1_scores)

    print("\n── Metrics ───────────────────────────────────────────────────────────")
    print(f"  recall@1          : {mean_r1:.3f}  "
          f"({int(round(sum(r1_scores)))}/{n} exact top-1 hits)")
    print(f"  recall@5          : {mean_r5:.3f}  "
          f"({int(round(sum(r5_scores)))}/{n} in top-{min(5,args.k)})")
    print(f"  citation_existence: {mean_ce:.3f}  "
          f"(fraction of result IDs that are real)")
    if mean_pi is not None:
        print(f"  provenance_integ  : {mean_pi:.3f}  "
              f"(metadata SHA-256 stability across two fetches)")
    print(f"  corpus            : [bootstrap] — not for external claims")
    print()

    # Save report
    report = {
        "type": "ValoriEvalReport",
        "schema_version": "1",
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "url": args.url,
        "namespace": ns,
        "k": args.k,
        "n": n,
        "embed_provider": args.embed_provider,
        "embed_model": args.embed_model,
        "corpus": "[bootstrap]",
        "metrics": {
            "recall_at_1": mean_r1,
            "recall_at_5": mean_r5,
            "citation_existence": mean_ce,
            "provenance_integrity": mean_pi,
        },
        "seeded_record_ids": seeded_ids,
        "note": "Bootstrap corpus — labeled [bootstrap]. Not for external claims.",
    }
    out = Path(args.report) if args.report else Path("eval_report.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2))
    print(f"[seed-eval] report → {out}")

    # Cleanup
    if not args.keep_namespace:
        client.drop_collection(ns)
        print(f"[seed-eval] dropped namespace {ns!r}")

    # CI gate
    threshold = args.recall_threshold
    if mean_r1 < threshold:
        print(f"\n[seed-eval] FAIL: recall@1 {mean_r1:.3f} < threshold {threshold:.3f}")
        sys.exit(1)
    if mean_ce < 1.0:
        print(f"\n[seed-eval] FAIL: citation_existence {mean_ce:.3f} < 1.0 — "
              f"some result IDs do not exist in the namespace")
        sys.exit(1)

    print(f"[seed-eval] PASS")


# ── Subcommand: verify ─────────────────────────────────────────────────────────

def cmd_verify(args: argparse.Namespace) -> None:
    """Verify provenance integrity of saved receipt JSON files against a live node."""
    client = ValoriClient(args.url, args.auth_token)
    auth_headers = dict(client.headers)

    files = [Path(f) for f in args.receipts]
    if not files:
        print("[verify] no receipt files provided")
        sys.exit(1)

    print(f"[verify] {len(files)} receipt(s) → {args.url}")

    total_pass, total_fail, total_skip = 0, 0, 0

    for path in files:
        try:
            receipt = json.loads(path.read_text())
        except Exception as e:
            print(f"  SKIP {path.name}: cannot parse — {e}")
            total_skip += 1
            continue

        chunks = receipt.get("chunks", [])
        version = receipt.get("version", "?")
        file_pass, file_fail, file_skip = 0, 0, 0

        for chunk in chunks:
            rid = chunk.get("record_id")
            stored_hash = chunk.get("content_sha256")
            if not rid or not stored_hash:
                file_skip += 1
                continue

            meta = client.get_meta(f"record:{rid}")
            if not meta:
                file_skip += 1
                continue

            text = meta.get("text") or meta.get("value")
            if not text:
                file_skip += 1
                continue

            computed = sha256hex(str(text))
            if computed == stored_hash:
                file_pass += 1
            else:
                file_fail += 1
                print(f"  MISMATCH record:{rid}")
                print(f"    stored  : {stored_hash[:32]}…")
                print(f"    computed: {computed[:32]}…")

        status = "PASS" if file_fail == 0 else "FAIL"
        print(f"  {status} {path.name} (v{version}): "
              f"{file_pass} verified, {file_fail} mismatch, {file_skip} skip")

        if file_fail == 0:
            total_pass += 1
        else:
            total_fail += 1
        total_skip += file_skip

    print(f"\n[verify] {total_pass} PASS  {total_fail} FAIL  ({total_skip} chunks skipped)")
    if total_fail:
        sys.exit(1)
    print("[verify] PASS")


# ── CLI ────────────────────────────────────────────────────────────────────────

def main() -> None:
    p = argparse.ArgumentParser(
        description="Valori C0 Eval Harness",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    p.add_argument("--url", default="http://localhost:3000",
                   help="Valori node base URL (default: http://localhost:3000)")
    p.add_argument("--auth-token", default=None, dest="auth_token",
                   help="Bearer token (VALORI_AUTH_TOKEN)")

    sub = p.add_subparsers(dest="cmd", required=True)

    # ── probe ──────────────────────────────────────────────────────────────────
    pb = sub.add_parser("probe", help="Health + reachability check")
    pb.add_argument("--namespace", default="default")

    # ── seed-eval ──────────────────────────────────────────────────────────────
    se = sub.add_parser("seed-eval", help="Seed test data and measure recall@k")
    se.add_argument("--namespace", default=None,
                    help="Namespace name (default: auto-generated eval-<timestamp>)")
    se.add_argument("--qa-file", default=None, dest="qa_file",
                    help="JSONL file with QA entries (optional; uses built-in seed texts if omitted)")
    se.add_argument("--embed-provider", default="ollama", dest="embed_provider",
                    choices=["ollama", "openai", "custom"],
                    help="Embedding provider (default: ollama)")
    se.add_argument("--embed-model", default="nomic-embed-text", dest="embed_model",
                    help="Embedding model name (default: nomic-embed-text)")
    se.add_argument("--embed-api-key", default="", dest="embed_api_key",
                    help="API key (required for openai/custom)")
    se.add_argument("--embed-endpoint", default="", dest="embed_endpoint",
                    help="Custom endpoint URL (overrides provider default)")
    se.add_argument("--k", type=int, default=5,
                    help="Top-k for search (default: 5)")
    se.add_argument("--recall-threshold", type=float, default=0.8, dest="recall_threshold",
                    help="CI gate: exit 1 if recall@1 < this (default: 0.8)")
    se.add_argument("--report", default=None,
                    help="Output report path (default: eval_report.json)")
    se.add_argument("--keep-namespace", action="store_true", dest="keep_namespace",
                    help="Do not drop the test namespace after eval (useful for debugging)")

    # ── verify ─────────────────────────────────────────────────────────────────
    vr = sub.add_parser("verify", help="Verify provenance integrity of saved receipt files")
    vr.add_argument("receipts", nargs="+", help="Receipt JSON file paths")

    args = p.parse_args()

    dispatch = {"probe": cmd_probe, "seed-eval": cmd_seed_eval, "verify": cmd_verify}
    dispatch[args.cmd](args)


if __name__ == "__main__":
    main()
