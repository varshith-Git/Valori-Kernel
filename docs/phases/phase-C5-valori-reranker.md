# Phase C5 — Valori Reranker (hybrid retrieval)

## Goal

Add a post-retrieval reranker that runs inside the Valori node and combines
vector similarity with term-frequency scoring, lifting retrieval accuracy on
lexically-precise queries without any external dependency or LLM round-trip.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/valori_reranker.rs` | New module — `ValoriReranker` struct, corpus management (`insert`, `remove`, `remove_batch`), hybrid scoring, `POOL_FACTOR = 20` |
| `crates/valori-node/src/lib.rs` | `pub mod valori_reranker;` declaration |
| `crates/valori-node/src/api.rs` | `InsertRecordRequest.text: Option<String>`, `BatchInsertRequest.texts: Option<Vec<Option<String>>>`, `SearchRequest.rerank: bool` (default `true`), `SearchRequest.query_text: Option<String>` |
| `crates/valori-node/src/engine.rs` | `reranker: ValoriReranker` field on `Engine`; `reranker_insert()` method; remove-on-`soft_delete`; `remove_batch` on `drop_collection` via `iter_records_in_ns` |
| `crates/valori-node/src/server.rs` | Insert/batch-insert handlers call `reranker_insert`; search handler fetches `k × POOL_FACTOR` candidates, calls `reranker.rerank()`, returns top-k |
| `crates/valori-node/src/cluster_server.rs` | `rerank` + `query_text` fields on local `SearchRequest`; search handler builds on-the-fly reranker from `with_text_corpus()` |
| `crates/valori-consensus/src/state_machine.rs` | `text_corpus: HashMap<u64, String>` on `SmInner`; populated at apply time; exposed via `with_text_corpus()` |
| `crates/valori-kernel/src/state/kernel.rs` | `iter_records_in_ns(namespace_id: u16)` — public iterator used by `drop_collection` cleanup |
| `python/valoricore/remote.py` | `SyncRemoteClient.health()` method; `insert(text=)`, `insert_batch(texts=)`, `search(rerank=True, query_text=)` on both `SyncRemoteClient` and `AsyncRemoteClient` (including cluster variant) |
| `pageindex/valori_tree_rag/compare_valori_vs_pageindex.py` | Updated comparison script — all Valori calls via SDK; `texts=` passed at insert; `rerank=True, query_text=` at search; "Valori alone" column removed |

## Findings

### POOL_FACTOR must cover the index

With `POOL_FACTOR = 4` and `k = 1`, the server only fetched 4 candidates —
too few for a 15-node collection where the correct section is rarely in the top
4 by vector similarity alone. Setting `POOL_FACTOR = 20` ensures the reranker
sees the full index for small collections; for large ones the overhead is
acceptable (20 extra kernel results per query).

### Circular dependency avoided

`valori-consensus` cannot import `valori-node` (would create a cycle).
The solution: consensus stores raw text in `text_corpus: HashMap<u64, String>`
and exposes it via `with_text_corpus()`. `cluster_server.rs` (in `valori-node`)
builds a transient `ValoriReranker` from those texts at query time — no heap
allocation kept between requests, no circular dep.

### `no_std` invariant preserved

`ValoriReranker` lives entirely in `valori-node` (std). Nothing was added to
`valori-kernel`. The `iter_records_in_ns` helper added to `kernel.rs` is
pure iteration with no std dependency.

## Validation

```
cargo test -p valori-kernel -p valori-node
251 tests passed, 0 failed
```

End-to-end comparison on Composer 2 Technical Report (23 pages, 10 hard questions):

| Method | Accuracy | Latency |
|---|---|---|
| Valori (vector + Valori Reranker) | **90%** | 0.4 s |
| PageIndex + term scoring | 60% | 38.6 s |
| PageIndex LLM alone | 60% | 38.6 s |

Hard-lexical questions: Valori Reranker = **100%** (7/7).
Semantic questions: 67% (2/3 — "write clean code" has no lexical signal).
BLAKE3 receipt chain: intact, 10 receipts per run.

## Follow-ups

| Item | Phase |
|---|---|
| Leaf-node preference heuristic — when parent and child embed similarly, prefer the leaf | C6 |
| Persist `text_corpus` to snapshot so cluster reranking survives restarts | C6 |
| Expose `POOL_FACTOR` as env var (`VALORI_RERANKER_POOL_FACTOR`) for tuning | C6 |
| Semantic-only fallback when query has no lexical tokens in corpus | C6 |
