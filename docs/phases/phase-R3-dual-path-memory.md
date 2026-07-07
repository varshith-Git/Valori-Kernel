# Phase R3 — Dual-path unification: memory domain (upsert, search, consolidate, contradict)

## Goal

Complete the dual-path unification began in Phases R1 and R2 by migrating the largest remaining duplicated surface—the Memory Domain (`upsert`, `search`, `consolidate`, `contradict`)—into `crates/valori-node/src/routes/memory.rs`. This eradicates the "Two Kitchens" problem where `server.rs` (standalone) and `cluster_server.rs` (cluster mode) maintained over 600 lines of duplicate and diverging handler boilerplate.

## Delivered

| File | What |
|---|---|
| `crates/valori-node/src/routes/memory.rs` (new) | `MemoryOps` trait + shared DTO structs + shared handler logic for `POST /v1/memory/upsert`, `POST /v1/memory/search`, `POST /v1/memory/consolidate`, and `POST /v1/memory/contradict`. Incorporates shared receipt emission via `receipt_bridge` and consistent JSON error handling. |
| `crates/valori-node/src/routes/mod.rs` | Exported `pub mod memory;`. |
| `crates/valori-node/src/api.rs` | Added `log_index: Option<u64>` (with `#[serde(skip_serializing_if = "Option::is_none")]`) to `MemoryUpsertResponse`, `MemoryConsolidateResponse`, and `MemoryContradictResponse`. Standalone responses remain byte-identical while cluster mode preserves its Raft log index tracking. |
| `crates/valori-node/src/server.rs` | Implemented `MemoryOps` for `SharedEngine`. Replaced ~200 lines of manual handler logic for the memory domain with 3-line wrappers delegating to `routes::memory`. |
| `crates/valori-node/src/cluster_server.rs` | Implemented `MemoryOps` for `DataPlaneState`. Deleted private duplicate DTO structs (`ClusterMemoryConsolidateRequest`, `ClusterMemoryContradictRequest`). Replaced ~210 lines of manual handler logic for memory upsert, search, consolidate, and contradict with 3-line wrappers delegating to `routes::memory`. |
| `crates/valori-node/tests/route_parity.rs` | Verified ongoing 100% route and method parity between standalone and cluster modes. |

## Architectural Highlights

1. **Unified Memory Domain Contract (`MemoryOps` trait)**:
   - Encapsulates domain variations between standalone engine execution and cluster Raft state machines:
     - `resolve_collection`: Maps collection names to namespace IDs.
     - `ensure_read_consistency`: Enforces linearizable or local read consistency (no-op in standalone, invokes Raft read-index protocol in cluster mode).
     - `upsert_vector`: Handles document node reuse/creation, chunk node creation, vector insertion, and ParentOf edge linking.
     - `search_vector`: Handles fixed-point vector search with decay reranking and metadata attachment.
     - `consolidate`: Soft-deletes superseded records, inserts replacement vectors, creates Supersedes graph edges, and transfers metadata.
     - `contradict`: Computes cosine similarity between searchable records and commits Contradicts graph edges if similarity exceeds threshold.

2. **Zero Route Divergence**:
   - Both HTTP routers now point to thin wrappers around the exact same domain logic in `routes::memory`, ensuring that any new memory features or bug fixes automatically benefit both standalone and cluster modes.

3. **Receipt & Audit Trail Consistency**:
   - Shared memory upsert handlers emit operation receipts via `crate::receipt_bridge::emit_write` for verifiable AI execution proofs across both execution paths.

## Validation

- `cargo test -p valori-node --test route_parity` — **2 passed, 0 failed** (verifies identical route sets and methods between standalone and cluster servers).
- `cargo test -p valori-node` — **All unit and integration tests passed** (including replication bootstrap, cluster sync, divergence healing, and memory domain tests).
