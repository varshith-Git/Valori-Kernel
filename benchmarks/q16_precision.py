"""
Q16.16 Precision Analysis — Recall vs Float32 Baseline
=======================================================

Measures whether Valori's Q16.16 fixed-point arithmetic costs any retrieval
quality compared to a standard float32 nearest-neighbour search (numpy).

What it measures
----------------
For each (embedding model, dimension) combination:

  - Float32 ground truth: exact L2 nearest neighbours computed in numpy.
  - Q16.16 via Valori:   search results returned by a running Valori node.
  - Recall@k:            |Q16.16 top-k ∩ float32 top-k| / k

A recall of 1.0 means Q16.16 and float32 return identical result sets.
Any degradation is purely from fixed-point quantisation, not approximation
(Valori's BruteForce index is exact within Q16.16 precision).

Embedding models tested
-----------------------
  384-dim  : sentence-transformers/all-MiniLM-L6-v2
  768-dim  : sentence-transformers/all-mpnet-base-v2
  1536-dim : OpenAI text-embedding-ada-002  (requires OPENAI_API_KEY)
  3072-dim : OpenAI text-embedding-3-large  (requires OPENAI_API_KEY)

Requirements
------------
    pip install valoricore requests numpy sentence-transformers

A Valori node must be running on localhost:3000 with matching DIM.
Start one per dimension:
    docker run --rm -p 3000:3000 -e VALORI_DIM=384 valori-node:latest

Usage
-----
    python benchmarks/q16_precision.py --dim 384
    python benchmarks/q16_precision.py --dim 768
    python benchmarks/q16_precision.py --dim 1536 --openai   # requires API key
    python benchmarks/q16_precision.py --dim 3072 --openai
    python benchmarks/q16_precision.py --all                 # runs all (skips OpenAI if no key)
"""

import argparse
import os
import sys
import time
from typing import Optional

try:
    import numpy as np
    import requests
except ImportError:
    sys.exit("pip install numpy requests")

K = 10           # Recall@10
N_CORPUS = 500   # number of corpus vectors
N_QUERIES = 50   # number of query vectors


# ── float32 ground truth ──────────────────────────────────────────────────────

def exact_l2_topk(corpus: np.ndarray, query: np.ndarray, k: int) -> list[int]:
    """Exact L2 nearest neighbours in float32 (ground truth)."""
    diffs = corpus - query
    dists = (diffs * diffs).sum(axis=1)
    return list(np.argsort(dists)[:k])


# ── Valori helpers ────────────────────────────────────────────────────────────

def valori_insert(base: str, vectors: np.ndarray) -> list[int]:
    ids = []
    for vec in vectors:
        resp = requests.post(
            f"{base}/records", json={"values": vec.tolist()}, timeout=15
        )
        resp.raise_for_status()
        ids.append(resp.json()["id"])
    return ids


def valori_search(base: str, query: np.ndarray, k: int) -> list[int]:
    resp = requests.post(
        f"{base}/search", json={"query": query.tolist(), "k": k}, timeout=15
    )
    resp.raise_for_status()
    return [h["id"] for h in resp.json().get("results", [])]


# ── embedding generators ──────────────────────────────────────────────────────

def gen_random(dim: int, n: int, seed: int = 42) -> np.ndarray:
    """Synthetic normalized vectors — fast, no model required."""
    rng = np.random.default_rng(seed)
    vecs = rng.standard_normal((n, dim)).astype(np.float32)
    norms = np.linalg.norm(vecs, axis=1, keepdims=True)
    return vecs / np.maximum(norms, 1e-9)


def gen_sentence_transformers(dim: int, n: int) -> np.ndarray:
    model_name = {
        384: "all-MiniLM-L6-v2",
        768: "all-mpnet-base-v2",
    }.get(dim)
    if model_name is None:
        raise ValueError(f"No sentence-transformers model mapped for dim={dim}")
    try:
        from sentence_transformers import SentenceTransformer
    except ImportError:
        sys.exit("pip install sentence-transformers")

    model = SentenceTransformer(model_name)
    # Generate diverse synthetic sentences for the corpus
    sentences = [f"Document {i}: the quick brown fox jumps over {i} lazy dogs." for i in range(n)]
    vecs = model.encode(sentences, convert_to_numpy=True, normalize_embeddings=True)
    return vecs.astype(np.float32)


def gen_openai(dim: int, n: int) -> np.ndarray:
    api_key = os.environ.get("OPENAI_API_KEY")
    if not api_key:
        raise EnvironmentError("OPENAI_API_KEY not set")
    model = {
        1536: "text-embedding-ada-002",
        3072: "text-embedding-3-large",
    }.get(dim)
    if model is None:
        raise ValueError(f"No OpenAI model mapped for dim={dim}")
    try:
        from openai import OpenAI
    except ImportError:
        sys.exit("pip install openai")

    client = OpenAI(api_key=api_key)
    texts = [f"Document {i}: financial record entry number {i}." for i in range(n)]
    vecs = []
    for i in range(0, n, 100):
        batch = texts[i : i + 100]
        resp = client.embeddings.create(model=model, input=batch)
        vecs.extend([d.embedding for d in resp.data])
    return np.array(vecs, dtype=np.float32)


# ── main benchmark ────────────────────────────────────────────────────────────

def run_benchmark(dim: int, base_url: str, use_openai: bool = False, use_st: bool = False):
    print(f"\n{'─'*60}")
    print(f"Dimension: {dim}")

    # Generate corpus and queries
    if use_openai and dim in (1536, 3072):
        print(f"  Embedding source : OpenAI (dim={dim})")
        try:
            all_vecs = gen_openai(dim, N_CORPUS + N_QUERIES)
        except EnvironmentError as e:
            print(f"  SKIP: {e}")
            return None
    elif use_st and dim in (384, 768):
        print(f"  Embedding source : sentence-transformers (dim={dim})")
        all_vecs = gen_sentence_transformers(dim, N_CORPUS + N_QUERIES)
    else:
        print(f"  Embedding source : synthetic normalized (seed=42)")
        all_vecs = gen_random(dim, N_CORPUS + N_QUERIES)

    corpus  = all_vecs[:N_CORPUS]
    queries = all_vecs[N_CORPUS:]

    # Insert corpus into Valori
    print(f"  Inserting {N_CORPUS} vectors into Valori node at {base_url}...")
    t0 = time.perf_counter()
    ids = valori_insert(base_url, corpus)
    insert_ms = (time.perf_counter() - t0) * 1000
    print(f"  Insert: {insert_ms:.0f} ms ({insert_ms/N_CORPUS:.2f} ms/vec)")

    # Compute recall@k for each query
    recalls = []
    latencies = []
    for query in queries:
        gt = set(exact_l2_topk(corpus, query, K))

        t0 = time.perf_counter()
        valori_ids = set(valori_search(base_url, query, K))
        latencies.append((time.perf_counter() - t0) * 1000)

        intersection = gt & valori_ids
        recalls.append(len(intersection) / K)

    recall_mean  = float(np.mean(recalls))
    recall_min   = float(np.min(recalls))
    p50_ms       = float(np.percentile(latencies, 50))
    p99_ms       = float(np.percentile(latencies, 99))

    print(f"  Recall@{K} (mean): {recall_mean:.4f}  min={recall_min:.4f}")
    print(f"  Search latency:   p50={p50_ms:.1f} ms  p99={p99_ms:.1f} ms")

    return {
        "dim": dim,
        "n_corpus": N_CORPUS,
        "n_queries": N_QUERIES,
        "k": K,
        "recall_mean": recall_mean,
        "recall_min": recall_min,
        "p50_ms": p50_ms,
        "p99_ms": p99_ms,
    }


def print_summary(results: list[dict]):
    print(f"\n{'='*60}")
    print("SUMMARY — Q16.16 Recall@10 vs float32 ground truth")
    print(f"{'='*60}")
    print(f"{'Dim':>6}  {'Recall@10 (mean)':>17}  {'Recall@10 (min)':>16}  {'p50 ms':>7}  {'p99 ms':>7}")
    print(f"{'─'*6}  {'─'*17}  {'─'*16}  {'─'*7}  {'─'*7}")
    for r in results:
        print(
            f"{r['dim']:>6}  {r['recall_mean']:>17.4f}  {r['recall_min']:>16.4f}"
            f"  {r['p50_ms']:>7.1f}  {r['p99_ms']:>7.1f}"
        )
    print()
    perfect = all(r["recall_mean"] == 1.0 for r in results)
    if perfect:
        print("✓  Recall@10 = 1.0000 across all dimensions.")
        print("   Q16.16 fixed-point introduces zero retrieval degradation")
        print("   on normalised embeddings within the representable range.")
    else:
        degraded = [r for r in results if r["recall_mean"] < 1.0]
        for r in degraded:
            loss = 1.0 - r["recall_mean"]
            print(f"  dim={r['dim']}: recall loss = {loss:.4f} ({loss*100:.2f}%)")
        print()
        print("Non-zero recall loss indicates near-ties whose relative ordering")
        print("flips between float32 and Q16.16. Check embedding value range —")
        print("values outside [-32767, 32767] saturate in Q16.16.")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://localhost:3000")
    parser.add_argument("--dim", type=int, choices=[384, 768, 1536, 3072])
    parser.add_argument("--all", action="store_true", dest="run_all")
    parser.add_argument("--openai", action="store_true", help="Use OpenAI embeddings for 1536/3072")
    parser.add_argument("--st", action="store_true", help="Use sentence-transformers for 384/768")
    args = parser.parse_args()

    dims = [384, 768, 1536, 3072] if args.run_all else [args.dim or 384]
    results = []
    for dim in dims:
        r = run_benchmark(
            dim,
            base_url=args.url,
            use_openai=args.openai,
            use_st=args.st,
        )
        if r:
            results.append(r)

    if results:
        print_summary(results)


if __name__ == "__main__":
    main()
