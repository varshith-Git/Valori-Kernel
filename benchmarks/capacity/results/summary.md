# S9 Capacity Benchmark — Raw Results Summary

Machine-readable results: [`results.json`](results.json) / [`results.csv`](results.csv).
Full per-scenario files also kept individually (see filenames below).

## Stage A — RAM boundary, 384D, BruteForce (`stage_a_*.json`)

| RAM | vectors | peak RSS | %used | insert vec/s | search p50 | restart hash |
|---|---|---|---|---|---|---|
| 512MB | 10K | 86 MB | 17% | 200.3 | 116 ms | ✅ match |
| 512MB | 50K | 349 MB | 68% | 245.8 | 671 ms | ✅ match |
| 512MB | 100K | 512 MB | **100%** | 120.0 | 1308 ms | ✅ match |
| 1024MB | 100K | 751 MB | 73% | 136.2 | 1307 ms | ✅ match |

**Reading this**: doubling RAM (512MB→1GB) barely moved search latency (1308ms→1307ms) at
100K vectors — confirms latency here is CPU-bound (0.5 vCPU, BruteForce O(N) scan), not
memory-bound. Memory-wise, 512MB is at a genuinely dangerous 100% utilization at 100K;
1GB has real headroom (73%) but the search latency at that scale is not acceptable for
an interactive product regardless of memory headroom.

## Index-type comparison, 384D, 10K vectors, 1GB/0.5CPU (`index_comparison.json`, `hnsw_recovery_retest.json`)

| index | insert vec/s | peak RSS | search p50 | recovery time | hash match |
|---|---|---|---|---|---|
| BruteForce | 1224.8 | 85.1 MB | 118 ms | 1.2 s | ✅ |
| HNSW | 51.2 | 87.4 MB | 116 ms | **188 s** | ✅ (retest w/ longer timeout) |
| IVF | 1188.8 | 85.2 MB | 122 ms | 5.2 s | ✅ |
| BQ | 1136.4 | 86.4 MB | 114 ms | 1.1 s | ✅ |

**Reading this**: at this small scale, all four index types give near-identical search
latency (index selection doesn't matter yet at 10K), but HNSW's insert throughput is
~24x slower and its recovery time is 35-160x slower than the other three. HNSW is real
and correct (hash integrity holds), but operationally expensive — a restart on an
HNSW-indexed collection means minutes of downtime even at modest scale. **Not tested
at larger scale in this phase** (time budget) — the gap only gets worse as vector count
grows, since index build cost scales with N.

## Dimension comparison, 20K vectors, BruteForce, 1GB/0.5CPU (`dimension_comparison*.json`)

| dim | peak RSS | bytes/vector (actual) | bytes/vector (raw Q16.16) | overhead | disk | insert vec/s | search p50 | hash match |
|---|---|---|---|---|---|---|---|---|
| 384 | 155 MB | 8,147 | 1,536 | 5.3x | 31 MB | 658.7 | 277 ms | ✅ |
| 768 | 277 MB | 14,497 | 3,072 | 4.7x | 60 MB | 162.3 | 510 ms | ✅ |
| 1024 | 363 MB | 19,026 | 4,096 | 4.6x | 80 MB | 91.2 | 693 ms | ✅ |
| 1536 | 534 MB | 27,997 | 6,144 | 4.6x | 119 MB | 41.1 | 1022 ms | ✅ |

**Linear fit** (measured, R² high across these 4 points):
`actual_bytes_per_vector(dim) ≈ 17.23 × dim + 1531`

This is a real, measured relationship — NOT `dim × sizeof(float)`, which would
undercount actual memory by ~4.6-5.3x. The overhead includes the record pool's
slot/metadata overhead, namespace linked-list pointers, and Q16.16 fixed-point
representation (4 bytes/scalar, same as f32, so that alone isn't the overhead source —
the pool/slab structure and per-record bookkeeping is).

## Multi-project contention (`multiproject.json`)

| concurrent projects | per-project insert vec/s | cross-project token check |
|---|---|---|
| 1 | 1889.5 | n/a |
| 2 | ~1286 (avg, -32%) | 401 (correctly rejected) both directions |
| 4 | ~828 (avg, -56%) | 401 (correctly rejected) in all 4 |

**Reading this**: real CPU contention between sibling project-containers on one host —
throughput degrades roughly evenly across all N concurrent projects (no evidence one
project starves another; degradation looked fair, not skewed). Cross-project
`worker_auth_token` isolation held under concurrent load in every case tested — no
leakage, no bypass.

## NOT MEASURED this phase (explicit, not silently skipped)

- Vector counts 250K/500K/750K/1M — each 100K-scale cell already took 700-830s single
  cell; the full higher range was not run for real. See `capacity_model.md` for the
  EXTRAPOLATED (not measured) estimate using the linear memory model above, clearly
  labeled as such.
- IVF/BQ at scale beyond 10K — only tested at the small feasibility-check scale.
  Given BruteForce's search latency is already the practical bottleneck at 100K+,
  determining whether IVF/BQ meaningfully improve on that is the highest-value S10
  follow-up.
- Collections scaling (1/5/10/25/50 collections) — script written
  (`scripts/bench_collections.py`), not run for real this session (time budget).
- Concurrency sweep (1/10/25 clients, read/write mix) — script written
  (`scripts/bench_concurrency.py`), not run for real this session (time budget).
- Replication factor 1/2/3 resource cost — not benchmarked (per S9's own allowance,
  section 15, when not enough of the replication path is set up to benchmark
  meaningfully in the time available).
- Disk-pressure / quota testing (1-10GB) — same Docker Desktop macOS limitation
  already documented in the E2E phase's benchmark scope note: no hard per-volume byte
  quota exposed to `docker compose` without extra host-level setup this phase didn't
  build.
- Rate limiting — not re-tested; already measured and documented in the E2E phase
  (`docs/architecture/project-api-v1.md`): real, per-key, 60/min free / 600/min pro /
  6000/min enterprise (from `public.plans.rate_limit_per_minute`).
