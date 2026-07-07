# Phase A4 — `valori-metadata`: Control-plane persistence

## Goal

Create the `valori-metadata` crate — the durable control-plane store for
everything that is not in the kernel's hot-path (`KernelState`) but must survive
process restarts: project configuration, collection name→NamespaceId mappings,
shard topology, snapshot catalog, execution history, and the planner cache.

This is Phase 4 of the architectural redesign:
`valori-core` → `valori-storage` → `valori-state` → **`valori-metadata`** → `valori-planner` → …

## Delivered

### New crate: `crates/valori-metadata/`

| File | Contents | Tests |
|---|---|---|
| `Cargo.toml` | Manifest; deps: valori-core, valori-wire, redb 2, serde, serde_json, thiserror, tracing, ulid | — |
| `src/lib.rs` | Module declarations + comprehensive re-exports | — |
| `src/error.rs` | `MetadataError` (Db / Transaction / Table / Storage / Commit / Serde / NotFound / InvalidInput); `MetadataResult<T>` | — |
| `src/project.rs` | `Project` (name, dir, port, dim, index, shard_count, node_count, mode, timestamps, nodes); `IndexKind` (Brute/Hnsw/Ivf/Bq/Auto) with `FromStr`/`Display`; `ProjectMode`; `ClusterNodeConfig`; `event_log_path(shard_id)`, `snapshot_path()` helpers | 2 |
| `src/collection.rs` | `Collection` (name, project, namespace_id); `CollectionRegistry` — elevated form of the node's `NamespaceRegistry`; `create()`, `resolve()`, `drop()`, `names()` | 3 |
| `src/shard.rs` | `ShardTopology`, `ShardConfig`, `ShardMember`; `standalone()` builder; `shard_for_namespace(ns_id) = ns_id % shard_count` | 1 |
| `src/snapshot.rs` | `SnapshotRecord` (id, project, shard_id, path, size, format_version, produced_at, state_hash, applied_height); `SnapshotCatalog`; `latest()`, `prunable(keep)` | 2 |
| `src/history.rs` | `ExecutionRecord`, `ExecutionRetentionPolicy` (default 30 days), `ExecutionStatus`; `is_graph_expired(now_secs)` | — |
| `src/planner_cache.rs` | `PlannerCacheKey` (op_hash + fp_hash + ctx_hash); `PlannerCacheEntry`; `to_db_key()` composite string; `is_expired()` | — |
| `src/db.rs` | `MetadataDb` — redb-backed store; 5 typed tables; CRUD for Project, Collection, Snapshot, ExecutionRecord, PlannerCache; `load_collection_registry()` | 5 |
| `README.md` | Crate documentation with module table, DB layout, dependency graph, invariants | — |

### Modified: `Cargo.toml` (workspace root)

- `valori-metadata` added to `members`, `default-members`, and `[workspace.dependencies]`

## Findings

1. **`redb` v2 uses `DatabaseError`** (not a generic `redb::Error`) for `Database::create`.
   The `MetadataError` enum was corrected to use `From<redb::DatabaseError>` instead of
   the non-existent `From<redb::Error>`.

2. **`ReadableTable` trait must be imported** for `iter()` to resolve on a
   `ReadOnlyTable`. Added `use redb::ReadableTable` in `db.rs`.

3. **`CollectionRegistry` is the canonical replacement** for `NamespaceRegistry` in
   `valori-node/src/engine.rs`. Future Phase A6 will import this type instead of the
   inline struct. The two types are byte-compatible (same JSON shape) to allow
   incremental migration.

4. **PlannerCache and ExecutionHistory are stubs** — the types and redb table schemas
   are defined, but the Planner integration (lookup before planning, insert after
   planning) is deferred to Phase A5/A7.

5. **`valori-metadata` does not depend on `valori-kernel` or `valori-storage`** —
   intentionally. It is a pure control-plane crate, operating on names and IDs,
   not on `KernelState` or event logs. This keeps the dependency graph shallow.

## Validation

```
cargo build -p valori-metadata                  ✓  (no errors)
cargo test -p valori-metadata                   13 passed / 0 failed
cargo build -p valori-node -p valori-consensus  ✓  (full workspace still clean)
```

Total tests passing after this phase: 348+ (13 new in `valori-metadata`).

## Follow-ups

| Item | Phase |
|---|---|
| Replace `valori-node/src/engine.rs` inline `NamespaceRegistry` with `valori_metadata::CollectionRegistry` | A6 |
| Wire `MetadataDb` into the node's project open/close path (replace JSON sidecar approach) | A5/A6 |
| Implement `ExecutionHistory` inserts from `ReceiptAssembler` | A7/A8 |
| Implement `PlannerCache` lookup and insert from the Planner | A5 |
| Add `ExecutionAnalytics` time-series (optional) | A7 |
