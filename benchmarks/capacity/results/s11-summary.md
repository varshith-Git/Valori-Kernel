# S11 — Index Tuning & Product Defaults: Results

Real Docker benchmarks, same image/harness family as S9/S10
(`benchmarks/capacity/scripts/bench_ivf_bq.py` extended with
`--n-list`/`--n-probe`/`--bq-pool-factor`/`--bq-min-candidates`;
`bench_cell.py` with its restart-health timeout fixed from 60s→300s).
1GB/0.5vCPU (planned Free tier), 384D, deterministic seed 42 for all
50K-vector cells. HNSW cell at 10K/384D (S9's comparison scale).
Recall computed against exact-L2 ground truth (numpy-vectorized) over
the same inserted dataset — same method as S10.

## IVF parameter sweep (50K vectors, 384D)

| n_list | n_probe | Peak RSS | Insert vec/s | Build/Restart | p50 | p95 | Recall@1/5/10 |
|---:|---:|---:|---:|---:|---:|---:|---|
| 223 (auto-scale, S10) | 14 (auto) | 345.9 MB | 220.9 | **47.4s** | 664 ms | 838 ms | 1.0/1.0/1.0 |
| **64** | 8 | 343.3 MB | 224.2 | **16.9s** | 660 ms | 672 ms | 1.0/1.0/1.0 |
| 256 | 8 | 347.5 MB | 224.0 | 53.6s | 661 ms | 684 ms | 1.0/1.0/1.0 |
| 512 | 8 | 345.4 MB | 233.7 | 103.4s | 661 ms | 701 ms | 1.0/1.0/1.0 |
| 512 | 2 | 346.2 MB | 224.9 | 102.8s | 664 ms | 885 ms | 1.0/1.0/1.0 |

**Reading this:**
- **Search p50 is flat at ~660-664ms across every configuration tested**,
  including the most aggressive pruning (n_list=512, n_probe=2 — only
  ~196 vectors + 512 centroids scanned per query, ~1.4% of N). This
  is the direct answer to S11.2's primary question: **no tested IVF
  configuration achieves the required 25% p50 reduction** — the
  latency floor here is not coming from the candidate-scan cost IVF
  is supposed to reduce; something else (fixed per-request overhead —
  HTTP handling, JSON marshalling, or the 0.5vCPU ceiling itself)
  dominates at this scale, so pruning the scan doesn't move the
  number. This is a genuine, unexpected finding, stated plainly rather
  than reconciled away.
- **Recovery time is driven almost entirely by `n_list`, not `n_probe`**:
  16.9s→53.6s→103.4s tracks n_list=64→256→512 near-linearly (not
  N^1.5 as S10 hypothesized from only two N-varying points — S11's
  fixed-N, varying-n_list sweep isolates the real driver). n_probe
  has no measurable effect on recovery (103.4s at n_probe=8 vs 102.8s
  at n_probe=2, same n_list=512) — expected, since n_probe only
  affects `search()`, not `build()`'s k-means step.
  **Practical implication**: if IVF is ever used, a *small fixed*
  `n_list` (e.g. 64, disabling auto-scale) is a genuinely free win —
  same recall, same latency, but recovery drops from 47.4s to 16.9s
  purely by not letting `n_list` scale up with N.
- Recall stayed perfect (1.0) at every tested point, including the
  most aggressive pruning — on this deterministic uniform-random
  dataset. This does not rule out recall loss on real embeddings with
  non-uniform cluster structure; flagged as an explicit limitation,
  not extrapolated past what was measured.
- Memory stayed flat (343-348MB) across all n_list/n_probe values —
  consistent with S10's finding that IVF's memory overhead does not
  materialize at this scale regardless of centroid count.

**Conclusion**: IVF tuning does not clear the S11.2 bar (25% p50
reduction + Recall@10≥0.95 + no material recovery/memory regression).
The one actionable lever found — capping `n_list` low — only helps
recovery time, not the latency question this phase was primarily
asked to answer.

## BQ candidate-pool sweep (50K vectors, 384D)

| min_candidates | Peak RSS | Restart | p50 | p95 | Recall@1/5/10 |
|---:|---:|---:|---:|---:|---|
| 200 (default, S10) | 404.7 MB | 4.2s | 635 ms | 863 ms | 1.0 / 0.51 / 0.48 |
| **2,000** | 351.8 MB | 4.19s | 599 ms | 722 ms | 1.0 / 0.94 / 0.885 |
| **10,000** | 359.6 MB | 4.18s | 634 ms | 754 ms | 1.0 / **1.0** / **0.99** |

**Reading this:**
- **Recall@10 crosses the 0.95 bar at `min_candidates=10000`** (20% of
  N) — Recall@10=0.99, Recall@5=1.0. This directly answers S11.3's
  question: **yes, BQ can reach acceptable recall**, but only by
  widening the candidate pool to ~20% of the corpus — a real, working
  configuration, not a theoretical one.
- Peak RSS at the tuned setting (359.6MB) is still *lower* than
  BruteForce (751MB@100K scale; not directly comparable N, but at
  50K BruteForce was 349MB — so tuned BQ at 359.6MB is roughly
  comparable, slightly higher) and than IVF at any tested config
  (343-348MB) — BQ's two-HashMap memory model costs more per-vector
  than BruteForce's single HashMap, an expected and confirmed
  tradeoff, not a surprise.
- p50 (634ms at the tuned setting) stayed within noise of the default
  config's 635ms and of BruteForce's 671ms (S9) — the candidate-pool
  widening did not meaningfully worsen latency relative to the
  already-slow floor identified in the IVF sweep above; consistent
  with S11.1's audit finding that the Hamming-scan stage is already
  O(N) regardless of pool size, so widening the *re-rank* pool from
  200→10,000 adds comparatively little extra cost.
- Recovery stayed fast (4.18-4.19s) at every pool size — expected,
  since `min_candidates` only affects `search()`, never `build()`.
- **This is the one clear positive finding of S11**: a tuned BQ
  (`VALORI_BQ_MIN_CANDIDATES=10000` at 50K scale, i.e. ≈20% of N)
  reaches Recall@10=0.99 while keeping BQ's fast-recovery advantage
  and staying memory-competitive with BruteForce. It does **not**
  achieve a meaningful latency win over BruteForce (the S11.3 goal
  was "acceptable recall while retaining a meaningful performance or
  memory advantage" — recovery speed is the advantage retained here,
  not search latency or memory).

## HNSW (10K vectors, 384D — S11.4's single mandated benchmark)

First real attempt used the pre-existing 60s restart-health-check
timeout inherited from `bench_cell.py` and returned
`status: restart_failed` — the *exact* artifact already documented in
S9/S10 (60s is too short for HNSW's real recovery time). Fixed the
timeout to 300s (matching `bench_ivf_bq.py`'s already-fixed value) and
reran cleanly:

| Index | Insert vec/s | Peak RSS | p50 | p95 | Recovery | Hash match |
|---|---:|---:|---:|---:|---:|---|
| BruteForce (S9, same 10K/384D scale) | 1224.8 | 85.1 MB | 118.29 ms | 169.63 ms | 1.23s | ✅ |
| **HNSW** | **43.9-51.5** | 87.4-87.7 MB | 115.95-116.14 ms | 167.41-169.61 ms | **187.1s** | ✅ (after fix) |

**Reading this:**
- **Search latency is statistically tied** with BruteForce at this
  scale (115.95-116.14ms HNSW vs 118.29ms BruteForce) — HNSW's
  theoretical O(log N) advantage over BruteForce's O(N) has not
  materialized by 10K vectors; BruteForce is already fast enough here
  that there's nothing for HNSW to win.
- **Insert throughput is 24-28x worse** (43.9-51.5/s vs 1224.8/s) —
  HNSW's per-insert graph-neighbor search and heuristic pruning
  (`select_neighbors_heuristic`, `hnsw.rs`) is real, measurable cost,
  not a rounding error.
- **Recovery is ~150x worse** (187.1s vs 1.23s) — confirms and
  precisely re-measures S9's single data point (188s), now with a
  correctly-configured health check and a **real, matching
  BLAKE3 state hash** (the earlier apparent "restart_failed" was
  purely a benchmark-timeout artifact, not an integrity problem —
  same conclusion S9/S10 already reached, re-confirmed rather than
  assumed).
- Memory is close to BruteForce's (87.7MB vs 85.1MB) — the graph
  adjacency overhead (`Vec<Vec<u32>>` per node) is a small fraction of
  total memory at this scale, not the dominant cost people might
  expect from reading the code alone.

**Per S11.4's explicit stop rule** ("If HNSW clearly outperforms
BruteForce while maintaining high recall, run ONE additional point at
50K. Otherwise stop."): **HNSW does not clearly outperform
BruteForce** — search latency is tied, not better, and both insert
and recovery costs are dramatically worse. **The 50K HNSW point was
not run.**

## Cross-index recall summary (S11.2/S11.3 verdicts)

| Index | Meaningful latency win? | Recall@10 ≥ 0.95 achievable? | Verdict |
|---|---|---|---|
| IVF | **No** — flat ~660ms floor at every tested n_list/n_probe | Yes (was already 1.0) | Tuning does not help the latency problem it was meant to solve |
| BQ | No (default and tuned both ≈ BruteForce's latency) | **Yes**, at min_candidates≈20% of N | Tuning fixes the recall problem, at the cost of no longer being meaningfully faster |
| HNSW | No — tied with BruteForce at 10K, not measured further | N/A (not measured) | Does not clear the bar to justify further investment right now |
| BruteForce | N/A (baseline) | N/A (exact) | Remains the simplest, most predictable choice |
