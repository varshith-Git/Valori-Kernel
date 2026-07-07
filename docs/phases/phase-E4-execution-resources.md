# Phase E4 — ExecutionResources

## Goal

Extract application-layer caches (`tree_cache`, `community_store`) from `Engine`
into a named struct so the boundary between database and product-feature code
is visible in the type system.

## Delivered

| File | What |
|---|---|
| `crates/valori-node/src/engine.rs` | New `pub struct ExecutionResources { tree_cache, community_store }` with `new()`. Engine's two standalone fields replaced with `pub resources: ExecutionResources`. Constructor updated. `cache_tree` / `get_cached_tree` now access `self.resources.tree_cache`. |
| `crates/valori-node/src/server.rs` | `eng.community_store` → `eng.resources.community_store` (4 sites). |

## Findings

1. `tree_cache` in the standalone path was already fully encapsulated via `cache_tree`/`get_cached_tree` — no external direct field access existed. The grouping is architectural signal, not a bug fix.
2. The cluster path (`cluster_server.rs`) stores `tree_cache` and `community_store` as `Arc<RwLock<>>` on `DataPlaneState` — they are NOT inside Engine there. E4 only affects the standalone path. This asymmetry is intentional (cluster needs async-safe shared state; standalone doesn't).

## Validation

- `cargo check -p valori-node` — clean.
- `cargo test --workspace` — all tests passed, 0 failed.

## Follow-ups

- The `tree_rag.rs`, `community.rs`, `valori_reranker.rs`, `ingest.rs` files (~2,700 lines) are still in `valori-node/src/`. Moving them into a `valori-rag` crate is the next step; E4 makes the extraction cleaner by showing which Engine fields they touch.
