# S10 — IVF / BQ Capacity & Recall Results

Real Docker benchmarks, same image/harness family as S9
(`benchmarks/capacity/scripts/bench_ivf_bq.py`), 1GB/0.5vCPU (the
planned Free tier), 384D, deterministic seed 42. Recall computed
against exact-L2 ground truth over the same dataset (numpy-vectorized,
not a second container).

## Head-to-head: BruteForce (S9) vs IVF vs BQ, 384D

| Index | Vectors | Peak RSS | Restart RSS | Insert vec/s | p50 | p95 | p99 | Recovery | Recall@10 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| BruteForce (S9) | 50K | 349 MB | 389 MB | 245.8 | 671 ms | — | — | ~4s | n/a (exact) |
| **IVF** | 50K | 345.9 MB | 392.0 MB | 220.9 | 664 ms | 838 ms | 838 ms | **47.4s** | 1.0 |
| **BQ** | 50K | 404.7 MB | 395.1 MB | 231.1 | 635 ms | 863 ms | 863 ms | **4.2s** | 0.48 |
| BruteForce (S9) | 100K | 751 MB | 776.5 MB | 136.2 | 1307 ms | 1559 ms | 1634 ms | ~9s | n/a (exact) |
| **IVF** | 100K | 738.6 MB | 781.1 MB | 127.2 | 1332 ms | **2131 ms** | **2131 ms** | **129.7s** | 1.0 |

BQ was not re-run at 100K — see "Why the deep matrix was stopped" below.

## Reading this, honestly

**IVF gives no measurable benefit over BruteForce at either 50K or
100K vectors, and is worse in every other dimension:**
- Memory: statistically indistinguishable from BruteForce (contradicts
  this phase's own code-audit prediction of ~2x overhead from
  duplicated vectors in the inverted lists — a real surprise, not
  silently reconciled: see `docs/reviews/index-capacity-audit.md`).
- Latency: essentially the same at 50K (664ms vs 671ms), *slightly
  worse* at 100K (1332ms vs 1307ms p50), and clearly worse tail latency
  at 100K (2131ms p95/p99 vs 1559/1634ms for BruteForce).
- Recall: perfect (1.0) at both scales — which, combined with the
  latency finding, means IVF's auto-scaled `n_probe`/`n_list`
  parameters aren't actually pruning the search space meaningfully at
  this scale. It's doing IVF's *bookkeeping* overhead without getting
  IVF's *algorithmic* benefit.
- Recovery: dramatically worse and scales badly — 47s at 50K, 130s at
  100K (≈ N^1.5, consistent with k-means cost scaling with both cluster
  count `sqrt(N)` and vector count `N`). At real Free-tier scale
  (25-30K vectors, per S9), this would still mean a real, measurable
  restart penalty over BruteForce's near-instant recovery.

**BQ trades meaningful accuracy for essentially no latency gain, and
costs more memory:**
- Memory: 404.7MB — the highest of the three at 50K (matches the audit's
  prediction directionally: BQ duplicates both the binary codes and the
  full f32 vectors).
- Latency: 635ms p50 — only ~5% faster than BruteForce's 671ms. Not a
  meaningful product-facing improvement.
- **Recall@5 = 0.51, Recall@10 = 0.48** — the real cost. Nearly half of
  the true top-10 neighbors are missed. Recall@1 = 1.0 masks this:
  the single best match is robust to quantization noise, but ranks
  2-10 are not reliably recovered by the Hamming-distance candidate
  filter before the exact re-rank stage.
- Recovery: genuinely fast (4.2s) — no clustering step, just a linear
  re-binarization pass. This is BQ's one real advantage over IVF.

## Why the deep matrix (250K/500K, 768D/1536D) was stopped here

Both index types already show a clear, negative or neutral signal at
the FIRST two priority-list checkpoints (50K, 100K) — IVF is
performance-neutral-to-worse with a severe recovery-time penalty; BQ
is memory-worse with a real recall cost for negligible latency gain.
Per this phase's own instruction ("the purpose is to establish the
capacity curve, not to complete a spreadsheet" / "do NOT waste hours
running cells that cannot change the product decision"): continuing to
250K-500K would only demonstrate further degradation (IVF's recovery
time alone would likely exceed operationally reasonable bounds well
before 500K, extrapolating from the 47s→130s trend), and testing
768D/1536D would extend the same qualitative finding to higher
dimensions without a compelling reason to expect a different
qualitative outcome. Both are explicitly **NOT MEASURED**, not silently
assumed — see the phase doc's "Remaining unknowns."

## HNSW (carried forward from S9, not re-run)

188 seconds to recover just 10,000 vectors at 384D — the single worst
recovery-time result across every index type tested in S9 or S10. Not
re-tested this phase; no inconsistency to resolve.
