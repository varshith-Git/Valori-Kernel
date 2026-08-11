# Phase S10: IVF & Binary Quantization Capacity Validation

## Goal

Determine whether IVF or Binary Quantization can materially increase
Valori's usable Free-tier capacity beyond the ~25,000-30,000 vector
BruteForce boundary S9 established (search-latency-bound at 1GB/0.5vCPU,
384D). Measurement-first — no theoretical assumptions, no invented
numbers, production configuration untouched.

## Delivered

- `docs/reviews/index-capacity-audit.md` — read-only audit of the real
  IVF/BQ implementations (`crates/valori-index/src/{ivf,bq}.rs`), done
  before any benchmark ran.
- `benchmarks/capacity/scripts/bench_ivf_bq.py` — extends the S9
  harness with recall@k measurement (numpy-vectorized exact-L2 ground
  truth over the same deterministic dataset, no second container) and
  the early-stop rules this phase specifies.
- 3 real measured cells: IVF 384D@50K, IVF 384D@100K, BQ 384D@50K —
  each including a real restart + BLAKE3 state-hash verification.
- `benchmarks/capacity/results/s10-summary.md` — the human-readable
  head-to-head comparison against S9's BruteForce baseline.

## Findings

1. **Neither IVF nor BQ, as currently implemented, meaningfully
   increase usable capacity beyond BruteForce.** This is the direct,
   evidence-based answer to this phase's core question — a real,
   negative finding, not a null result from insufficient testing.
2. **IVF's memory overhead prediction (from reading the code — inverted
   lists duplicate every vector) did NOT hold at measured scale.**
   345.9MB (IVF) vs 349MB (BruteForce) at 50K, 738.6MB vs 750.7MB at
   100K — statistically indistinguishable. The code audit's structural
   prediction was real (the duplication genuinely exists in the code)
   but its magnitude didn't materialize at these scales — flagged
   explicitly rather than silently reconciled.
3. **IVF's search latency showed no improvement, and got measurably
   worse at the tail as scale increased**: 664ms→1332ms p50 (roughly
   tracking BruteForce's own degradation, not better), and p95/p99
   degraded to 2131ms at 100K vs BruteForce's 1559/1634ms — worse, not
   better. Recall stayed perfect (1.0) at both scales, which is the
   root cause: the auto-scaled `n_probe`/`n_list` parameters aren't
   pruning the search space aggressively enough to realize IVF's
   theoretical sub-linear advantage — it pays the bookkeeping cost
   without collecting the algorithmic benefit.
4. **IVF's recovery time is a second, independent operational cost**:
   47.4s at 50K, 129.7s at 100K — scaling roughly as N^1.5 (consistent
   with k-means cost scaling with both cluster count `√N` and vector
   count `N`), on top of the index never being persisted (rebuilt from
   scratch every restart, confirmed in the S8/S9/S10 audits).
5. **BQ trades real recall for negligible latency gain, at higher
   memory cost.** 404.7MB (highest of the three tested at 50K),
   635ms p50 (only ~5% faster than BruteForce's 671ms), and
   **Recall@10 = 0.48** — the implementation's two-stage design
   (Hamming-distance candidate filter, then exact re-rank) loses nearly
   half the true top-10 neighbors before the re-rank stage ever sees
   them. BQ's one genuine advantage: fast recovery (4.2s, no clustering
   step needed).
6. **The deep matrix (250K-500K vectors, 768D/1536D) was deliberately
   not run.** Both indices already showed a clear signal at the first
   two priority checkpoints; per this phase's own instruction not to
   "complete a spreadsheet," continuing would extend the same
   qualitative finding without a compelling reason to expect a
   different outcome, while costing significant additional time
   (single 100K-scale cells took 13-15 minutes wall-clock each in this
   environment).

## Validation

- All 3 cells: real restart, real BLAKE3 state-hash comparison —
  **all matched** (S8 fix holds for both IVF and BQ, not just
  BruteForce).
- Recall computed against real exact-L2 ground truth over the actual
  inserted dataset (not a theoretical/assumed value) — numpy-vectorized
  for speed, same underlying arithmetic as a literal brute-force scan.
- `cargo fmt --check`, `cargo check --workspace`, `cargo test
  --workspace`, `cargo clippy --workspace -- -D warnings`: all clean —
  no source code was changed this phase (benchmarking + documentation
  only), confirmed by re-running the same suite S9 already passed.
- `npx tsc --noEmit`: clean (no UI changes this phase either).
- Local Cloud E2E suite: not re-run this phase — no code touched that
  the E2E environment exercises; the last real run (end of S9, with
  every S8+S9 change baked in via a genuine `--no-cache` rebuild) is
  still the accurate, current status. Re-running it here would not
  test anything new and was skipped rather than performed as
  theater.

## Answer to the S10 executive question

**Can IVF or BQ materially increase the Free-tier capacity beyond the
current ~25K-30K BruteForce estimate?**

No — not with the current implementation and default configuration.
IVF is performance-neutral-to-worse with a severe recovery-time
penalty that grows faster than the workload. BQ is memory-worse with a
real, measurable recall cost for a latency improvement too small to
matter. **RECOMMENDED, NOT APPLIED**: the Free-tier vector quota should
continue to be derived from BruteForce's measured performance boundary
(~25,000-30,000 vectors at 384D, 1GB/0.5vCPU) until either (a) IVF's
parameter auto-scaling is tuned to actually prune the search space
more aggressively (trading some recall for real latency improvement,
which it currently isn't doing), or (b) a different index
implementation is measured and shown to genuinely outperform
BruteForce at this scale.

**Index-specific plan limits are not currently justified by the
evidence** (Option A over Option B, per this phase's own framing) —
there's no measured index that's clearly better than BruteForce to
carve out a higher quota for. If IVF's parameters are tuned in a
follow-up and shown to help, that recommendation would change.

## Remaining unknowns

- Pro/Enterprise capacity (any index type) — REQUIRES SEPARATE
  MEASUREMENT, not attempted this phase.
- IVF/BQ at 250K-500K vectors — NOT MEASURED (see Findings #6).
- IVF/BQ at 768D/1536D — NOT MEASURED.
- Whether tuning `VALORI_IVF_N_LIST`/`VALORI_IVF_N_PROBE` away from
  auto-scale defaults would change IVF's latency/recall tradeoff —
  NOT MEASURED, but directly suggested by Finding #3 as the most
  promising lever if IVF is to be revisited.
- HNSW at any scale beyond S9's single 10K data point — NOT MEASURED.
- Replication, disk-pressure, collections-at-scale, concurrency-at-scale,
  CPU as an independent variable — unchanged from S9's own "Remaining
  unknowns," not revisited this phase.

## Production configuration status

**Unchanged.** No plan limits, provisioning defaults, pricing, or
customer-facing quotas were modified in this phase.
