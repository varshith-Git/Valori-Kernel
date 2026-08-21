# Phase 2.4 — Complete Storage Coherence

## 1. Goal
Close the storage coherence gaps in the Valori storage engine by making graph entities collection-owned and durable in Collection snapshots, wiring production WAL rotation to publish sealed segments through `StorageProvider`, streaming logical WAL segments during recovery without unbounded allocations, and decoupling project recovery from raw filesystem paths.

## 2. Delivered

### Kernel
- `crates/valori-kernel/src/state/kernel.rs`:
  - Added namespace-scoped graph iterators: `iter_nodes_in_ns(&self, namespace_id: u16) -> impl Iterator<Item = &GraphNode>` and `iter_edges_in_ns(&self, namespace_id: u16) -> impl Iterator<Item = &GraphEdge>`.
  - Added full graph edge iterator: `iter_edges(&self) -> impl Iterator<Item = &GraphEdge>`.

### Storage
- `crates/valori-storage/src/collection_snapshot.rs`:
  - Upgraded schema to V3 (`COLLECTION_SNAPSHOT_SCHEMA_VERSION = 3`).
  - Added `CollectionSnapshotNode` (`id`, `kind`, `record`) and `CollectionSnapshotEdge` (`id`, `kind`, `from`, `to`).
  - Extended `CollectionSnapshotMeta` with `node_count`, `edge_count`, `node_pool_ceiling`, `edge_pool_ceiling`.
  - Backward-compatible decoder: V2 snapshots decode with empty graph collections.
  - Implemented multi-collection hole-filling during `restore_project_into` for records, graph nodes, and graph edges, ensuring monotonic global allocator alignment without inter-collection corruption.
- `crates/valori-storage/src/events/event_commit.rs`:
  - Added `with_storage_provider` and `set_storage_provider` to `EventCommitter`.
  - Wired `maybe_rotate` and `rotate_log` to publish sealed WAL segments to `StorageKey::WalSegment` via `provider.put_immutable`.
- `crates/valori-storage/src/events/event_replay.rs`:
  - Implemented `read_segment_bytes` for parsing segment buffers in memory.
  - Implemented `stream_events_from_provider` to stream only events `> after_lsn` across sealed segments in `StorageProvider` and active WAL files.

### State & Engine
- `crates/valori-state/src/collection_bootstrap.rs`:
  - Updated `recover_project_from_snapshots` and `snapshot_collection` to persist and restore graph nodes and edges with V3 snapshots.
  - Implemented `recover_project_from_storage(provider, project_id, shard_id, active_wal_path)` utilizing `stream_events_from_provider` and namespace-scoped LSN filtering.
  - Updated `recover_project_with_wal_tail` to delegate to `recover_project_from_storage`.
- `crates/valori-engine/src/engine.rs`:
  - Updated `configure_storage_provider` to inject `provider` into `EventCommitter` when active.
  - Updated `try_recover` to invoke `recover_project_from_storage`.
- `crates/valori-engine/Cargo.toml`:
  - Added `valori-core = { workspace = true }`.

### Architecture & Tests
- `crates/valori-node/tests/dependency_direction.rs`:
  - Updated `DOMAIN_FIREWALL` contract to reflect that `valori-storage` and `valori-state` are authorized consumers of domain identities (`ProjectId`, `ProjectName`, `ProjectTopology`, `Metric`).

## 3. Findings
1. **Allocator Ceilings Across Collections**: `NodeId` and `EdgeId` share global monotonic slab allocators. When snapshots are taken per-collection, hole-filling with throwaway create+delete events is necessary to advance the node and edge counters to match the original allocation sequence across collections.
2. **Node-before-Edge Invariant**: When restoring graph state from multiple collection snapshots, all nodes across all collections must be restored before any edges are created to ensure that `from` and `to` node endpoints exist when `CreateEdge` events execute.

## 4. Validation
- `valori-storage`: 65 unit tests, 2 compat tests, 11 validation tests (passed).
- `valori-state`: 19 unit tests, 5 compat tests (passed).
- `valori-engine`: 15 unit tests (passed).
- `valori-kernel`: 153 tests (passed).
- `valori-node`: 363 tests including dependency direction & route parity (passed).
- `wasm32-unknown-unknown` compilation: verified clean.

## 5. Follow-ups
- Multi-shard StorageProvider recovery routing in cluster mode (Raft snapshot transfer integration) owned by future cluster storage phase.
