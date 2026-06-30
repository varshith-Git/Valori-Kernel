# Valori Performance Benchmark

> dim=128 · release build · Apple Silicon M-series
> Generated: 2026-06-30

## B1 — Insert throughput (single `insert`, dim={DIM})

| Records | Total time   | Throughput   |
| ------- | ------------ | ------------ |
| 100     | 7.09 ms      | 14,103 rec/s |
| 1,000   | 96.898 ms    | 10,320 rec/s |
| 5,000   | 1,147.579 ms | 4,356 rec/s  |

## B2 — Batch insert throughput (`insert_batch`, dim=128)

| Batch size | Total time | Throughput    |
| ---------- | ---------- | ------------- |
| 10         | 4.655 ms   | 2,148 rec/s   |
| 100        | 4.808 ms   | 20,800 rec/s  |
| 1,000      | 10.188 ms  | 98,150 rec/s  |
| 5,000      | 29.864 ms  | 167,427 rec/s |
| 10,000     | 56.273 ms  | 177,705 rec/s |

## B3 — Search latency vs dataset size (bruteforce, dim=128, k=10)

| Records   | p50        | p95        | p99        | QPS       |
| --------- | ---------- | ---------- | ---------- | --------- |
| 1,000     | 0.129 ms   | 0.131 ms   | 0.135 ms   | 7,820 q/s |
| 10,000    | 1.224 ms   | 1.285 ms   | 1.354 ms   | 810 q/s   |
| 50,000    | 10.129 ms  | 10.735 ms  | 11.336 ms  | 98 q/s    |
| 1,000,000 | 247.815 ms | 288.795 ms | 308.291 ms | 3 q/s     |

## B4 — Index type comparison (dim=128, k=10)

| Index      | Records   | Build time     | p50        | p99        | QPS       |
| ---------- | --------- | -------------- | ---------- | ---------- | --------- |
| bruteforce | 1,000,000 | 26,636.37 ms   | 247.407 ms | 297.008 ms | 4 q/s     |
| hnsw       | 1,000,000 | 263,779.926 ms | 0.107 ms   | 0.138 ms   | 9,199 q/s |
| ivf        | 1,000,000 | 27,913.735 ms  | 58.35 ms   | 66.048 ms  | 16 q/s    |

## B5 — Dimension impact (10K records, bruteforce, k=10)

| Dim | Bytes/record | p50      | p99      | QPS       |
| --- | ------------ | -------- | -------- | --------- |
| 32  | 128 B/rec    | 0.258 ms | 0.315 ms | 3,802 q/s |
| 128 | 512 B/rec    | 0.804 ms | 0.867 ms | 1,233 q/s |
| 384 | 1536 B/rec   | 2.834 ms | 3.451 ms | 345 q/s   |

## B6 — Snapshot timing

| Records | Size       | snapshot() | restore() | save_snapshot() |
| ------- | ---------- | ---------- | --------- | --------------- |
| 10,000  | 5281.5 KB  | 2.247 ms   | 4.272 ms  | 4.704 ms        |
| 50,000  | 26375.3 KB | 10.08 ms   | 21.569 ms | 26.705 ms       |

## B7 — Batch size impact on throughput (dim=128, bruteforce)

| Batch size | # calls | Total time   | Throughput    |
| ---------- | ------- | ------------ | ------------- |
| 1          | 10000   | 3,979.774 ms | 2,512 rec/s   |
| 10         | 1000    | 5,162.906 ms | 1,936 rec/s   |
| 100        | 100     | 686.764 ms   | 14,561 rec/s  |
| 500        | 20      | 164.458 ms   | 60,805 rec/s  |
| 1,000      | 10      | 105.1 ms     | 95,147 rec/s  |
| 10,000     | 1       | 57.155 ms    | 174,963 rec/s |

## Summary

| Metric | Value |
|---|---|
| Best insert throughput | `insert_batch(10000)` → **177,705 rec/s** |
| Brute search @10K | **1.224 ms** p50 · 810 q/s |
| Brute search @50K | **10.129 ms** p50 · 98 q/s |
| Brute search @1M  | **247.815 ms** p50 · 3 q/s |
| HNSW search @1,000,000 | **0.107 ms** p50 · 9,199 q/s |
| IVF search @1,000,000  | **58.35 ms** p50 · 16 q/s |

> Benchmark environment: Apple Silicon M-series · macOS · release build
> Run: `python3 benchmarks/local_perf.py --million`
