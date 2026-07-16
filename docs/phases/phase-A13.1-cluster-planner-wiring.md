# Phase A13.1 — Cluster Planner Wiring: Route Cluster Handlers Through ExecutionGraph

## Goal

Wire the 7 cluster-path handlers (graphrag, snapshot_save, tree_build, tree_query, tree_hybrid, community_detect, community_search) through `run_graph_inline` so the cluster path mirrors the standalone path end-to-end — both paths now follow the unified `HTTP → ExecutionGraph → TaskRunner → RaftKernelCapability → Kernel → Response` flow.

## Delivered

**`crates/valori-node/src/capabilities.rs`**
- Expanded `RaftKernelCapability` struct with 4 new fields: `tree_cache`, `community_store`, `embed_config`, `http`.
- Implemented 8 new methods on `RaftKernelCapability`:
  - `save_snapshot(shard_id, _path)` — clones shard state, hashes with BLAKE3, returns hex hash.
  - `graph_rag(shard_id, ns_id, vector, k, depth)` — two-pass: sync `with_state()` for kNN + subgraph, async `get_meta_json()` loop for metadata enrichment.
  - `memory_search(...)` — two-pass: sync `with_state_and_timestamps()` for search + timestamps, async `get_meta_json()` loop with decay/filter.
  - `community_detect(shard_id, ns_id, max_iter)` — `with_state()` for label propagation + store build, writes into `self.community_store`.
  - `community_search(...)` — reads `self.community_store`, calls `rank_communities()`.
  - `tree_build(text, doc_name)` — `TreeIndex::from_markdown()`, caches in `self.tree_cache`.
  - `tree_query(tree_json, query, k, prev_hash)` — resolves tree (string = cache key lookup or full JSON), calls `tree.answer()`.
  - `tree_hybrid(shard_id, ns_id, query, k, params)` — resolves tree, ranks, optional vector scan via `with_state()`, fuses scores, returns `HybridResponse` JSON.
- Updated `RaftKernelCapability::new()` to accept all 7 params.
- Updated `CapabilityRegistryBuilder::build_cluster()` to accept `tree_cache` and `community_store`.

**`crates/valori-node/src/cluster_server.rs`** — 7 handlers replaced:
- `cluster_graphrag` — consistency + dim checks preserved; computation through `TaskKind::GraphRag`.
- `cluster_snapshot_save` — through `TaskKind::SnapshotArtifact`; returns `hash` from capability.
- `cluster_tree_build` — through `TaskKind::TreeBuild`; reconstructs `BuildResponse` from task JSON.
- `cluster_tree_query` — tree resolution (inline or cache) preserved; through `TaskKind::TreeQuery`.
- `cluster_tree_hybrid` — tree + embed resolution preserved; through `TaskKind::TreeHybrid`.
- `cluster_community_detect` — S8 namespace→shard routing preserved; through `TaskKind::CommunityDetect`.
- `cluster_community_search` — through `TaskKind::CommunitySearch`.

**`crates/valori-node/src/tree_rag.rs`**
- Added `Deserialize` to `HybridHit` and `HybridResponse` (required for task output round-trip).

**`crates/valori-node/src/community.rs`**
- Added `Deserialize` to `CommunitySummary`, `DetectResponse`, `CommunityHit`, `SearchResponse`.

**`crates/valori-node/src/api.rs`**
- Added `Deserialize` to `MemorySearchHit` and `MemorySearchResponse`.

## Findings

- Two-pass pattern is required for cluster reads: sync `with_state()` closure for kernel state access, then a separate async loop for `get_meta_json()` (which is async). Mixing async inside `with_state()` is not possible.
- `cluster_memory_search` was intentionally excluded — it already routes through `routes::memory::memory_search()` via the `MemoryOps` trait, which is the correct shared-handler pattern.
- `KernelState.meta` is a `BTreeMap<String, String>`; metadata access is via `shard_sm.get_meta_json("rec:{rid}").await`, not a field access on `KernelState`.

## Validation

```
cargo build -p valori-node  → 0 errors, 4 pre-existing warnings
cargo test -p valori-node   → 43 passed, 0 failed
cargo test -p valori-kernel → 138 passed, 0 failed
tests/route_parity.rs       → 2 passed
```

## Follow-ups

- **A14 — Receipt assembly**: Wire `ReceiptAssembler` so `run_graph_inline` produces a BLAKE3 receipt chain across both paths.
- **A15 — Planner cache integration**: Wire `plan_with_cache()` so graph construction uses the two-layer cache (in-process + MetadataDb) rather than building a new graph per request.
