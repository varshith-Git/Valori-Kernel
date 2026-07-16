# Phase A14 — valori-node audit bug fixes

## Goal

Fix the bugs surfaced during the deep valori-node audit (post-A13.1): P0 correctness
blockers first, then P1 correctness bugs. No behavior regressions; all tests must
continue to pass.

## Delivered

### `crates/valori-node/src/capabilities.rs`

- **`RaftKernelCapability::state_hash()` (P0 — zero hash)**: Was always returning `"0".repeat(64)`. Now uses `tokio::task::block_in_place` + `block_on` to call `ValoriStateMachine::with_state()` async from the sync trait method, computing the real BLAKE3 hash of the shard's `KernelState`. Safe because the `ValoriStateMachine` mutex is never held by the calling task at that point (prior awaits release it), and Valori uses a multi-threaded Tokio runtime.
- **`RaftKernelCapability::memory_search()` — decay sort order (P1)**: Sort was `score * decay_factor` ascending, which shrinks older records' effective distance and ranks them *better* — the opposite of the intent. Fixed to `score / decay_factor` ascending, matching the standalone path's `adjusted = distance / factor` formula in `decay.rs`.

### `crates/valori-node/src/cluster_server.rs`

- **`cluster_snapshot_save` (P0 — shard-0-only + field mismatch)**: Old handler only saved shard 0 and read `o.json["hash"]` (wrong field — `SnapshotArtifactTask` emits `"state_hash"`). New handler loops `0..shard_count`, runs one planner graph per shard, reads `"state_hash"`, and returns `{ "shards": [{ "shard_id": N, "state_hash": "..." }, ...] }`.
- **Auth middleware — `/health` and `/metrics` behind auth (P0)**: `cluster_auth_guard` was applied to the merged router including `/health` and `/metrics`. Restructured: `public` sub-router (health, metrics) is merged without auth; `protected` sub-router (all v1 routes) gets `cluster_auth_guard` via `.layer()` before merge.
- **`cluster_community_search` shard routing (P1)**: Was hardcoded to `shard_id: 0` and `namespace_id: 0`. Now resolves `payload.namespace` via `s.sm.resolve_namespace()` to get the real `ns_id`, routes to the correct shard via `shard_for_namespace()`, and passes the correct depth/drill_in from the request.
- **`cluster_community_detect` error propagation (P1)**: Return type changed from `Json<DetectResponse>` to `Result<Json<DetectResponse>, (StatusCode, Json<Value>)>`. The `.ok()` that silently swallowed planner errors is replaced with `?` propagating a 500 INTERNAL_SERVER_ERROR.

### `crates/valori-node/src/server.rs`

- **Namespace truncation in shard routing (P1)**: Three occurrences of `(ns as u8).wrapping_rem(shard_count.max(1))` truncated the 16-bit `NamespaceId` to 8 bits before the modulo, causing misrouting for namespaces ≥ 256. Fixed to `((ns as u32) % (shard_count as u32).max(1)) as u8`.

## Findings

**Remaining known gaps (not fixed in this phase):**

- **BM25 reranking on cluster path**: `RaftKernelCapability::memory_search` over-fetches `k×10` candidates when `rerank=true` but never applies BM25 term scoring — the `ValoriReranker` corpus lives on `Engine` and doesn't replicate through Raft. Fixing properly requires either making the reranker part of `KernelState` (no_std constraint) or building the corpus lazily from stored text fields (non-trivial). Deferred.
- **GraphRAG f32 validation dropped**: `cluster_graphrag` no longer calls `to_fxp()` which previously returned 400 for out-of-range floats. The planner path passes raw f32 values directly to `FxpScalar` conversion. Low-severity (values clamp rather than panic), but diverges from standalone behavior.
- **Snapshot write lock narrowing**: `EngineKernelCapability::save_snapshot` holds a write lock on the engine during serialization and file I/O. Should copy state under read lock, release, then write. Deferred.
- **reqwest::Client not reused**: `tree_hybrid`, `extract_entities`, and several ingest handlers create new `reqwest::Client` instances per request. Should use a shared client from the registry. Deferred.

## Validation

```
cargo test -p valori-kernel -p valori-node
```

All suites pass:

| Suite | Result |
|---|---|
| valori-kernel unit tests | 31 passed |
| valori-node integration tests | 1 passed |
| valori-node route parity | 2 passed |
| valori-kernel format/snapshot/compat | 11+12+6+22+5+12+8+4+4+11+11+43+6+4+2 passed |
| Total | ~195 passed, 0 failed |

## Follow-ups

- **BM25 cluster reranking**: Needs `ValoriReranker` to be reconstructable from `KernelState` text fields. Complex; assign a dedicated phase.
- **GraphRAG f32 guard**: Re-add range validation inside `RaftKernelCapability::graph_rag` before `FxpScalar` conversion. Simple fix, low priority.
- **Snapshot lock narrowing**: Refactor `EngineKernelCapability::save_snapshot` to copy state under read lock before writing.
- **reqwest::Client reuse**: Thread a shared `reqwest::Client` through `CapabilityRegistry` or `SharedEngine`.
- **Integration test suite**: Standalone vs. cluster parity tests that run identical operations through both paths and assert identical outputs.
