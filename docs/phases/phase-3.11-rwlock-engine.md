# Phase 3.11 — Concurrent reads via RwLock engine

## Goal

Replace the `Arc<Mutex<Engine>>` exclusive lock with `Arc<RwLock<Engine>>` so that read-only HTTP handlers (search, health, proof, timeline, etc.) can execute concurrently instead of serializing behind a single global lock. Write handlers keep exclusive access.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/server.rs` | `SharedEngine` type changed from `Arc<Mutex<Engine>>` to `Arc<RwLock<Engine>>`; 18 read-only handlers converted from `.write().await` to `.read().await`; write handlers retain `.write().await` |
| `crates/valori-node/src/main.rs` | `use tokio::sync::Mutex` → `RwLock`; `Mutex::new` → `RwLock::new`; auto-snapshot task uses `.read().await` |
| `crates/valori-node/src/replication.rs` | Hash-checker task and start-offset read use `.read().await`; event application loop (mutating) and bootstrap use `.write().await` |
| `crates/valori-node/tests/replication_divergence.rs` | `Mutex` → `RwLock` |
| `crates/valori-node/tests/replication_bootstrap.rs` | `Mutex` → `RwLock`; read-only locks converted to `.read()` |
| `crates/valori-node/tests/health_metrics.rs` | `Mutex` → `RwLock` |
| `crates/valori-node/tests/api_as_of.rs` | `Mutex` → `RwLock` |
| `crates/valori-node/tests/api_crypto_shred.rs` | `Mutex` → `RwLock` |
| `crates/valori-node/tests/api_replication.rs` | `Mutex` → `RwLock` |
| `crates/valori-node/tests/api_batch_ingest.rs` | `Mutex` → `RwLock`; read-only lock converted |
| `crates/valori-node/tests/api_keys.rs` | `Mutex` → `RwLock` |
| `crates/valori-node/tests/replication_cluster.rs` | `Mutex` → `RwLock`; follower search locks converted to `.read()` |
| `crates/valori-node/tests/collections.rs` | `Mutex` → `RwLock`; read-only locks converted |

**Read-only handlers converted** (18 total): `health_check`, `snapshot_save`, `meta_set`, `meta_get`, `search`, `search_as_of`, `get_node`, `list_nodes`, `get_edges`, `get_subgraph`, `snapshot` download, `memory_search_vector`, `get_proof`, `get_event_proof`, `get_wal_stream`, `metrics_handler`, `get_timeline`, `list_collections_handler`, `list_remote_snapshots`, `upload_snapshot_to_store` (snapshot read), `list_remote_wal`, `archive_wal_segment`, `crypto_status_handler`, `restore_from_store` (object-store clone and post-restore hash reads).

**Write-only handlers kept**: `delete_record`, `snapshot_restore`, `insert_record`, `batch_insert`, `create_node`, `create_edge`, `delete_node`, `memory_upsert_vector`, `insert_encrypted_handler`, `shred_key_handler`, `create_collection_handler`, `drop_collection_handler`, `get_replication_events` (flush requires `&mut`).

## Findings

- `meta_set` calls `engine.metadata.set()` and `engine.flush_metadata()` — both take `&self` via interior mutability (`RwLock<HashMap>` inside `MetadataStore`), so it correctly uses `.read()` at the `Engine` level.
- `get_replication_events` calls `committer.flush_log()` which requires `&mut self`, so it must stay `.write()`.
- `upload_snapshot_to_store` calls `engine.snapshot()` (read) then releases the lock before async I/O — correctly uses `.read()` for the capture phase.

## Validation

```
cargo test -p valori-node -p valori-kernel
224 passing, 0 failing
```

## Follow-ups

- Phase 3.13 — HNSW parameter exposure: `VALORI_HNSW_M`, `VALORI_HNSW_EF_CONSTRUCTION`, `VALORI_HNSW_EF_SEARCH`
