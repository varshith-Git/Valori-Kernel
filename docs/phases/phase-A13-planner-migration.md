# Phase A13 — Planner Migration: Route Operations Through ExecutionGraph

## Goal

Route all complex standalone operations (Snapshot, GraphRAG, MemorySearch, CommunityDetect, CommunitySearch, TreeBuild, TreeQuery, TreeHybrid) through the `valori-planner` ExecutionGraph pipeline, so every endpoint follows the unified `HTTP → Operation → ExecutionGraph → TaskRunner → Capability → Kernel/Effects → Response` path.

## Delivered

**`crates/valori-effect/src/capability.rs`**
- Added 8 default async methods to `KernelCapability` trait: `save_snapshot`, `graph_rag`, `memory_search`, `community_detect`, `community_search`, `tree_build`, `tree_query`, `tree_hybrid`. All default to `Err(EffectError::CapabilityUnavailable)`.

**`crates/valori-node/src/capabilities.rs`**
- Implemented all 8 new methods on `EngineKernelCapability` (standalone path).
- `save_snapshot`: write-locks engine, calls `eng.save_snapshot()`, returns BLAKE3 state hash.
- `graph_rag`: read-locks, calls `eng.search_l2_ns()`, resolves seed nodes, expands subgraph, returns full JSON.
- `memory_search`: read-locks, over-fetches, applies decay + metadata filter + optional reranker, returns hit list.
- `community_detect`: write-locks, runs label propagation, builds community store, caches result.
- `community_search`: read-locks, checks store, calls `rank_communities()`.
- `tree_build`: builds `TreeIndex`, caches by content hash, returns JSON with `cache_key`.
- `tree_query`: deserializes tree, calls `tree.answer()`, returns `AnswerResult`.
- `tree_hybrid`: resolves tree, does tree ranking + optional vector fusion, builds full `HybridResponse`-shaped JSON.

**`crates/valori-effect/src/tasks/`** — 5 new task files:
- `snapshot.rs` — `SnapshotArtifactTask`
- `graph_rag.rs` — `GraphRagTask`
- `memory_search.rs` — `MemorySearchTask`
- `community.rs` — `CommunityDetectTask`, `CommunitySearchTask`
- `tree_rag.rs` — `TreeBuildTask`, `TreeQueryTask`, `TreeHybridTask`

**`crates/valori-planner/src/graph.rs`**
- Added 6 new `TaskKind` variants: `MemorySearch`, `CommunityDetect`, `CommunitySearch`, `TreeBuild`, `TreeQuery`, `TreeHybrid`.

**`crates/valori-node/src/runner.rs`**
- Registered all 8 new task implementations in `TaskRegistry::default_registry()`.
- Updated `kind_to_key()` to map all new variants.

**`crates/valori-node/src/server.rs`**
- Wired 8 standalone handlers through `run_graph_inline`:
  - `snapshot_save`, `graphrag`, `memory_search_vector`, `community_detect`, `community_search`, `tree_build`, `tree_query`, `tree_hybrid`.

**`crates/valori-node/src/tree_rag.rs`**
- Added `Deserialize` to `HybridHit` and `HybridResponse` (needed for task output deserialization).

**`crates/valori-node/src/community.rs`**
- Added `Deserialize` to `CommunitySummary`, `DetectResponse`, `CommunityHit`, `SearchResponse`.

**`crates/valori-node/src/api.rs`**
- Added `Deserialize` to `MemorySearchHit` and `MemorySearchResponse`.

## Findings

- `HybridHit` and `HybridResponse` were `Serialize`-only; adding `Deserialize` is safe since `skip_serializing_if` fields just become optional during deserialization.
- `tree_hybrid` capability previously returned minimal JSON `{"hits", "receipt"}` — upgraded to return the complete `HybridResponse`-shaped JSON including `tree_answer`, `reasoning`, `tree_hit_count`, `vector_hit_count`.
- `TaskOutput.json` (not `.value`) is the correct field name for accessing task result data.
- The cluster path (`cluster_server.rs`) retains inline implementations — `RaftKernelCapability` does not yet implement the 8 new methods. Cluster reads could be wired by implementing state-machine reads in `RaftKernelCapability`; deferred to a follow-on phase.

## Validation

```
cargo test -p valori-node   → 43 passed, 0 failed (unit + integration)
cargo test -p valori-kernel → 138 passed, 0 failed
tests/route_parity.rs       → 2 passed (paths and methods match)
```

## Follow-ups

- **A13.1 — RaftKernelCapability**: Implement the 8 new capability methods for the cluster path so cluster handlers can also be routed through `run_graph_inline`. Requires passing shard state machines into `RaftKernelCapability`.
- **A14 — Receipt assembly**: Wire `ReceiptAssembler` so `run_graph_inline` produces a BLAKE3 receipt chain over the task outputs.
