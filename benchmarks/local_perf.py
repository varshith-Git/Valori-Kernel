#!/usr/bin/env python3
"""
Valori local-client performance benchmark
==========================================
Tests insert throughput, search latency, index comparison, and snapshot
timing using the embedded LocalClient (no server required).

Usage
-----
    # Standard run (up to 50K records):
    python3 benchmarks/local_perf.py

    # Quick run (skip 50K / 1M tests):
    python3 benchmarks/local_perf.py --quick

    # Full 1 million record run (takes ~5–15 min depending on index):
    python3 benchmarks/local_perf.py --million

    # Save results to a file:
    python3 benchmarks/local_perf.py --million --out benchmarks/RESULTS_1M.md

Requirements
------------
    pip install -e python/       # install the valoricore SDK from source
    # OR install from PyPI:
    pip install valoricore

    The release wheel must be installed for meaningful numbers:
        cd crates/valori-ffi
        maturin build --release
        pip install ../../target/wheels/valoricore_ffi-*.whl --force-reinstall
"""

import argparse
import math
import os
import statistics
import sys
import tempfile
import time
from typing import List

try:
    from valoricore import LocalClient
except ImportError:
    print("ERROR: valoricore not installed. Run: pip install -e python/")
    sys.exit(1)

# ── CLI ───────────────────────────────────────────────────────────────────────

parser = argparse.ArgumentParser(description="Valori local performance benchmark")
parser.add_argument("--quick",   action="store_true", help="Skip 50K and 1M tests")
parser.add_argument("--million", action="store_true", help="Include 1M record tests (slow)")
parser.add_argument("--dim",     type=int, default=128, help="Vector dimension (default 128)")
parser.add_argument("--out",     type=str, default="", help="Write markdown results to this file")
args = parser.parse_args()

DIM   = args.dim
QUICK = args.quick
MILLION = args.million

output_lines: List[str] = []

# ── helpers ───────────────────────────────────────────────────────────────────

def vec(seed: int, dim: int = DIM) -> List[float]:
    return [math.sin(seed * 1.7 + i * 0.9) for i in range(dim)]

def fresh(index: str = "bruteforce", max_records: int = 0) -> LocalClient:
    if max_records == 0:
        max_records = 1_100_000 if MILLION else 60_000
    return LocalClient(
        path=tempfile.mkdtemp(prefix="val_bench_"),
        dim=DIM,
        index_kind=index,
        max_records=max_records,
    )

def percentile(data: List[float], p: float) -> float:
    data = sorted(data)
    k = (len(data) - 1) * p / 100
    f, c = int(k), math.ceil(k)
    if f == c:
        return data[int(k)]
    return data[f] * (c - k) + data[c] * (k - f)

def ms(seconds: float) -> float:
    return round(seconds * 1000, 3)

def emit(line: str = "") -> None:
    print(line)
    output_lines.append(line)

def section(title: str) -> None:
    emit()
    emit(f"## {title}")
    emit()

def table(headers: List[str], rows: List[List[str]]) -> None:
    widths = [
        max(len(str(h)), max((len(str(r[i])) for r in rows), default=0))
        for i, h in enumerate(headers)
    ]
    sep   = "| " + " | ".join("-" * w for w in widths) + " |"
    hrow  = "| " + " | ".join(str(h).ljust(w) for h, w in zip(headers, widths)) + " |"
    emit(hrow)
    emit(sep)
    for row in rows:
        emit("| " + " | ".join(str(c).ljust(w) for c, w in zip(row, widths)) + " |")

# ── header ────────────────────────────────────────────────────────────────────

emit(f"# Valori Performance Benchmark")
emit(f"")
emit(f"> dim={DIM} · release build · Apple Silicon M-series")
emit(f"> Generated: {time.strftime('%Y-%m-%d')}")

# ── B1: Insert throughput ────────────────────────────────────────────────────

section(f"B1 — Insert throughput (single `insert`, dim={DIM})")

scales = [100, 1_000, 5_000]
rows_b1 = []
for n in scales:
    c = fresh()
    vectors = [vec(i) for i in range(n)]
    t0 = time.perf_counter()
    for v in vectors:
        c.insert(v)
    el = time.perf_counter() - t0
    rows_b1.append([f"{n:,}", f"{ms(el):,} ms", f"{int(n/el):,} rec/s"])

table(["Records", "Total time", "Throughput"], rows_b1)

# ── B2: Batch insert throughput ───────────────────────────────────────────────

section(f"B2 — Batch insert throughput (`insert_batch`, dim={DIM})")

batch_sizes = [10, 100, 1_000, 5_000, 10_000]
rows_b2 = []
for bs in batch_sizes:
    c = fresh()
    vectors = [vec(i) for i in range(bs)]
    t0 = time.perf_counter()
    c.insert_batch(vectors)
    el = time.perf_counter() - t0
    rows_b2.append([f"{bs:,}", f"{ms(el):,} ms", f"{int(bs/el):,} rec/s"])

table(["Batch size", "Total time", "Throughput"], rows_b2)

# ── B3: Search latency at scale (bruteforce) ──────────────────────────────────

section(f"B3 — Search latency vs dataset size (bruteforce, dim={DIM}, k=10)")

scales_b3 = [1_000, 10_000]
if not QUICK:
    scales_b3.append(50_000)
if MILLION:
    scales_b3.append(1_000_000)

QUERY_COUNT = 200
rows_b3 = []
for n in scales_b3:
    print(f"  B3: loading {n:,} records...", end=" ", flush=True)
    c = fresh("bruteforce", max_records=n + 1000)
    chunk = 10_000
    for start in range(0, n, chunk):
        size = min(chunk, n - start)
        c.insert_batch([vec(start + j) for j in range(size)])

    q = min(QUERY_COUNT, 50) if n >= 500_000 else QUERY_COUNT
    lats = []
    for qi in range(q):
        t0 = time.perf_counter()
        c.search(vec(9999 + qi), k=10)
        lats.append(time.perf_counter() - t0)

    p50 = round(percentile(lats, 50)  * 1000, 3)
    p95 = round(percentile(lats, 95)  * 1000, 3)
    p99 = round(percentile(lats, 99)  * 1000, 3)
    qps = int(1 / statistics.mean(lats))
    rows_b3.append([f"{n:,}", f"{p50} ms", f"{p95} ms", f"{p99} ms", f"{qps:,} q/s"])
    print(f"p50={p50}ms QPS={qps:,}")

table(["Records", "p50", "p95", "p99", "QPS"], rows_b3)

# ── B4: Index type comparison ─────────────────────────────────────────────────

section(f"B4 — Index type comparison (dim={DIM}, k=10)")

n_b4 = 1_000_000 if MILLION else 10_000
rows_b4 = []
indexes = ["bruteforce", "hnsw", "ivf"]

for idx in indexes:
    print(f"  B4: {idx} @ {n_b4:,} records...", end=" ", flush=True)
    c = fresh(idx, max_records=n_b4 + 1000)
    chunk = 10_000
    t_ins = time.perf_counter()
    for start in range(0, n_b4, chunk):
        size = min(chunk, n_b4 - start)
        c.insert_batch([vec(start + j) for j in range(size)])
    ins_ms = ms(time.perf_counter() - t_ins)

    q = 50 if n_b4 >= 500_000 and idx == "bruteforce" else 500
    lats = []
    for qi in range(q):
        t0 = time.perf_counter()
        c.search(vec(9999 + qi), k=10)
        lats.append(time.perf_counter() - t0)

    p50 = round(percentile(lats, 50) * 1000, 3)
    p99 = round(percentile(lats, 99) * 1000, 3)
    qps = int(1 / statistics.mean(lats))
    rows_b4.append([idx, f"{n_b4:,}", f"{ins_ms:,} ms", f"{p50} ms", f"{p99} ms", f"{qps:,} q/s"])
    print(f"insert={ins_ms}ms p50={p50}ms QPS={qps:,}")

table(["Index", "Records", "Build time", "p50", "p99", "QPS"], rows_b4)

# ── B5: Dimension impact ──────────────────────────────────────────────────────

section("B5 — Dimension impact (10K records, bruteforce, k=10)")

# Dims match real embedding models:
#   128  — baseline / small custom models
#   384  — nomic-embed-text, all-MiniLM-L6-v2
#   768  — BGE-base, E5-base, bert-base
#   1536 — OpenAI text-embedding-3-small / ada-002
rows_b5 = []
for dim, label in [(128, "baseline"), (384, "nomic/MiniLM"), (768, "BGE/E5/bert-base"), (1536, "OpenAI ada-002")]:
    c = LocalClient(
        path=tempfile.mkdtemp(prefix="val_bench_"),
        dim=dim, index_kind="bruteforce", max_records=11_000,
    )
    vd = [math.sin(i * 0.9) for i in range(dim)]
    c.insert_batch([vd] * 10_000)

    lats = []
    for qi in range(QUERY_COUNT):
        t0 = time.perf_counter()
        c.search(vd, k=10)
        lats.append(time.perf_counter() - t0)

    p50 = round(percentile(lats, 50) * 1000, 3)
    p99 = round(percentile(lats, 99) * 1000, 3)
    qps = int(1 / statistics.mean(lats))
    rows_b5.append([f"{dim}", label, f"{dim*4} B/rec", f"{p50} ms", f"{p99} ms", f"{qps:,} q/s"])

table(["Dim", "Model", "Bytes/record", "p50", "p99", "QPS"], rows_b5)

# ── B6: Snapshot timing ───────────────────────────────────────────────────────

section("B6 — Snapshot timing")

snap_scales = [10_000]
if not QUICK:
    snap_scales.append(50_000)

rows_b6 = []
for n in snap_scales:
    c = fresh("bruteforce", max_records=n + 1000)
    c.insert_batch([vec(i) for i in range(n)])

    t0 = time.perf_counter(); snap_bytes = c.snapshot(); snap_ms  = ms(time.perf_counter()-t0)
    t0 = time.perf_counter(); c.restore(snap_bytes);     rest_ms  = ms(time.perf_counter()-t0)
    t0 = time.perf_counter(); snap_path  = c.save_snapshot(); save_ms = ms(time.perf_counter()-t0)
    snap_kb = round(os.path.getsize(snap_path) / 1024, 1)

    rows_b6.append([f"{n:,}", f"{snap_kb} KB", f"{snap_ms} ms", f"{rest_ms} ms", f"{save_ms} ms"])

table(["Records", "Size", "snapshot()", "restore()", "save_snapshot()"], rows_b6)

# ── B7: Batch size sweet spot ─────────────────────────────────────────────────

section(f"B7 — Batch size impact on throughput (dim={DIM}, bruteforce)")

total = 10_000
rows_b7 = []
for bs in [1, 10, 100, 500, 1_000, 10_000]:
    c = fresh()
    all_vecs = [vec(i) for i in range(total)]
    t0 = time.perf_counter()
    if bs == 1:
        for v in all_vecs:
            c.insert(v)
    else:
        for b in range(total // bs):
            c.insert_batch(all_vecs[b*bs:(b+1)*bs])
    el = time.perf_counter() - t0
    rows_b7.append([f"{bs:,}", f"{total//bs}", f"{ms(el):,} ms", f"{int(total/el):,} rec/s"])

table(["Batch size", "# calls", "Total time", "Throughput"], rows_b7)

# ── Summary ───────────────────────────────────────────────────────────────────

section("Summary")

emit("| Metric | Value |")
emit("|---|---|")
emit(f"| Best insert throughput | `insert_batch(10000)` → **{rows_b2[-1][2]}** |")
emit(f"| Brute search @10K | **{rows_b3[1][1]}** p50 · {rows_b3[1][4]} |")
if not QUICK and len(rows_b3) >= 3:
    emit(f"| Brute search @50K | **{rows_b3[2][1]}** p50 · {rows_b3[2][4]} |")
if MILLION and len(rows_b3) >= 4:
    emit(f"| Brute search @1M  | **{rows_b3[3][1]}** p50 · {rows_b3[3][4]} |")

hnsw_row = next((r for r in rows_b4 if r[0] == "hnsw"), None)
if hnsw_row:
    emit(f"| HNSW search @{n_b4:,} | **{hnsw_row[3]}** p50 · {hnsw_row[5]} |")

ivf_row = next((r for r in rows_b4 if r[0] == "ivf"), None)
if ivf_row:
    emit(f"| IVF search @{n_b4:,}  | **{ivf_row[3]}** p50 · {ivf_row[5]} |")

emit()
emit("> Benchmark environment: Apple Silicon M-series · macOS · release build")
emit("> Run: `python3 benchmarks/local_perf.py --million`")

# ── write output file ─────────────────────────────────────────────────────────

if args.out:
    with open(args.out, "w") as f:
        f.write("\n".join(output_lines) + "\n")
    print(f"\nResults written to: {args.out}")
