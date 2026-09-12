# Phase B1 - Live Retrieval Quality Benchmark

## Goal

Prove, with the same public data and same embeddings, whether Valori's vector+graph retrieval can return better context than pure vector retrieval. Hosted/API-only databases were excluded; Pinecone was therefore not run.

## Delivered

- `benchmarks/live_local_db_comparison.py` - added a repeatable BEIR SciFact benchmark that downloads the public dataset, caches MiniLM embeddings, builds same-data FAISS and Valori runs, and supports a server-backed Valori GraphRAG arm.
- `benchmarks/LIVE_LOCAL_RESULTS.json` - recorded the live run: 300 SciFact papers, 100 claim queries, 384 dimensions, 72 public claim-to-evidence graph edges.

## Findings

- Same vectors produce identical pure-vector retrieval quality for FAISS and Valori, which confirms the harness is comparing the retrieval layer rather than changing embeddings.
- At `k=3`, Valori HTTP GraphRAG improved Recall from `0.7857` to `0.9035`, NDCG from `0.7745` to `0.8735`, and complete-context rate from `0.76` to `0.89`.
- The embedded Python FFI graph path did not expose created edges through `get_edges()`/`walk()`/`expand()` during smoke testing, even though the HTTP graph path did. This is a real follow-up bug/gap, not hidden by the benchmark.
- Full 5,183-document HTTP insertion was too slow for an interactive run because every record is committed through the audited node path. The script keeps full-corpus support, but the recorded run uses an evidence-preserving `--docs 300` slice.

## Validation

- Ran `python benchmarks/live_local_db_comparison.py --dbs valori faiss --queries 100` as the embedded baseline; vector-only FAISS and Valori both scored Recall `0.272`, NDCG `0.2332`, complete context `0.2667` on the full 5,183-document corpus before metric scoping was fixed.
- Smoke-tested `target/debug/valori-node.exe` on `127.0.0.1:3307`: inserted two vectors, created a graph edge, verified `/v1/graph/edges/:id` returned it, and verified `/v1/graphrag` returned a graph-only hit at distance 1.
- Ran `python benchmarks/live_local_db_comparison.py --dbs faiss valori-http --queries 100 --docs 300 --k 3 --valori-url http://127.0.0.1:3307`.
- Result: FAISS and Valori vector-only matched exactly; Valori vector+graph improved retrieval quality with event-log proof state hash `9a21caf3162921ee998a225714c9f0bf96a6e0b60c2be1a2efd51834175c9a7a`.
- Ran `python -m py_compile benchmarks/live_local_db_comparison.py`.
- `cargo test -p valori-kernel -p valori-node` was not run because the workspace already contains unrelated dirty changes and an unresolved conflict outside this benchmark phase.

## Follow-ups

- B2 should add Qdrant, Milvus, and Weaviate local adapters as pure-vector baselines using the same cached SciFact embeddings.
- B3 should investigate and fix the embedded Python FFI graph traversal gap.
- B4 should add a batch HTTP ingest path or benchmark mode so the full 5,183-document server-backed GraphRAG run is practical.
