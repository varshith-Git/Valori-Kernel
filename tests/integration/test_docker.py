#!/usr/bin/env python3
"""
Docker integration tests for Valori — BQ index, Auto-tier, and Standalone sharding.

Prerequisites:
    docker build -t valori-node:test .
    docker compose -f docker-compose.test.yml up -d
    python3 tests/integration/test_docker.py

Exit code 0 = all tests passed.
"""

import sys
import time
import random
import math
import traceback
from typing import Dict, List, Optional
import requests

# ── Config ────────────────────────────────────────────────────────────────────

BQ_URL     = "http://localhost:3100"
AUTO_URL   = "http://localhost:3101"
SHARD_URL  = "http://localhost:3102"
DIM        = 128
TIMEOUT    = 60  # seconds to wait for each node to become healthy

PASS = "\033[32mPASS\033[0m"
FAIL = "\033[31mFAIL\033[0m"

results = []

# ── Helpers ───────────────────────────────────────────────────────────────────

def rand_vec(dim: int = DIM) -> List[float]:
    v = [random.uniform(-1.0, 1.0) for _ in range(dim)]
    mag = math.sqrt(sum(x*x for x in v)) or 1.0
    return [x / mag for x in v]

def wait_healthy(url: str, timeout: int = TIMEOUT) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            r = requests.get(f"{url}/health", timeout=3)
            if r.status_code == 200:
                return True
        except Exception:
            pass
        time.sleep(1)
    return False

def check(name: str, condition: bool, detail: str = ""):
    status = PASS if condition else FAIL
    msg = f"  [{status}] {name}"
    if not condition and detail:
        msg += f"\n         {detail}"
    print(msg)
    results.append((name, condition))
    return condition

def insert(url: str, vec: List[float], collection: str = "default", text: str = None) -> dict:
    payload: dict = {"values": vec}
    if collection != "default":
        payload["collection"] = collection
    if text:
        payload["text"] = text
    r = requests.post(f"{url}/records", json=payload, timeout=10)
    r.raise_for_status()
    return r.json()

def search(url: str, vec: List[float], k: int = 5, collection: str = "default") -> List[dict]:
    payload: dict = {"query": vec, "k": k}
    if collection != "default":
        payload["collection"] = collection
    r = requests.post(f"{url}/search", json=payload, timeout=10)
    r.raise_for_status()
    body = r.json()
    # API returns either a flat list or {"results": [...]}
    if isinstance(body, list):
        return body
    return body.get("results", [])

def create_collection(url: str, name: str) -> None:
    r = requests.post(f"{url}/v1/namespaces", json={"name": name}, timeout=10)
    r.raise_for_status()

# ── Section 1: BQ index ───────────────────────────────────────────────────────

def test_bq(url: str) -> None:
    print("\n=== BQ Index Tests ===")

    h = requests.get(f"{url}/health", timeout=5).json()
    # Health returns Debug format e.g. "Bq" — compare case-insensitively
    check("health.index == 'bq'", h.get("index", "").lower() == "bq",
          f"got index={h.get('index')!r}")

    # Insert a batch of vectors and search — BQ uses Hamming coarse + L2 rescore
    vecs = [rand_vec() for _ in range(50)]
    for v in vecs:
        insert(url, v)

    h2 = requests.get(f"{url}/health", timeout=5).json()
    check("records inserted", h2["records"]["live"] == 50,
          f"got {h2['records']['live']}")

    query = vecs[0]
    hits = search(url, query, k=5)
    check("search returns 5 hits", len(hits) == 5,
          f"got {len(hits)}")
    # First hit should be the query vector itself (record 0 with near-zero distance)
    check("nearest hit is exact match", hits[0]["id"] == 0 and hits[0]["score"] < 0.01,
          f"got id={hits[0]['id']}, score={hits[0]['score']}")
    check("BQ scores are non-negative", all(h["score"] >= 0 for h in hits))

    # BQ proof — audit chain must exist
    r = requests.get(f"{url}/v1/proof/state", timeout=5)
    check("proof/state responds", r.status_code == 200)
    proof = r.json()
    check("proof has state_hash", bool(proof.get("final_state_hash")))


# ── Section 2: Auto-tier index ────────────────────────────────────────────────

def test_auto(url: str) -> None:
    print("\n=== Auto-tier Index Tests ===")

    h = requests.get(f"{url}/health", timeout=5).json()
    # auto(bruteforce) or auto(brute_force) — just check prefix
    check("initial auto tier reported",
          h.get("index", "").lower().startswith("auto("),
          f"got index={h.get('index')!r}")
    check("initial tier is brute-force",
          "brute" in h.get("index", "").lower(),
          f"got index={h.get('index')!r}")

    # Insert < 10k records (stay in brute-force tier)
    for _ in range(100):
        insert(url, rand_vec())

    h2 = requests.get(f"{url}/health", timeout=5).json()
    check("after 100 inserts still brute-force tier",
          "brute" in h2.get("index", "").lower(),
          f"got index={h2.get('index')!r}")
    check("auto prefix present", h2.get("index", "").lower().startswith("auto("),
          f"got index={h2.get('index')!r}")

    query = rand_vec()
    hits = search(url, query, k=5)
    check("auto-tier search returns results", len(hits) > 0)

    # Verify index config endpoint (returns index_type / hnsw keys)
    r = requests.get(f"{url}/v1/index/config", timeout=5)
    check("index/config responds", r.status_code == 200)
    cfg = r.json()
    check("index/config has index_type key",
          "index_type" in cfg,
          f"keys: {list(cfg.keys())}")
    check("index/config index_type is auto",
          cfg.get("index_type", "").lower() == "auto",
          f"got {cfg.get('index_type')!r}")

    # Switch to BQ via rebuild endpoint
    r2 = requests.post(f"{url}/v1/index/rebuild", json={"index": "bq"}, timeout=30)
    check("rebuild to bq succeeds", r2.status_code == 200)
    rebuilt = r2.json()
    check("rebuild response has ok=true", rebuilt.get("ok") is True)
    check("rebuild response shows bq", rebuilt.get("index") == "bq",
          f"got {rebuilt.get('index')!r}")

    h3 = requests.get(f"{url}/health", timeout=5).json()
    check("after rebuild health shows bq", h3.get("index", "").lower() == "bq",
          f"got {h3.get('index')!r}")

    # Switch back to auto
    r3 = requests.post(f"{url}/v1/index/rebuild", json={"index": "auto"}, timeout=30)
    check("rebuild back to auto succeeds", r3.status_code == 200)


# ── Section 3: Standalone sharding ───────────────────────────────────────────

def test_sharding(url: str) -> None:
    print("\n=== Standalone Sharding Tests ===")

    h = requests.get(f"{url}/health", timeout=5).json()
    check("health.shard_count == 4", h.get("shard_count") == 4,
          f"got shard_count={h.get('shard_count')!r}")

    # Create collections in different namespaces
    collections = ["alpha", "beta", "gamma", "delta", "epsilon"]
    for col in collections:
        create_collection(url, col)

    # Shard routing endpoint
    r = requests.get(f"{url}/v1/shard/routing", timeout=5)
    check("shard/routing responds 200", r.status_code == 200)
    routing = r.json()
    check("routing.mode == standalone", routing.get("mode") == "standalone")
    check("routing.shard_count == 4", routing.get("shard_count") == 4,
          f"got {routing.get('shard_count')!r}")
    check("routing has 4 shards", len(routing.get("shards", [])) == 4,
          f"got {len(routing.get('shards', []))}")

    # All collections should appear across shards
    all_cols = set()
    for shard in routing.get("shards", []):
        all_cols.update(shard.get("collections", []))
    for col in collections:
        check(f"collection '{col}' routed to a shard", col in all_cols)

    # Namespace routing formula: ns_id % shard_count
    # "default" is ns_id=0 → shard 0
    shard0_cols = routing["shards"][0].get("collections", [])
    check("default collection is on shard 0", "default" in shard0_cols,
          f"shard 0 collections: {shard0_cols}")

    # Insert records into multiple collections and verify search works per-collection
    vecs_by_col: Dict[str, list] = {}
    for col in ["default"] + collections[:3]:
        vecs = [rand_vec() for _ in range(10)]
        vecs_by_col[col] = vecs
        col_arg = col if col != "default" else "default"
        for v in vecs:
            insert(url, v, collection=col_arg if col != "default" else "default")

    for col in ["default"] + collections[:3]:
        col_arg = col if col != "default" else "default"
        query = vecs_by_col[col][0]
        hits = search(url, query, k=5, collection=col_arg)
        check(f"search in collection '{col}' returns results", len(hits) > 0,
              f"got {len(hits)} hits")

    # Health after inserts
    h2 = requests.get(f"{url}/health", timeout=5).json()
    total = 10 * (1 + 3)  # default + 3 collections × 10 records
    check("total record count correct", h2["records"]["live"] == total,
          f"expected {total}, got {h2['records']['live']}")


# ── Main ──────────────────────────────────────────────────────────────────────

def main() -> None:
    random.seed(42)

    nodes = [
        ("BQ node",           BQ_URL),
        ("Auto-tier node",    AUTO_URL),
        ("Sharded node",      SHARD_URL),
    ]

    print("Waiting for nodes to become healthy...")
    for name, url in nodes:
        ok = wait_healthy(url)
        check(f"{name} healthy", ok, f"timed out after {TIMEOUT}s at {url}")

    try:
        test_bq(BQ_URL)
    except Exception as e:
        print(f"  [{FAIL}] BQ test suite crashed: {e}")
        traceback.print_exc()

    try:
        test_auto(AUTO_URL)
    except Exception as e:
        print(f"  [{FAIL}] Auto-tier test suite crashed: {e}")
        traceback.print_exc()

    try:
        test_sharding(SHARD_URL)
    except Exception as e:
        print(f"  [{FAIL}] Sharding test suite crashed: {e}")
        traceback.print_exc()

    # Summary
    total   = len(results)
    passed  = sum(1 for _, ok in results if ok)
    failed  = total - passed
    print(f"\n{'='*50}")
    print(f"Results: {passed}/{total} passed", end="")
    if failed:
        print(f", {failed} FAILED")
        for name, ok in results:
            if not ok:
                print(f"  FAILED: {name}")
        sys.exit(1)
    else:
        print(" ✓")
        sys.exit(0)


if __name__ == "__main__":
    main()
