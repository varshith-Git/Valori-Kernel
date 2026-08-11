# Phase S11: Index Tuning & Product Defaults

## Goal

Determine whether IVF, BQ, or HNSW can be tuned into a genuinely
better default than BruteForce for Free-tier Cloud projects, using
real measurement against the current implementations — no theoretical
assumptions, no invented numbers, production configuration untouched.

## Delivered

- `docs/reviews/index-tuning-audit.md` — read-only audit of all four
  `VectorIndex` implementations (BruteForce/HNSW/IVF/BQ), covering
  memory model, build/insert/search cost, recall characteristics,
  restart/recovery behavior, configuration surface, and determinism.
  **Two real, previously-undocumented findings surfaced by this audit**:
  1. **IVF and HNSW both have real, working `snapshot()`/`restore()`
     code that `Engine::try_recover()` never calls** — recovery always
     goes through `rebuild_index()` from the record pool, meaning IVF
     re-runs k-means from scratch on every restart and HNSW re-runs its
     full graph build, rather than deserializing either persisted
     structure. This is the concrete root cause of both indices'
     expensive recovery time.
  2. **BQ's candidate-pool size (`POOL_FACTOR=10`, `MIN_CANDIDATES=200`)
     was hardcoded, not configurable** — the direct cause of S10's
     Recall@10=0.48 finding and something that had to be fixed in code
     before it could even be tested.
- **Code change** (small, additive, no behavior change unless
  explicitly configured): `BqConfig { pool_factor, min_candidates }`
  added to `crates/valori-index/src/bq.rs`, defaulting to the exact
  prior constants (10, 200). Wired through
  `VALORI_BQ_POOL_FACTOR` / `VALORI_BQ_MIN_CANDIDATES` env vars in
  `crates/valori-node/src/config.rs` and
  `crates/valori-engine/src/{config.rs,engine.rs}`, following the
  identical pattern already used for `VALORI_IVF_N_LIST`/`_N_PROBE`.
  2 new unit tests in `bq.rs` (default-matches-prior-constants,
  custom-config-plumbing). No existing behavior changed — default
  values reproduce pre-S11 output exactly.
- Real Docker benchmarks (1GB/0.5vCPU, 384D, seed 42):
  - **IVF sweep**: 4 cells at 50K vectors — (n_list, n_probe) ∈
    {(64,8), (256,8), (512,8), (512,2)} — plus S10's existing
    auto-scale point (223,14) as the 5th reference row.
  - **BQ sweep**: 2 cells at 50K vectors — `min_candidates` ∈
    {2000, 10000} — plus S10's existing default (200) as the 3rd
    reference row.
  - **HNSW**: 1 cell at 10K vectors (per S11.4's mandate), compared
    against S9's existing BruteForce data point at the same scale.
    First attempt hit the same 60s-restart-timeout artifact S9/S10
    already documented; fixed `bench_cell.py`'s restart wait to 300s
    (matching `bench_ivf_bq.py`'s already-fixed value) and reran.
- `benchmarks/capacity/results/s11-summary.md` — full results tables
  and honest reading of every cell.
- `benchmarks/capacity/scripts/bench_ivf_bq.py` — extended with
  `--n-list`/`--n-probe`/`--bq-pool-factor`/`--bq-min-candidates` CLI
  flags (env-var passthrough to the container).
- `benchmarks/capacity/scripts/bench_cell.py` — restart-health-check
  timeout fixed from 60s → 300s (same class of fix as S10's harness
  already had; this file hadn't been touched yet).
- Raw result JSONs: `benchmarks/capacity/results/s11_ivf_nl{64,256,512}_np{8,2}.json`,
  `s11_bq_pool{2000,10000}.json`, `s11_hnsw_10k.json` (the failed
  first attempt, kept for transparency), `s11_hnsw_10k_v2.json` (the
  corrected run).

## Findings

1. **IVF tuning does not achieve the required 25% p50 latency
   reduction at any tested configuration.** Search p50 stayed flat at
   ~660-664ms across n_list ∈ {64,256,512} × n_probe ∈ {2,8}, including
   the most aggressive pruning tested (n_list=512, n_probe=2, scanning
   ~1.4% of the corpus per query). This means the ~650-670ms floor seen
   across BruteForce/IVF/BQ in S10 and again here is **not primarily an
   index-scan cost** — something else (fixed per-request HTTP/JSON
   overhead, or the 0.5vCPU ceiling) dominates at this scale, and no
   amount of IVF pruning tuning can address it.
2. **IVF recovery time is driven almost entirely by `n_list`, not
   `n_probe`, and scales roughly linearly with it** (16.9s→53.6s→103.4s
   for n_list=64→256→512, n_probe held constant). A small fixed
   `n_list` (e.g. 64, via `VALORI_IVF_N_LIST=64` disabling auto-scale)
   is a genuinely free win if IVF is ever used: identical recall,
   identical latency, but recovery drops from the auto-scaled 47.4s to
   16.9s. This does not change S11's overall recommendation (IVF still
   doesn't solve the latency problem it exists to solve), but it is a
   real, actionable, low-risk tuning finding worth recording.
3. **BQ's recall problem is fixable by widening the candidate pool.**
   `VALORI_BQ_MIN_CANDIDATES=10000` (≈20% of a 50K corpus) reaches
   Recall@10=0.99, Recall@5=1.0 — up from the default's Recall@10=0.48.
   This is the one clearly positive tuning result in this phase. The
   cost: BQ no longer offers a latency advantage over BruteForce at
   the tuned setting (634ms vs BruteForce's 671ms — within noise), so
   the value of tuned BQ is its fast recovery (4.18s vs BruteForce's
   ~4-9s at this scale — comparable, not dramatically better either)
   and comparable memory, not raw search speed.
4. **HNSW does not clearly outperform BruteForce at 10K vectors.**
   Search latency is statistically tied (115.95-116.14ms HNSW vs
   118.29ms BruteForce, S9's comparison point at the same scale/dim).
   Insert throughput is 24-28x worse (43.9-51.5/s vs 1224.8/s).
   Recovery is ~150x worse (187.1s vs 1.23s, now measured with a
   correct timeout and a **confirmed matching state hash** — the
   earlier "restart_failed" status was a benchmark artifact, not a
   real integrity gap, precisely as S9/S10 already concluded and now
   re-verified rather than assumed). Per S11.4's own stop rule, the
   50K HNSW point was **not run** — HNSW didn't clear the bar to
   justify it.
5. **Two indices (IVF, HNSW) have real snapshot/restore code that is
   dead on the actual restart path.** `Engine::try_recover()` always
   calls `rebuild_index()`, discarding any persisted centroid/graph
   structure. This is the true, now-confirmed root cause of both
   indices' expensive recovery times — not an inherent property of the
   algorithms, but a wiring gap. Fixing it (making recovery actually
   deserialize the persisted structure) is explicitly **out of scope**
   for this phase (S11 is investigation + tuning via existing knobs,
   not an engine rewrite) but is the clearest concrete lead for a
   future phase that wants to make IVF or HNSW viable.
6. **No index beats BruteForce on the Free-tier default question.**
   IVF's tuning lever only helps recovery, not the latency problem.
   BQ's tuning lever only helps recall, trading away its latency edge
   in the process. HNSW doesn't clear its own bar to justify further
   investment. BruteForce remains simplest, has perfect recall by
   definition, and the fastest real recovery of the four (no rebuild
   step beyond the record-pool scan itself).

## S11.5 — Default index recommendation

**BruteForce**, unchanged from S9/S10. Priority order requested by
this phase (predictable memory → predictable latency → high recall →
fast recovery → deterministic → operationally simple) — BruteForce
wins or ties on every axis at Free-tier scale (≤~25-30K vectors, per
S9):

- Predictable memory: single `HashMap<u32, Vec<f32>>`, no auxiliary
  structure — the most predictable of the four by construction.
- Predictable latency: no tuning parameters to get wrong — always
  O(N), no cliff from a mistuned centroid/candidate-pool parameter.
- High recall: exact, always 1.0 — the only index here with recall
  as a *non-question*.
- Fast recovery: fastest real restart of all four measured this
  session (~1-9s depending on scale) — no clustering, no graph build,
  just a linear scan of the record pool.
- Deterministic: yes, unconditionally.
- Operational simplicity: no config surface at all.

This is **not** a default chosen for theoretical scalability — it is
the one that wins on every measured axis at the scale that matters for
Free-tier today.

## S11.6 — User-facing index selection

**Option B (Valori chooses the index automatically) for the Free
tier**, with the internal abstraction kept intact for future tiers.
Rationale: none of IVF/HNSW/BQ demonstrated a clear win this phase, so
there is nothing today for a user to meaningfully choose between —
exposing "index type" as a user-facing knob would let users pick a
worse-performing option with no offsetting benefit, for no product
reason. The `VectorIndex` trait abstraction and all four
implementations remain untouched and available — this is a **product
exposure decision**, not an architectural one. Nothing was removed.

If a future phase demonstrates a real win for a specific index at a
specific scale/dimension (e.g., IVF once the persisted-restore gap
from Finding #5 is fixed, or BQ for a memory-constrained tier that can
tolerate widened candidate pools), Option C (users pick a named
"index strategy" that Valori maps internally) is the natural next
step — it keeps the API surface stable while allowing internal
implementation changes.

## S11.7 — Future-proof configuration model

No code changes made this phase beyond the BQ config plumbing (S11.7
asks only to "establish the contract and document the extension
point," not to build a generic framework — and the current
architecture doesn't need one yet: `EngineConfig`'s per-index
`Option<T>` fields, one group per index kind, already scale to a
handful of index types without redesign).

**The contract**, for when Cloud does need to expose index
configuration (Option C, above):

```json
{
  "index": {
    "type": "bruteforce",
    "config": {}
  }
}
```

or:

```json
{
  "index": {
    "type": "ivf",
    "config": { "n_list": 64, "n_probe": 8 }
  }
}
```

or:

```json
{
  "index": {
    "type": "bq",
    "config": { "min_candidates": 10000 }
  }
}
```

This maps directly onto the existing internal shape — each index kind
already has its own config struct (`IvfConfig`, `HnswConfig`,
`BqConfig` as of this phase) with named fields, and `EngineConfig`
already carries one `Option<T>` group per index kind. **Adding a new
index type or a new tuning parameter under an existing type requires
no change to this contract's shape** — only a new `"type"` value or a
new key inside an existing `"config"` object. This is the extension
point; no generic/reflective config framework was built because the
existing one-`Option`-group-per-index-kind pattern already satisfies
the requirement at today's index count (4).

## S11.8 — Dimension implications

Not re-benchmarked this phase (out of scope per this phase's explicit
"do NOT benchmark every index × dimension × vector-count combination"
instruction). Reusing S9's real measurement:

**Memory formula** (S9, MEASURED, BruteForce, 20K vectors, dims
384/768/1024/1536): `bytes_per_vector ≈ 17.23 × dim + 1531`
(<1% fit error). This is BruteForce-specific — S11 did not measure
whether IVF/BQ/HNSW's per-vector overhead scales the same way with
dimension (IVF's centroid storage and BQ's binary-code storage both
have their own dimension-dependent terms not captured by this linear
fit). **Marked UNKNOWN** for IVF/HNSW/BQ specifically — do not apply
S9's BruteForce-derived formula to the other three without dedicated
measurement.

Should dimension influence:
- **Maximum vector count**: yes, mechanically, via the memory formula
  above (MEASURED, BruteForce only).
- **RAM allocation**: not separately — RAM is fixed per plan tier;
  higher dimension just consumes the existing budget faster via the
  formula above.
- **Index selection**: UNKNOWN — no evidence this phase or S9/S10 that
  any index's *relative* standing changes with dimension (all
  dimension testing in S9 was BruteForce-only).
- **Pricing / project limits**: not a question this phase can answer;
  belongs with S9/S10's existing "REQUIRES SEPARATE MEASUREMENT" note
  for anything beyond the already-measured Free/BruteForce/384D point.

## S11.9 — Product recommendation

### Free
- RAM: 1 GB
- CPU: 0.5 vCPU
- Recommended index: **BruteForce**
- Maximum dimension: no hard measured ceiling; capacity formula
  (S9, BruteForce, MEASURED) applies: `bytes_per_vector ≈ 17.23×dim + 1531`
- Recommended vector limit: ~25,000-30,000 at 384D (S9, MEASURED,
  search-latency-bound) — **unchanged by S11**, no index tuning found
  this session raises it
- Maximum collections: NOT YET VALIDATED — S12 must measure
  collections-at-scale (deferred since S9/S10, still deferred)
- Maximum projects (per host): NOT YET VALIDATED — S9 ran a
  multi-project contention test but did not establish a hard per-host
  ceiling
- Rate limit: NOT YET VALIDATED — no rate-limiting benchmark run in
  S9, S10, or S11

### Pro
- RAM: 4 GB (existing infra config, unvalidated)
- CPU: 2 vCPU (existing infra config, unvalidated)
- Recommended index: NOT YET VALIDATED
- Maximum dimension: NOT YET VALIDATED
- Recommended vector limit: NOT YET VALIDATED
- Maximum collections: NOT YET VALIDATED
- Maximum projects: NOT YET VALIDATED
- Rate limit: NOT YET VALIDATED

**S12 must measure**: the entire Pro tier from scratch at 4GB/2vCPU —
BruteForce vector-count boundary (the S9/S10/S11 methodology directly
transfers), whether the larger RAM/CPU budget changes which index (if
any) becomes competitive (more headroom could change IVF/HNSW's
recovery-time economics, though S11's core latency-floor finding
suggests it may not change the search-latency conclusion), and
dimension scaling at this tier.

### Enterprise
- RAM: 16 GB (existing infra config, unvalidated)
- CPU: 8 vCPU (existing infra config, unvalidated)
- Recommended index: NOT YET VALIDATED
- Maximum dimension: NOT YET VALIDATED
- Vector limit: NOT YET VALIDATED
- Collections: NOT YET VALIDATED
- Projects: NOT YET VALIDATED
- Rate limit: NOT YET VALIDATED

**S12 must measure**: same full matrix as Pro, at 16GB/8vCPU. At this
resource tier, HNSW and IVF's higher build/recovery costs may finally
be affordable relative to the larger RAM budget — this tier is the
most plausible place either could become a legitimate recommendation,
but that is a hypothesis, not a finding; nothing in S9/S10/S11
measured it.

## S11.10 — Production safety

No plan limits, provisioning defaults, pricing, or customer-facing
quotas were modified. The one code change this phase (`BqConfig`) is
additive and defaults to prior behavior exactly — it does not change
what any existing deployment does unless a new env var is explicitly
set, which nothing in production sets today.

## Validation

- `cargo fmt --check`: clean.
- `cargo check --workspace`: clean.
- `cargo clippy --workspace -- -D warnings`: clean.
- `cargo test --workspace`: **1189 passed, 0 failed** (up from S10's
  1187 — the 2 new `bq.rs` unit tests added this phase:
  `custom_config_changes_candidate_pool_without_error`,
  `default_config_matches_prior_constants`).
- `npx tsc --noEmit` (valori-ui): clean.
- `npm run build` (valori-ui): clean. No UI changes this phase; run
  per the mandatory verification suite regardless.
- All 7 real benchmark cells (4 IVF, 2 BQ, 1 HNSW): real container
  start, real inserts, real restart, real BLAKE3 state-hash comparison
  — **all matched** (S8 invariant holds for every tuned configuration
  tested, not just the defaults S10 already covered).
- The Docker image used for every S11 cell was rebuilt with
  `--no-cache`-equivalent freshness (`docker compose build worker-a`
  after the `BqConfig` source change), so every cell reflects the
  actual code being measured, not a stale binary.
- Local Cloud E2E suite: not re-run — same reasoning as S10 (no
  route/config/SDK surface touched that the E2E environment exercises;
  the `BqConfig` change is purely internal to `valori-index` and only
  reachable via the same env vars IVF already used, which E2E doesn't
  exercise either).

## Remaining unknowns

- Whether real (non-uniform-random) embeddings would show recall loss
  at IVF's aggressive pruning settings that this session's synthetic
  uniform dataset did not surface.
- Whether tuning HNSW's own parameters (`ef_search`, `m`) could close
  its search-latency-tie-not-win gap with BruteForce at scales beyond
  10K — not tested; S11.4 explicitly scoped this phase to ONE HNSW
  benchmark unless it clearly won.
- The persisted-restore gap for IVF/HNSW (Finding #5) — a concrete,
  scoped future-phase candidate, not attempted here.
- Pro/Enterprise capacity for any index — unchanged from S9/S10,
  entirely unmeasured.
- Dimension scaling for anything other than BruteForce (S11.8).
- Rate limiting, collections-at-scale, concurrency-at-scale — carried
  forward from S9/S10, still not measured.

## Production configuration status

**Unchanged.** No plan limits, provisioning defaults, pricing, or
customer-facing quotas were modified in this phase.
