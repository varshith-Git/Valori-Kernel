# Phase B1.2 - Automatic KG Construction Benchmark (Arm C)

## Goal

Answer, with the exact same 300-document/100-query SciFact slice frozen in Phase B1.1: starting from raw text only (zero access to SciFact's evidence annotations), can an automatically constructed graph recover some of the retrieval improvement that B1.1's oracle graph provides? Add arm C alongside B1.1's frozen A (vector-only) and B (oracle graph) and report `Recovery = (C-A)/(B-A)`.

## Delivered

- `benchmarks/autokg_extract.py` (new) — `SpacyEntityRelationExtractor`, a subclass of `neo4j_graphrag.components.entity_relation_extractor.EntityRelationExtractor` using `spacy`/`en_core_web_sm` noun-chunk entities + a subject-verb-object dependency-parse heuristic for relations (falling back to a generic `CO_OCCURS_WITH` edge when no SVO pattern is found). No LLM, no credential, fully offline and deterministic — chosen over Neo4j KG Builder's shipped `LLMEntityRelationExtractor` specifically to avoid requiring an `OPENAI_API_KEY`/`ANTHROPIC_API_KEY` (neither present in this environment) and real per-run API cost. `assert_no_leakage()` scans all chunk metadata and any extra strings for a blocklist (`qrel`, `evidence`, `rationale`, `support`, `contradict`, `oracle`, `sufficient_label`) and raises before extraction runs.
- `benchmarks/autokg_adapter.py` (new) — `collapse_co_mentions()` (bridges the extracted graph's natural two-hop claim→entity→document shape into a single direct claim↔document edge wherever they share a normalized entity name, keeping GraphRAG's fixed `depth=1` comparable to B1.1's oracle edges), `construction_stats()`, `build_provenance()`, `evaluate_edge_quality()` (precision/recall/F1 of the `co_mentions_entity` edges against SciFact's real evidence — the only edge type with a valid semantic mapping to SciFact's claim/evidence structure), `compute_recovery()`.
- `benchmarks/live_local_db_comparison.py` — new sibling function `valori_http_autokg()` (deliberately duplicates some of the frozen `valori_http()`'s shape rather than refactoring it, since B1.1 is frozen) running the identical 300 documents/100 queries/embeddings through a **fresh** Valori collection with auto-constructed edges instead of oracle edges. New `validate_abc()` extends `validate_ab()` with one additional invariant: the vector-only arm must be bit-identical whether computed alongside the oracle graph or the auto-kg graph, since vector search never consults edges — this caught nothing this run (it passed), but is exactly the kind of cross-collection-contamination bug it exists to catch. `--dbs valori-http-autokg` wired into `main()` (requires `valori-http` in the same invocation, since recovery needs A and B).
- `benchmarks/test_autokg_extract.py`, `benchmarks/test_autokg_adapter.py` (new), plus 2 new tests in `benchmarks/test_metrics_b1_1.py` for `validate_abc` — 21 tests total across the three files.
- `benchmarks/LIVE_LOCAL_RESULTS_B1_2.json` (new) — the frozen A/B/C run. `benchmarks/LIVE_LOCAL_RESULTS.json` and `benchmarks/LIVE_LOCAL_RESULTS_B1_1.json` are untouched.
- `benchmarks/public-data/scifact/auto_kg_graph_b1_2.json` (new) — the full extracted graph (15,104 nodes, 2,936 relationships) + co-mention edges + provenance sidecar, so the exact graph used for evaluation is inspectable without re-running extraction.
- New dependencies (this environment only, `benchmarks/` tooling — nothing in `crates/`): `neo4j-graphrag` 1.19.0 (for its `EntityRelationExtractor`/`Neo4jGraph`/`Neo4jNode`/`Neo4jRelationship`/`TextChunk` types only — never connects to a real Neo4j database), `spacy` 3.8.16 + `en_core_web_sm` 3.8.0.

## Results — frozen A/B/C

Same 300 documents, 100 queries, `sentence-transformers/all-MiniLM-L6-v2` (384-dim), Valori HTTP path, brute-force index, `retrieval_k=10, final_k=20, depth=1, graph_weight=0.3` as B1.1.

| Metric | A — Vector-only | B — Oracle Graph | C — Auto-KG |
|---|---:|---:|---:|
| Recall@3 | 0.8885 | 0.8985 | 0.8885 |
| Recall@5 | 0.9130 | 0.9360 | 0.9260 |
| Recall@10 | 0.9250 | 0.9500 | 0.9400 |
| nDCG@3 | 0.8646 | 0.8637 | 0.8587 |
| nDCG@5 | 0.8719 | 0.8756 | 0.8703 |
| nDCG@10 | 0.8767 | 0.8812 | 0.8759 |
| MRR@10 | 0.8633 | 0.8579 | 0.8546 |
| CompleteContext@10 | 0.9200 | 0.9500 | 0.9400 |
| Latency mean (ms) | 6.0997 | 6.9035 | 9.9150 |
| Latency p50 (ms) | 5.9577 | 6.8031 | 9.5811 |
| Latency p95 (ms) | 7.0965 | 8.1401 | 11.2395 |
| Latency p99 (ms) | 7.5847 | 8.7961 | 14.2237 |

A's numbers here differ slightly from A's B1.1-recorded run (e.g. Recall@10 0.9250 both times, but latency and some earlier-recorded values shift run-to-run due to normal HTTP timing jitter — the retrieval-quality numbers themselves are reproducible). Within *this* run, `validate_abc()`'s cross-collection invariant held exactly: `valori_http_vector` and `valori_http_vector_from_autokg_run` matched bit-for-bit on every retrieval-quality field, confirming the auto-kg collection's vectors never diverged from the oracle collection's.

### Recovery — `(C-A)/(B-A)`

| Metric | auto_gain (C-A) | oracle_gain (B-A) | Recovery |
|---|---:|---:|---:|
| Recall@3 | 0.0000 | 0.0100 | 0.0% |
| Recall@5 | 0.0130 | 0.0230 | 56.5% |
| **Recall@10** | **0.0150** | **0.0250** | **60.0%** |
| nDCG@3 | -0.0059 | -0.0009 | N/A (oracle_gain ≤ 0) |
| nDCG@5 | -0.0016 | 0.0037 | -43.2% |
| nDCG@10 | -0.0008 | 0.0045 | -17.8% |
| MRR@10 | -0.0087 | -0.0054 | N/A (oracle_gain ≤ 0) |
| CompleteContext@10 | 0.0200 | 0.0300 | 66.7% |

Not computed for latency, per design. `B` (oracle) is reported here as an oracle reference, not a theoretical ceiling — nDCG and MRR@10 both show B itself scoring *below* A, which is why recovery is `N/A` for those metrics (a negative denominator makes the ratio meaningless, not just uninteresting).

### Graph-quality evaluation (co_mentions_entity edges vs. SciFact evidence, opened only after C's graph was frozen)

- Edge precision: **0.1491**, edge recall: **0.1917**, edge F1: **0.1677** (65 true positives out of 436 predicted co-mention edges, against 339 real gold claim→evidence pairs).
- Construction stats: 15,104 nodes / 2,936 relationships extracted from 400 texts (300 docs + 100 claims) in 12.50s; 12,326 unique entities after exact-string dedup (2,778 duplicate mentions collapsed); 9,741 isolated nodes (entities with no intra-chunk relation); 436 `co_mentions_entity` bridge edges built from shared entities (vs. B1.1's 72 oracle edges — auto-kg is ~6x noisier in edge count). `model_api_usage: "spacy/en_core_web_sm (local, offline)"`, `estimated_cost_usd: 0`.
- No edge-F1 was computed for entity-to-entity relations (`REDUCE`, `INDUCE`, `HAVE`, etc., dominated by the generic `CO_OCCURS_WITH` fallback at 1,939 of 2,936 edges) — per the design, SciFact does not label entity-level relations, so there is no valid ground truth to compare them against.

## Findings

- **The co-mention collapse design decision worked as intended**: arm C scored meaningfully above arm A on recall/complete-context (it could not have if the two-hop claim→entity→document shape had been left un-collapsed under the fixed `depth=1` GraphRAG parameter — that would have structurally forced C ≈ A regardless of extraction quality).
- **A fully offline, zero-cost, non-LLM extractor recovered 56-67% of the oracle graph's Recall@5/@10/CompleteContext@10 improvement** — a real, usable signal that automatic graph construction captures a substantial fraction of the value a hand-labeled graph provides, without calling any external API.
- **It is not uniformly positive**: nDCG@3/@5/@10 and MRR@10 all show auto-kg performing *worse* than vector-only (negative `auto_gain`), and for nDCG@3/MRR@10 the oracle graph itself also regressed vs. vector-only, making `recovery` undefined (`N/A`) rather than a misleading ratio. This echoes the two findings carried forward from B1.1: even a graph edge can introduce less-relevant candidates near the very top of the ranking, and the effect compounds when the edges are noisier (auto-kg's 436 edges vs. oracle's 72).
- **Edge quality is low in absolute terms** (F1 = 0.1677) despite recall improving — the co-mention rule is deliberately permissive (any shared noun-phrase bridges a claim to a document), so most of its edges are not genuine evidence links, yet the correct ~15-19% still moves retrieval quality measurably. This is the central engineering signal: precision matters for trust/inspectability even when recall-side retrieval numbers look fine.
- `en_core_web_sm`'s generic parser occasionally mis-parses short scientific sentences (e.g. "Metformin treats X" was parsed as a single noun chunk in ad-hoc testing, not subject+verb) — a known, documented limitation of the lightweight heuristic, not a bug; a specialized biomedical NER/relation model would be the natural follow-up if this line of investigation continues.
- Latency grows with graph size as expected: auto-kg's `graphrag_ms`-equivalent p99 (14.22ms) is roughly 1.6x oracle's (8.80ms) and 1.9x vector-only's (7.58ms), consistent with GraphRAG traversing ~6x more edges (436 vs. 72).

## Validation

- `python3 -m pytest benchmarks/test_metrics_b1_1.py benchmarks/test_autokg_extract.py benchmarks/test_autokg_adapter.py -v` — 21/21 passed.
- `python3 -m py_compile benchmarks/live_local_db_comparison.py benchmarks/autokg_extract.py benchmarks/autokg_adapter.py` — clean.
- Live run: `cargo build -p valori-node` (0.3.1, no Rust changes) → node on `127.0.0.1:3411` → `python3 benchmarks/live_local_db_comparison.py --dbs valori-http valori-http-autokg --docs 300 --queries 100 --k 10 --valori-url http://127.0.0.1:3411 --out benchmarks/LIVE_LOCAL_RESULTS_B1_2.json` exited 0; `validate_abc()` did not raise, including its cross-collection vector-consistency check.
- Confirmed `benchmarks/LIVE_LOCAL_RESULTS.json` and `benchmarks/LIVE_LOCAL_RESULTS_B1_1.json` unchanged (`git status --porcelain` empty for both).
- `cargo test -p valori-kernel -p valori-node`: 645 passed, 0 failed, exit 0 (no Rust source changed this phase).

## Follow-ups

- B1.2's numbers are now frozen — no tuning of the extraction heuristic, the co-mention collapse rule, or GraphRAG parameters based on this result.
- The low edge-F1 (0.1677) suggests the next real lever is extraction *precision*, not recall — e.g. restricting the co-mention bridge to entities that are the subject/object of an SVO relation (rather than any shared noun chunk) would likely cut false-positive edges at some recall cost; worth its own controlled follow-up phase rather than adjusting B1.2 in place.
- A specialized biomedical NER/relation model (in place of `en_core_web_sm`) is a plausible next experiment if the non-LLM direction continues — SciFact's domain vocabulary (genes, diseases, treatments) is a poor match for a general-purpose small English model.
- The originally-scoped LLM-based path (`neo4j-graphrag`'s own `LLMEntityRelationExtractor`) was not run — it remains available as a future arm D if an `OPENAI_API_KEY`/`ANTHROPIC_API_KEY` and approved API spend become available, for a direct LLM-vs-non-LLM extraction comparison.
- B1.1's own follow-up (root-causing its Recall@3 drift vs. the original B1 recording) remains deliberately not investigated, per B1.1's phase doc.
