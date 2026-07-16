# Phase N1 — valori-search extraction

## Goal

Extract the three post-retrieval search primitives from `valori-node` into a
standalone `valori-search` crate with no kernel or node dependency. This is the
first step of the node decomposition plan: zero runtime cost, immediate
incremental build benefit.

## Delivered

| File | Change |
|------|--------|
| `crates/valori-search/Cargo.toml` | New crate — deps: `serde`, `serde_json` only |
| `crates/valori-search/src/lib.rs` | Crate root — re-exports, design invariant docs |
| `crates/valori-search/src/decay.rs` | Moved from `valori-node/src/decay.rs` |
| `crates/valori-search/src/reranker.rs` | Moved from `valori-node/src/valori_reranker.rs`; inverted-index scalability fix |
| `crates/valori-search/src/filter.rs` | Extracted from `valori-node/src/api.rs::matches_metadata_filter` |
| `crates/valori-search/README.md` | Crate README with usage examples and complexity table |
| `Cargo.toml` | Added `valori-search` to workspace members + `workspace.dependencies` |
| `crates/valori-node/Cargo.toml` | Added `valori-search` dep |
| `crates/valori-node/src/lib.rs` | Removed `decay` and `valori_reranker` mod declarations |
| `crates/valori-node/src/api.rs` | Removed `matches_metadata_filter` + `value_matches`; re-exports from `valori_search` |
| `crates/valori-node/src/engine.rs` | Updated `corpus_len()` → `len()` (redundant alias removed) |
| `crates/valori-node/src/server.rs` | All `crate::decay::*`, `crate::valori_reranker::*`, `crate::api::matches_metadata_filter` → `valori_search::*` |
| `crates/valori-node/src/cluster_server.rs` | Same substitution |
| `crates/valori-node/src/capabilities.rs` | Same substitution |
| `crates/valori-node/src/decay.rs` | **Deleted** (moved) |
| `crates/valori-node/src/valori_reranker.rs` | **Deleted** (moved) |

## Findings

**Scalability bug fixed:** The original `ValoriReranker::rerank` computed IDF by
scanning the entire corpus for each query term — O(|corpus| × |query_terms|).
At 100k documents with a 5-term query this is 500k HashMap iterations per
search. The new implementation maintains `doc_freq: HashMap<String, usize>`
updated incrementally on every `insert` and `remove`. IDF lookup is now O(1).

**`restore_corpus` was unsound:** The old implementation accepted a
`total_tokens` argument from the snapshot and used it as-is. If the tokeniser
changed between snapshot and restore, the avgdl would be wrong. The new
implementation always recomputes from the restored corpus and rebuilds
`doc_freq` from scratch, making restore deterministic and version-safe.

**`corpus_len()` was a dead alias:** Removed; `len()` serves the same purpose.

## Validation

```
cargo test -p valori-search   → 27 passed, 0 failed
cargo test -p valori-node     → all passed (including route_parity)
cargo build -p valori-node    → 0 errors, 3 pre-existing warnings (unchanged)
```

## Follow-ups

- Phase N2: extract `valori-index` — `structure/{hnsw,ivf,bq,index}` → new crate
- Phase N3: extract `valori-ingest` — `ingest.rs` + `embedder.rs` → new crate
- Phase N4: extract `valori-rag` — `graph_rag.rs` + `tree_rag.rs` + `community.rs`
- Phase N5: extract `valori-engine` — `engine.rs` + `commit/` (highest risk, do last)
