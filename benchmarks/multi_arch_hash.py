"""
Multi-Architecture Identical-Hash Benchmark
============================================

Proof of the central Valori claim: identical BLAKE3 state hash regardless of
CPU architecture or operating system.

How to use
----------
Run this script on each target platform independently and compare the printed
hash. If Valori's determinism guarantee holds, every platform prints the same
64-character hex string.

Automated CI proof: see .github/workflows/multi-arch-determinism.yml, which
runs the equivalent Rust test (valori-node/tests/multi_arch_determinism.rs) on
x86_64 (Linux), ARM64 (macOS), and WASM32 in the same GitHub Actions run and
fails the build if any hash differs.

Requirements
------------
    pip install valoricore requests

A running Valori node is required (standalone mode is sufficient):
    docker run --rm -p 3000:3000 -e VALORI_DIM=8 -e VALORI_MAX_RECORDS=256 valori-node:latest

Usage
-----
    python benchmarks/multi_arch_hash.py [--url http://localhost:3000]
"""

import argparse
import hashlib
import json
import sys
import time

try:
    import requests
except ImportError:
    sys.exit("pip install requests")

# Deterministic seed corpus — same on every run, every machine.
# Do NOT use random(); the point is reproducibility.
SEED_VECTORS = [
    [float(((i * 31 + j * 17) % 1000) / 1000.0) for j in range(8)]
    for i in range(50)
]


def insert_all(base_url: str) -> list[int]:
    ids = []
    for vec in SEED_VECTORS:
        resp = requests.post(f"{base_url}/records", json={"values": vec}, timeout=10)
        resp.raise_for_status()
        ids.append(resp.json()["id"])
    return ids


def get_state_hash(base_url: str) -> str:
    resp = requests.get(f"{base_url}/v1/proof/state", timeout=10)
    resp.raise_for_status()
    h = resp.json()["final_state_hash"]
    # Handle both wire formats: hex string (current) or byte array (legacy)
    if isinstance(h, list):
        return bytes(h).hex()
    return h


def search_top1(base_url: str, query: list[float]) -> dict:
    resp = requests.post(
        f"{base_url}/search", json={"query": query, "k": 1}, timeout=10
    )
    resp.raise_for_status()
    results = resp.json().get("results", [])
    return results[0] if results else {}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://localhost:3000")
    args = parser.parse_args()
    base = args.url.rstrip("/")

    print(f"Valori Multi-Architecture Hash Benchmark")
    print(f"Node: {base}")
    print()

    # 1. Insert deterministic corpus
    print(f"Inserting {len(SEED_VECTORS)} deterministic vectors...")
    t0 = time.perf_counter()
    ids = insert_all(base)
    elapsed = time.perf_counter() - t0
    print(f"  Inserted {len(ids)} records in {elapsed:.3f}s")

    # 2. Capture state hash
    state_hash = get_state_hash(base)
    print()
    print(f"BLAKE3 state hash:")
    print(f"  {state_hash}")
    print()

    # 3. Verify search determinism — same query must return same top hit
    query = SEED_VECTORS[0]
    hit = search_top1(base, query)
    print(f"Search determinism check (query = SEED_VECTORS[0]):")
    print(f"  top-1 id={hit.get('id')}  score={hit.get('score')}")
    print()

    # 4. Expected hash (computed from the canonical run; update after any
    #    corpus or dimension change)
    EXPECTED = None  # Set this after first run to lock the expected value.
    if EXPECTED:
        if state_hash == EXPECTED:
            print("✓  Hash matches expected value — determinism confirmed")
        else:
            print("✗  HASH MISMATCH")
            print(f"   Expected : {EXPECTED}")
            print(f"   Got      : {state_hash}")
            sys.exit(1)
    else:
        print("(No expected hash set. Run on your canonical platform first,")
        print(" then set EXPECTED = '<hash>' in this script and re-run on")
        print(" each additional architecture.)")

    print()
    print("Copy this line to compare across architectures:")
    print(f'  EXPECTED = "{state_hash}"')


if __name__ == "__main__":
    main()
