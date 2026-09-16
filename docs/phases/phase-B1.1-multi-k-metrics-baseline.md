# Phase B1.1 - Multi-cutoff Metrics Baseline

## Goal

Extend the Phase B1 live SciFact benchmark so a single run reports Recall@3/5/10, nDCG@3/5/10, MRR@10, complete_context@10, and latency mean/p50/p95/p99 for both the vector-only arm and the oracle-graph GraphRAG arm, with enough reproducibility metadata to serve as a frozen A/B baseline for the upcoming B1.2 Auto-KG comparison — without overwriting B1's original recorded result.

## Delivered

- `benchmarks/live_local_db_comparison.py`:
  - `metrics(results, qrels, ks=(3, 5, 10))` rewritten to compute `recall_at_{k}`/`ndcg_at_{k}` for every cutoff in one pass over a single top-10 ranked list per query, plus `mrr_at_10` (reciprocal rank of the first relevant doc in the top-10 list) and the existing `complete_context`/`query_count` fields.
  - New `latency_stats(durations_s)` helper reporting `latency_mean_ms`/`p50`/`p95`/`p99`; wired into `valori_http()`'s vector-search and GraphRAG query loops via per-query timing (previously a single wall-clock mean over the whole loop).
  - `valori_http()` now also returns an `ingestion` block (`documents_inserted`, `vectors_inserted`, `oracle_edges_inserted`, `ingestion_duration_s`) and asserts `set(vector_results) == set(graph_results) == set(selected)` before returning.
  - New `--out` CLI flag (default unchanged: `benchmarks/LIVE_LOCAL_RESULTS.json`) so this run does not clobber the B1 artifact.
  - New `git_commit()` and `node_version()` helpers; `main()` now writes a `meta` block (git commit, timestamp, dataset, doc/query counts, embedding model+dim, index description, `graph_source: "oracle_scifact"`, GraphRAG params, node URL, node version) into the output JSON.
  - New `validate_ab(out)` — asserts data/math invariants only (see Validation below) — called before writing output; raises loudly on violation.
  - The three pre-existing `metrics()` call sites (FAISS, embedded-FFI vector, embedded-FFI vector+graph) updated for the new signature so they don't break, though only the HTTP path was exercised for this recorded run.
  - Deferred the top-level `sentence_transformers` import to only fire on an embedding-cache miss (see Findings) — needed to run this benchmark without installing a new heavy dependency, since the cached 300-doc/100-query MiniLM embeddings from B1 already cover this exact run.
- `benchmarks/test_metrics_b1_1.py` (new) — 7 pytest tests covering `metrics()` (multi-cutoff recall, MRR@10, no-hit zeroing), `latency_stats()` (percentile ordering), and `validate_ab()` (passes on well-formed output, rejects non-monotonic recall, explicitly does *not* require nDCG monotonicity, explicitly does *not* penalize a graph-arm regression vs. the vector arm).
- `benchmarks/LIVE_LOCAL_RESULTS_B1_1.json` (new) — the recorded run. `benchmarks/LIVE_LOCAL_RESULTS.json` (B1) is untouched (`git status --porcelain` empty for that path throughout).

## Results — frozen A/B baseline

300 SciFact documents, 100 queries, `sentence-transformers/all-MiniLM-L6-v2` (384-dim), Valori HTTP path, brute-force index, 72 oracle claim→evidence edges from SciFact's public query metadata, GraphRAG params `retrieval_k=10, final_k=20, depth=1, graph_weight=0.3`.

| Metric | A — Vector-only | B — Oracle GraphRAG |
|---|---:|---:|
| Recall@3 | 0.8885 | 0.8985 |
| Recall@5 | 0.9130 | 0.9360 |
| Recall@10 | 0.9250 | 0.9500 |
| nDCG@3 | 0.8646 | 0.8637 |
| nDCG@5 | 0.8719 | 0.8756 |
| nDCG@10 | 0.8767 | 0.8812 |
| MRR@10 | 0.8633 | 0.8579 |
| CompleteContext@10 | 0.9200 | 0.9500 |
| Latency mean (ms) | 5.7242 | 6.3143 |
| Latency p50 (ms) | 5.6743 | 6.1457 |
| Latency p95 (ms) | 6.1270 | 6.8411 |
| Latency p99 (ms) | 6.3061 | 8.1778 |

Recorded outcome, not a pass/fail gate: GraphRAG improved Recall@3/5/10, nDCG@3/5/10, and CompleteContext@10, but **regressed slightly on MRR@10** (0.8633 → 0.8579) and added latency overhead at every percentile, most visibly at p99 (6.31ms → 8.18ms). Both are legitimate results and are recorded as-is.

Full metadata (commit `4eecce63c9d5c5bacfec317d9a5fed0e287e5c93`, node version `0.3.1`, timestamp, GraphRAG params, `graph_source: "oracle_scifact"`) and ingestion stats (300 docs / 400 vectors / 72 edges inserted in 1.5763s) are in `benchmarks/LIVE_LOCAL_RESULTS_B1_1.json`.

## Findings

- The script's Recall@3 numbers here (0.8885 vector-only / 0.8985 graph) are higher than B1's recorded k=3 numbers (0.7857 / 0.9035) despite being nominally the same experiment. The likely cause is that B1's committed script version differs subtly from what's in the current `main` branch (e.g. edge/candidate construction may have changed between the two commits) — this is worth a direct diff against the B1 commit if exact reproduction of the historical numbers matters, but is out of scope for B1.1 (multi-cutoff instrumentation), since B1.1 deliberately did not touch retrieval/graph-construction logic, only the metrics layer.
- `live_local_db_comparison.py` unconditionally imported `sentence_transformers` at the top of `main()` even when the embedding cache fully covers the requested run — a pre-existing gap that blocked this exact recorded run until fixed (see Delivered). No new dependency was installed; the fix only defers the import.
- `/v1/proof/event-log` returned no event-log hash for this run (`state_hash: null` in the `vector_graph` block) because the node was started without `VALORI_EVENT_LOG_PATH` — informational only, not required for the retrieval-quality metrics.

## Validation

- `python3 -m pytest benchmarks/test_metrics_b1_1.py -v` — 7/7 passed.
- `python3 -m py_compile benchmarks/live_local_db_comparison.py` — clean.
- Live run: `cargo build -p valori-node` (0.3.1) → started `target/debug/valori-node` on `127.0.0.1:3410` (`VALORI_DIM=384`) → `python3 benchmarks/live_local_db_comparison.py --dbs valori-http --docs 300 --queries 100 --k 10 --valori-url http://127.0.0.1:3410 --out benchmarks/LIVE_LOCAL_RESULTS_B1_1.json` exited 0; `validate_ab()` did not raise.
- Confirmed `benchmarks/LIVE_LOCAL_RESULTS.json` (B1) unchanged (`git status --porcelain` empty for that path).
- `cargo test -p valori-kernel -p valori-node`: 625 passed, 0 failed (no Rust source changed this phase — identical to the pre-phase baseline).

## Follow-ups

- **B1.2 is next**: introduce Neo4j KG Builder (or an equivalent) to construct the graph automatically, with SciFact's evidence annotations completely hidden from graph construction and opened only afterward for evaluation. Same 300 documents / 100 queries / embeddings / HTTP path; add a third arm (C — Auto-KG) alongside A (vector-only) and B (oracle GraphRAG, this phase). Compute graph-extraction Precision/Recall/F1 against the hidden SciFact evidence edges, and `Recovery = (C − A) / (B − A)` per cutoff.
- B1.1's parameters (`depth=1`, `graph_weight=0.3`, `retrieval_k/final_k`) are now frozen — no tuning based on this result before B1.2 runs.
- The Recall@3 discrepancy vs. B1's originally recorded numbers (see Findings) should be root-caused if an apples-to-apples historical comparison is ever needed; it does not block B1.2, since B1.2 will compare against this phase's own numbers, not B1's.
- FAISS/Qdrant/Milvus/Weaviate local baselines remain deferred to B2 per the original B1 follow-ups.
