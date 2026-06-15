# Valori benchmark results

Corpus: 10 documents, 23 chunks, 10 entities. Model: all-MiniLM-L6-v2 (384-dim). Top-k = 5.

Three arms, variables isolated as requested in the 2026-06-12 review:

| Arm | Math | Retrieval |
|---|---|---|
| A — baseline (Pinecone-class) | float32 | vector only |
| B — Valori | Q16.16 fixed point | vector only |
| C — Valori hybrid | Q16.16 fixed point | vector + built-in graph |

## 1. Retrieval quality (gold-labeled, Recall@5 / complete-context rate)

| Question set | A: recall | A: complete | B: recall | B: complete | C: recall | C: complete |
|---|---|---|---|---|---|---|
| Single-hop (control, 6 q) | 100% | 100% | 100% | 100% | 100% | 100% |
| Multi-statement (hard, 6 q) | 83% | 67% | 83% | 67% | 100% | 100% |

*complete = ALL gold chunks for the question appear in the top-5 (a multi-statement answer is only as good as its weakest missing chunk).*

### Per-question detail (multi-statement set)

| Question | A | B | C |
|---|---|---|---|
| Why was trade TRD-88231 flagged for review, and what was the final outcome after... | 100% | 100% | 100% |
| Describe the full chain of system failures that led to desk EU-7 positions being... | 100% | 100% | 100% |
| Which policy applied to trades executed during the March incident, and how many ... | 100% | 100% | 100% |
| How did the duplicated fills escape detection by the firm's reconciliation proce... | 50% | 50% | 100% |
| Which client's orders could have been affected by the March order routing incide... | 50% | 50% | 100% |
| What governance changes followed the March incident for the affected desk, and w... | 100% | 100% | 100% |

## 2. Fixed point vs float (variable isolated: A vs B)

- Top-5 agreement between float32 and Q16.16 ranking: **100%**
- Identical recall in section 1 → the deterministic math costs no retrieval quality on this corpus.

## 3. Float non-determinism is real, measured on THIS corpus

- Same query, same document, same float32 values — summed in 4 orders (scalar, reversed, 8-lane AVX-style, 4-lane NEON-style): **276/276** query-document scores (100%) differ at the bit level. Worst spread: 5.36e-07.
- **Concrete ranking flip (Mayur's challenge):** two documents with a true similarity gap of ~1e-9 (below float32 reduction noise at 384 dims). Which one ranks #1 depends only on summation order:

| Reduction order | winner |
|---|---|
| scalar | **d2** |
| reversed | **d1** |
| avx8 | **d1** |
| neon4 | **d1** |

  Constructed adversarial pair — but at 10M+ vectors, near-ties below reduction noise stop being adversarial and start being inventory. On hardware A the user gets document 1; on hardware B, document 2. Both 'correct'. Neither reproducible.

## 4. Valori determinism on the same workload

- Two independent ingests of the corpus → identical BLAKE3 root: **True**
- 5 repeated runs of each query → identical results: **True**
- Q16.16 integer accumulation under all 4 summation orders → identical: **True**
- State root: `3d0500853d6300eff9c82b36ca67510b40591e4451ed873589d171b26bd72a26`
