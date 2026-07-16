# Phase N3 — valori-rag extraction

## Goal

Extract `graph_rag.rs`, `tree_rag.rs`, and `community.rs` from `valori-node` into a standalone `valori-rag` crate. Break the circular dependency between entity extraction and `EmbedConfig` by introducing a minimal `LlmConfig` struct inside the new crate. SOLID principles throughout: one file per modality (SRP), narrow public surface (ISP), depend on `KernelState` not `Engine` (DIP).

## Delivered

| File | Change |
|------|--------|
| `crates/valori-rag/Cargo.toml` | New crate — deps: `valori-kernel`, `serde`, `serde_json`, `axum`, `blake3`, `reqwest`, `tokio`, `tracing` |
| `crates/valori-rag/src/lib.rs` | Crate root — module declarations + flat re-exports |
| `crates/valori-rag/src/graph.rs` | `resolve_seed_nodes`, `expand_subgraph`, `MAX_DEPTH` — moved from `graph_rag.rs` |
| `crates/valori-rag/src/tree.rs` | `TreeIndex`, `TreeNode`, `Receipt`, `verify_chain`, stateless handlers `tree_verify`/`tree_chain_verify` — moved from `tree_rag.rs` |
| `crates/valori-rag/src/community.rs` | `label_propagation`, `build_community_store`, `rank_communities` + all request/response types — moved from `community.rs` |
| `crates/valori-rag/src/llm.rs` | `LlmConfig` + `extract_entities_via_llm` — extracted from `community.rs`, decoupled from `EmbedConfig` |
| `crates/valori-rag/README.md` | Crate README with module table, usage examples, design invariants, scalability table |
| `Cargo.toml` | Added `valori-rag` to workspace members + `workspace.dependencies` |
| `crates/valori-node/Cargo.toml` | Added `valori-rag` dep |
| `crates/valori-node/src/lib.rs` | Removed `pub mod graph_rag;`, `pub mod tree_rag;`, `pub mod community;` |
| `crates/valori-node/src/server.rs` | All `crate::graph_rag::*` / `crate::tree_rag::*` / `crate::community::*` → `valori_rag::graph::*` / `valori_rag::tree::*` / `valori_rag::community::*`; extract-entities call site constructs `LlmConfig` |
| `crates/valori-node/src/cluster_server.rs` | Same as server.rs |
| `crates/valori-node/src/capabilities.rs` | All `crate::tree_rag::*` → `valori_rag::tree::*` |
| `crates/valori-node/src/engine.rs` | `tree_cache` field type + `cache_tree`/`get_cached_tree` signatures → `valori_rag::tree::TreeIndex` |
| `crates/valori-node/src/graph_rag.rs` | **Deleted** |
| `crates/valori-node/src/tree_rag.rs` | **Deleted** |
| `crates/valori-node/src/community.rs` | **Deleted** |

## Findings

**`EmbedConfig` coupling:** `community.rs::extract_entities_via_llm` took `&crate::embedder::EmbedConfig` — a direct reference into `valori-node`'s own types. Moving this function naively would have required `valori-rag` to depend on `valori-node` (circular). Fixed by defining `LlmConfig { provider, model, url, api_key }` in `valori_rag::llm`; the node constructs it at the call site with 4 field assignments.

**`KernelState::new()` signature:** Integration tests in `valori-index` called `KernelState::new(4, 16, 32)` — that was the old pre-refactor signature. The actual API is `KernelState::new()` (no arguments). Fixed in all new `valori-rag` tests on first compile run.

**Stateless handlers stay in the RAG crate:** `tree_verify` and `tree_chain_verify` use only `axum::Json` — no `State<>` parameter — so they compile into both `server.rs` and `cluster_server.rs` unchanged. This is the correct pattern per the CLAUDE.md shared-handler guidance.

**`capabilities.rs` and `engine.rs` also referenced `tree_rag`:** The bulk `sed` on `server.rs` and `cluster_server.rs` was not sufficient — two more files had `crate::tree_rag` references that needed updating.

## Validation

```
cargo test -p valori-rag   → 13 passed, 0 failed
cargo test -p valori-node  → all passed (0 failures across all test suites)
cargo build -p valori-node → 0 errors, 1 dead-code warning (pre-existing)
```

Test breakdown for `valori-rag`:
- `graph::tests` — 2 tests (empty_seeds, resolve_seeds_empty)
- `tree::tests` — 8 tests (hierarchy, breadcrumb, navigation, citation, tampering, chain, json roundtrip, determinism)
- `community::tests` — 3 tests (empty_graph LP, empty_store rank, receipt len)

## Follow-ups

- Phase N4: extract `valori-ingest` — `ingest.rs` + `embedder.rs` → new crate; this removes the last large chunk from `valori-node` beyond the server scaffolding itself
- Phase N5: extract `valori-engine` — `engine.rs` + `commit/` (highest risk; do last)
- `expand_subgraph` currently emits only outgoing edges; incoming edges are not traversed (by design — follows existing `/graph/subgraph` behaviour). A future phase could expose a bidirectional option.
