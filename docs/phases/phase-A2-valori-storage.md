# Phase A2 — `valori-storage`: Durable storage layer extraction

## Goal

Extract all disk-touching code from `valori-node` into a new `valori-storage`
crate: WAL read/write, append-only event log, event journal, crash recovery, and
object-store (S3/file) backend. `valori-node` re-exports these modules so zero
call-site imports change.

This is Phase 2 of the architectural redesign:
`valori-core` → **`valori-storage`** → `valori-query` → `valori-planner` → …

## Delivered

### New crate: `crates/valori-storage/`

| File | Contents | Lines |
|---|---|---|
| `Cargo.toml` | Manifest; deps: blake3, bincode, opendal, tokio/sync, metrics | — |
| `src/lib.rs` | Module declarations + re-exports `StorageError` | — |
| `src/wal_writer.rs` | `WalWriter` — append-only WAL with 16-byte header (version / dim / CRC) | 150 |
| `src/wal_reader.rs` | `WalReader` — header-validated iterator over `Command`s | 155 |
| `src/events/mod.rs` | Module + public re-exports (`EventLogWriter`, `EventJournal`, etc.) | 27 |
| `src/events/event_log.rs` | Append-only BLAKE3-chained log (v2/v3 formats, rotation, splice) | 543 |
| `src/events/event_journal.rs` | Runtime committed/buffer distinction; tokio broadcast for live tailing | 264 |
| `src/events/event_commit.rs` | `EventCommitter` — shadow-first commit barrier; batch commit; auto-rotation | 466 |
| `src/events/event_replay.rs` | `recover_from_event_log`, `read_all_segments`, chain-splice verification | 434 |
| `src/events/event_proof.rs` | `EventProof` struct + BLAKE3 log hash + proof generation | 153 |
| `src/object_store.rs` | `ObjectStoreBackend` — S3/file upload/download/list/prune via opendal | 304 |
| `src/recovery.rs` | `replay_wal`, `recover_from_events`, `validate_snapshot`; defines `StorageError` | 116 |

### Modified: `crates/valori-node/`

- `Cargo.toml` — added `valori-storage` workspace dependency
- `src/lib.rs` — replaced `pub mod wal_writer; pub mod wal_reader; pub mod events; pub mod recovery;` and the `object_store` directory module with `pub use valori_storage::{wal_writer, wal_reader, events, recovery, object_store}` — zero import changes in any call-site file
- `src/errors.rs` — added `From<valori_storage::StorageError> for EngineError`

### Modified: `Cargo.toml` (workspace root)

- `valori-storage` added to `members`, `default-members`, and `[workspace.dependencies]`

## Findings

1. **`EngineError` was the only coupling** between `recovery.rs` and `valori-node` internals. Resolved by defining `StorageError` in `valori-storage` and a `From` impl in `valori-node/src/errors.rs`. No other cross-layer leakage existed.
2. **`event_journal.rs` uses `tokio::sync::broadcast`** — `valori-storage` therefore takes a `tokio = { features = ["sync"] }` dep. This is correct: the event log is a server-side concern; `no_std` is only required for `valori-core` and `valori-kernel`.
3. **Source files were byte-identical** copies — all `crate::events::*` self-references become valid in the new crate without any edits because the module hierarchy is preserved.
4. **`persistence.rs`** (snapshot save/load with CRC header, `IndexKind`/`QuantizationKind` deps) was intentionally NOT moved — it depends on `crate::config` node-level types. Deferred to Phase A3 or later when config types are extracted.

## Validation

```
cargo build -p valori-storage                    ✓  (no warnings in new code)
cargo build -p valori-node                       ✓  (3 pre-existing unused-mut warnings, not introduced here)
cargo test -p valori-storage                     23 passed / 0 failed
cargo test -p valori-kernel -p valori-node       all existing tests pass
```

Total passing after this phase: 332+ (23 new in `valori-storage`; 309 pre-existing).

## Follow-ups

| Item | Phase |
|---|---|
| Remove now-redundant source files from `valori-node/src/` (`wal_writer.rs`, `wal_reader.rs`, `recovery.rs`, `events/`, `object_store/`) — currently kept for a safe rollback window | A2 cleanup |
| Extract `persistence.rs` + `config.rs` snapshot/config types | Phase A3 |
| Make `valori-consensus` import `StorageError` instead of ad-hoc string errors | Phase A3 |
