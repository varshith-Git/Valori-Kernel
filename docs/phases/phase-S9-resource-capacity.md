# Phase S9: Resource Capacity & Plan Limits

## Goal

Determine, using real Docker-based measurements against the real
`valori-node` binary, what Valori can safely support on different
worker sizes, and derive evidence-based Free/Pro/Enterprise limits —
measurement-first, no invented numbers, no copied competitor limits,
no production changes until the evidence is reviewed.

## Delivered

- `docs/reviews/resource-capacity-audit.md` — read-only audit of the
  kernel's memory/storage model and the existing (unvalidated) Cloud
  provisioning schema, done before any benchmark ran.
- `benchmarks/capacity/` — real Docker benchmark harness reusing the
  actual `cloud-worker-a` image (see its own README). 6 scripts, all
  exercising the real `valori-node` binary with real
  `docker run --memory`/`--cpus` limits.
- 19 real measured result records across: RAM boundary (512MB/1GB at
  384D/BruteForce, 10K-100K vectors), index-type comparison
  (brute/hnsw/ivf/bq at 10K), dimension sweep (384/768/1024/1536 at
  20K), and multi-project contention (1/2/4 concurrent projects).
- `benchmarks/capacity/results/capacity_model.md` — derived formulas
  (`actual_bytes_per_vector(dim)`, safe-vector-count estimate),
  explicitly labeled MEASURED/INTERPOLATED/EXTRAPOLATED throughout, per
  the instruction never to present a projection as a fact.
- A **proposed** (not applied) Free/Pro/Enterprise limits table — see
  the final report; production plan config was not touched.

## Findings

1. **The existing `max_records_per_project = 1,000,000` free-tier quota
   was never benchmark-validated** (confirmed via the audit — the
   migration's own comment says these were "sized off the
   recommendation in the architecture discussion"). Real measurement
   puts the search-latency-driven safe limit at roughly 29,300 vectors
   for BruteForce at 384D on the planned 1GB/0.5vCPU free worker — a
   ~34x gap from the current unvalidated number.
2. **Search latency, not memory, is the binding constraint** for
   BruteForce at realistic scale. Doubling RAM (512MB→1GB) at 100K
   vectors left p50 search latency unchanged (1308ms→1307ms) — proving
   the bottleneck is CPU-bound O(N×dim) scan cost, not memory pressure.
   No amount of extra RAM fixes this; only a real index (or more CPU)
   would.
3. **HNSW recovery time is a serious, real operational concern.**
   Confirmed correct (state hash matches — S8 fix holds) but took 188
   seconds to recover just 10,000 vectors at dim=384 — 35-160x slower
   than BruteForce/IVF/BQ's 1-5 second recovery. Every worker restart
   (deploy, crash, maintenance) on an HNSW-indexed collection means
   multi-minute downtime that gets worse as vector count grows. Not
   independently caught by any prior review — only surfaced by
   deliberately measuring a real restart under real load.
4. **Actual memory per vector is ~4.6-5.3x the raw Q16.16 byte size**
   (`dim × 4 bytes`), not equal to it — confirmed by measurement across
   4 dimensions with a consistent linear fit
   (`17.23 × dim + 1531` bytes/vector, <1% error at an independent
   cross-check point). Any capacity estimate based on
   `dimension × sizeof(float)` alone would be wrong by roughly 5x.
5. **Restart RSS consistently exceeds live-insert peak RSS by 5-11%**
   across every tested configuration — capacity planning must size for
   the recovery peak, not steady-state.
6. **Multi-project isolation holds under real concurrent load**: 4
   sibling containers under simultaneous insert load showed roughly
   even throughput degradation (no evidence of one project starving
   another) and cross-project `worker_auth_token` checks correctly
   returned 401 in every case tested, concurrently.
7. **BQ and IVF were only feasibility-tested at small scale (10K
   vectors)**, where nothing differentiates them from BruteForce yet —
   whether either meaningfully raises the *performance* limit at 100K+
   scale (their algorithmic design suggests they should) is genuinely
   unmeasured and is the single highest-value next benchmarking task.

## Validation

- All restart-hash checks across every scenario: **match**, confirming
  the S8 fix holds across every index type and dimension tested — the
  one stop-condition (`state hash fails after restart`) never
  triggered in this phase.
- Cross-project token isolation: verified concurrently under real
  multi-project load, not just in isolation.
- No OOM kill was observed in any tested cell — the worst case (512MB
  at 100K vectors/384D) sat at 99-100% memory for several minutes
  without crashing, but with severely degraded throughput and latency;
  the container survived, it just wasn't operating safely.
- `cargo test --workspace`, `cargo clippy --workspace`, `npx tsc
  --noEmit`, `npm run build`, and a clean `docker compose down -v &&
  build --no-cache && up -d` — see the final S9 report for exact
  results (run after this doc, verification is the last step before
  the report).

## Follow-ups (explicit S10 candidates)

1. **IVF/BQ at realistic scale (100K+)** — the highest-value remaining
   question: do they preserve sub-linear search latency where
   BruteForce doesn't? This directly determines whether the free-tier
   quota can be materially higher than the BruteForce-derived 29,300
   estimate.
2. **HNSW recovery-time root cause** — is 188s for 10K vectors
   expected given the algorithm, or is there a real optimization
   opportunity (e.g., persisting the graph instead of rebuilding it on
   every restart)? Currently the index is never persisted, only
   rebuilt from the record pool (confirmed in the S8/S9 audits).
3. Vector counts above 100K (BruteForce) / 10K (other indices) — not
   run for real; the capacity model's extrapolation should be replaced
   with real measurement before being trusted at that scale.
4. Collections-scaling and concurrency scripts exist
   (`benchmarks/capacity/scripts/bench_{collections,concurrency}.py`)
   but weren't run at meaningful scale this session.
5. Replication resource cost, disk-pressure/quota behavior — not
   measured (see `summary.md`'s "NOT MEASURED" section for why).
6. CPU as an independent axis (1 vCPU, 2 vCPU) — every cell in this
   phase used 0.5 vCPU; near-50% utilization observed during inserts
   suggests more CPU would meaningfully help, but this wasn't isolated
   from RAM/dim/index as its own variable.
