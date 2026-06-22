#!/usr/bin/env python3
"""
Phase 3.8 — Write-throughput regression gate.

Inserts 10 000 vectors into a running Valori node and reports:
  - p50 / p99 single-insert latency (ms)
  - batch throughput (records/sec)
  - comparison against a stored baseline (if present)

Exit codes:
  0 — passed (or no baseline to compare against)
  1 — regression detected (p99 > baseline*1.15 or throughput < baseline*0.90)

Usage:
  # Measure and print (no baseline comparison):
  python3 benchmarks/write_regression.py

  # Measure and compare against saved baseline:
  python3 benchmarks/write_regression.py --compare

  # Save current measurements as the new baseline (CI: make benchmark-baseline):
  python3 benchmarks/write_regression.py --save-baseline

Environment:
  VALORI_URL   — node URL (default http://localhost:3000)
  VALORI_DIM   — vector dimension (default 128)
  BENCH_N      — number of records to insert (default 10000)
  BENCH_BATCH  — records per batch (default 100)
"""

import os
import sys
import json
import math
import time
import random
import argparse
import statistics
import urllib.request
import urllib.error

VALORI_URL = os.environ.get("VALORI_URL", "http://localhost:3000")
DIM        = int(os.environ.get("VALORI_DIM", "128"))
N          = int(os.environ.get("BENCH_N", "10000"))
BATCH_SIZE = int(os.environ.get("BENCH_BATCH", "100"))
BASELINE_PATH = os.path.join(os.path.dirname(__file__), "baseline", "write_regression_baseline.json")

P99_REGRESSION_THRESHOLD    = 1.15   # p99 may not grow more than 15%
THROUGHPUT_REGRESSION_THRESHOLD = 0.90  # throughput may not drop more than 10%


def _post(path: str, payload: dict) -> dict:
    body = json.dumps(payload).encode()
    req = urllib.request.Request(
        f"{VALORI_URL}{path}",
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read())


def _check_health():
    try:
        with urllib.request.urlopen(f"{VALORI_URL}/health", timeout=5) as resp:
            data = json.loads(resp.read())
            if not data.get("status", "").lower().startswith("ok"):
                print(f"[warn] Unexpected health status: {data}")
    except Exception as e:
        print(f"[error] Cannot reach node at {VALORI_URL}: {e}")
        sys.exit(1)


def rand_vec():
    return [round(random.uniform(-1.0, 1.0), 4) for _ in range(DIM)]


def measure_single_inserts(n: int = 200) -> list:
    """Measure p50/p99 by timing individual single inserts."""
    latencies = []
    for _ in range(n):
        vec = rand_vec()
        t0 = time.perf_counter()
        _post("/records", {"values": vec})
        latencies.append((time.perf_counter() - t0) * 1000)
    return latencies


def measure_batch_throughput(total: int, batch_size: int) -> float:
    """Insert `total` records in batches of `batch_size`, return records/sec."""
    batches = math.ceil(total / batch_size)
    t0 = time.perf_counter()
    for _ in range(batches):
        batch = [rand_vec() for _ in range(batch_size)]
        _post("/v1/vectors/batch_insert", {"batch": batch})
    elapsed = time.perf_counter() - t0
    return total / elapsed


def run() -> dict:
    _check_health()
    print(f"Valori write regression — {N} records, dim={DIM}, batch={BATCH_SIZE}")
    print(f"Target: {VALORI_URL}\n")

    print("Phase 1: single-insert latency (200 inserts) ...")
    lats = measure_single_inserts(200)
    lats_sorted = sorted(lats)
    p50 = statistics.median(lats_sorted)
    p99 = lats_sorted[int(len(lats_sorted) * 0.99)]
    print(f"  p50 = {p50:.2f} ms   p99 = {p99:.2f} ms")

    print(f"\nPhase 2: batch throughput ({N} records, batch={BATCH_SIZE}) ...")
    throughput = measure_batch_throughput(N, BATCH_SIZE)
    print(f"  throughput = {throughput:.0f} records/sec")

    return {"p50_ms": round(p50, 3), "p99_ms": round(p99, 3), "throughput_rps": round(throughput, 1)}


def compare_with_baseline(result: dict) -> bool:
    """Return True if result passes regression check vs saved baseline."""
    try:
        with open(BASELINE_PATH) as f:
            baseline = json.load(f)
    except FileNotFoundError:
        print(f"\n[warn] No baseline at {BASELINE_PATH} — skipping comparison.")
        return True

    passed = True
    print("\n── Regression check ──────────────────────────────────────────")
    for metric, label, threshold, direction in [
        ("p99_ms",         "p99 latency",  P99_REGRESSION_THRESHOLD,        "higher"),
        ("throughput_rps", "throughput",   THROUGHPUT_REGRESSION_THRESHOLD, "lower"),
    ]:
        base_val = baseline.get(metric)
        curr_val = result.get(metric)
        if base_val is None:
            print(f"  {label}: baseline missing, skipping")
            continue

        if direction == "higher":
            limit = base_val * threshold
            ok = curr_val <= limit
            symbol = "✓" if ok else "✗"
            print(f"  {symbol} {label}: {curr_val:.2f} ms  (baseline {base_val:.2f} ms, limit {limit:.2f} ms)")
        else:
            limit = base_val * threshold
            ok = curr_val >= limit
            symbol = "✓" if ok else "✗"
            print(f"  {symbol} {label}: {curr_val:.0f} rps  (baseline {base_val:.0f} rps, limit {limit:.0f} rps)")

        if not ok:
            passed = False

    print()
    return passed


def save_baseline(result: dict):
    os.makedirs(os.path.dirname(BASELINE_PATH), exist_ok=True)
    with open(BASELINE_PATH, "w") as f:
        json.dump(result, f, indent=2)
    print(f"\nBaseline saved to {BASELINE_PATH}")


def main():
    parser = argparse.ArgumentParser(description="Valori write regression benchmark")
    parser.add_argument("--compare", action="store_true", help="Compare against saved baseline and exit 1 on regression")
    parser.add_argument("--save-baseline", action="store_true", help="Save current measurements as the new baseline")
    args = parser.parse_args()

    result = run()
    print(f"\nResult: {json.dumps(result, indent=2)}")

    if args.save_baseline:
        save_baseline(result)

    if args.compare:
        passed = compare_with_baseline(result)
        sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
