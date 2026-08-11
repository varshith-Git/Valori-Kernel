# Capacity Benchmark Harness (S9)

Real Docker-based capacity benchmarking for `valori-node`, using the
same image the Local Cloud E2E environment builds
(`e2e/cloud/docker-compose.yml`'s `worker-a`/`worker-b`, tag
`cloud-worker-a:latest`) — nothing here is a separate/fake
implementation.

## Prerequisites

```bash
cd ../../e2e/cloud
docker compose build worker-a
```

This builds `cloud-worker-a:latest`, which every script here reuses
directly via `docker run` (not `docker compose` — each benchmark cell
needs its own resource limits and lifecycle, independent of the E2E
stack).

## Scripts

- `scripts/bench_cell.py` — one full capacity cell: start container with
  given RAM/CPU/dim/index limits, insert N vectors, measure memory
  throughout, measure search latency, do a real restart, verify the
  BLAKE3 state hash matches (S8 invariant — **exits non-zero and
  refuses to report the cell as valid if it doesn't**).
  ```bash
  python3 scripts/bench_cell.py --ram-mb 1024 --cpu 0.5 --dim 384 \
      --index brute --vectors 100000 --name my_scenario --port 3500
  ```
- `scripts/bench_dimensions.py [dims...]` — dimension sweep at a fixed
  vector count/RAM/CPU/index (defaults 384/768/1024/1536; pass explicit
  dims to run a subset).
- `scripts/bench_index_types.py` — brute/hnsw/ivf/bq comparison at a
  fixed small scale (feasibility + relative comparison, not an
  at-scale test).
- `scripts/bench_multiproject.py` — N sibling containers (simulating N
  projects on one host) running concurrently; measures contention and
  re-verifies cross-project token isolation under concurrent load.
- `scripts/bench_collections.py` — N collections × fixed vectors/collection
  in one container.
- `scripts/bench_concurrency.py` — concurrent client load against one
  pre-loaded container.
- `scripts/bench_ivf_bq.py` (S10, extended S11) — IVF/BQ capacity cell
  with recall@k measurement against a numpy-computed exact-L2 ground
  truth over the same deterministic dataset (no second container
  needed). Same restart-hash verification as `bench_cell.py`. S11
  added `--n-list`/`--n-probe`/`--bq-pool-factor`/`--bq-min-candidates`
  to sweep IVF/BQ tuning parameters via the corresponding
  `VALORI_IVF_N_LIST`/`VALORI_IVF_N_PROBE`/`VALORI_BQ_POOL_FACTOR`/
  `VALORI_BQ_MIN_CANDIDATES` env vars.
  ```bash
  python3 scripts/bench_ivf_bq.py --index ivf --dim 384 --vectors 100000 \
      --ram-mb 1024 --cpu 0.5 --port 4500 --name my_ivf_scenario \
      --n-list 64 --n-probe 8
  ```

## Results

`results/*.json` — one file per scenario/sweep, real measured output.
`results/results.json` / `results/results.csv` — flattened combination
of every scenario file, for spreadsheet/analysis use.
`results/summary.md` — human-readable tables with interpretation.
`results/capacity_model.md` — derived formulas, explicitly labeled
MEASURED / INTERPOLATED / EXTRAPOLATED.

## Re-running

Every script is self-contained and idempotent (removes its own
containers/volumes before and after). Re-run any script standalone to
refresh its results file; nothing here depends on run order except
"don't run two CPU-bound scripts at the same time on the same host" —
that skews both.

## S10 addendum — IVF/BQ finding

Real measurement (`results/s10-summary.md`) found neither IVF nor BQ
meaningfully improves on BruteForce capacity in the current
implementation/configuration — IVF is latency-neutral-to-worse with a
severe (N^1.5-scaling) recovery-time penalty; BQ trades real recall
(Recall@10 ≈ 0.48 at 50K) for negligible latency gain. See
`docs/phases/phase-S10-index-capacity.md` for the full analysis.

## S11 addendum — index tuning finding

Real parameter sweeps (`results/s11-summary.md`) found: IVF's search
latency is flat (~660ms) across every tested `n_list`/`n_probe`
combination — no configuration achieves a meaningful latency win, but
recovery time scales linearly with `n_list` independent of `n_probe`
(a small fixed `n_list` is a real, free recovery-time win if IVF is
ever used). BQ's recall is fixable: `VALORI_BQ_MIN_CANDIDATES=10000`
(~20% of a 50K corpus) reaches Recall@10=0.99, at the cost of no
longer being meaningfully faster than BruteForce. HNSW (one cell at
10K, per the phase's own stop rule) ties BruteForce on search latency
while being dramatically worse on insert throughput and recovery time
— the mandated 50K follow-up was not run. **BruteForce remains the
recommended Free-tier default.** See
`docs/phases/phase-S11-index-tuning.md` and
`docs/reviews/index-tuning-audit.md` for the full analysis.

## Known limitations (see `results/summary.md` for the full list)

- Vector counts above 100K (BruteForce) / 10K (HNSW/IVF/BQ) not run for
  real — extrapolated only, see `capacity_model.md`.
- Collections and concurrency scripts exist but weren't run at scale
  this session — explicit S10 follow-up.
- Disk-pressure/quota testing not implemented — Docker Desktop on
  macOS has no hard per-volume byte quota exposed to `docker compose`
  without extra host setup.
- Replication resource cost not benchmarked.
