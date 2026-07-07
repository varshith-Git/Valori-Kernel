# Phase E0 — Dead-file cleanup + architecture tripwire

## Goal

Remove the dead duplicate storage-layer files left behind in `valori-node`
by the Phase 1.1 workspace restructure, and make that failure mode
(extraction leaves stale copies behind) a permanent test failure.

## Delivered

| File | What |
|---|---|
| `crates/valori-node/src/wal_writer.rs` (deleted) | Dead copy of `valori-storage/src/wal_writer.rs` — byte-identical |
| `crates/valori-node/src/wal_reader.rs` (deleted) | Dead copy |
| `crates/valori-node/src/recovery.rs` (deleted) | Dead copy that had ALREADY drifted from the live `valori-storage` version (different error type + edits it never received) |
| `crates/valori-node/src/events/` (deleted, 6 files) | Dead copies of `valori-storage/src/events/` |
| `crates/valori-node/src/object_store/` (deleted) | Dead copy of `valori-storage/src/object_store.rs` |
| `crates/valori-node/tests/architecture.rs` (new) | Tripwire: a `.rs` file with the same crate-relative path existing in both `valori-node/src` and any of `valori-storage`/`valori-state`/`valori-metadata` fails the test |

All 10 deleted files were provably unreferenced: no `mod` declaration in
lib.rs (which re-exports the live crates: `pub use valori_storage::wal_writer`
etc.), no `#[path]` attribute, no `include!`/`include_str!`.

## Findings

1. The Phase 1.1 restructure moved storage/state into their own crates and
   kept old import paths working via lib.rs re-exports — but never deleted
   the originals. Anyone reading `use crate::wal_writer` in engine.rs
   naturally assumed the local file was the live one.
2. `recovery.rs` proved the drift risk is real, not theoretical — the copies
   had already diverged.
3. Same pattern exists at the API level: `valori-metadata`'s
   `CollectionRegistry` duplicates Engine's `NamespaceRegistry` with zero
   consumers. Owned by Phase E2 (see Follow-ups).

## Validation

- `cargo test -p valori-node` — **228 passed, 0 failed** (was 227; +1
  architecture test).
- Negative test: recreating `src/wal_writer.rs` makes the tripwire fail with
  a pointed message; verified manually.

## Follow-ups

- **Phase E2**: reconcile `NamespaceRegistry` (engine.rs) with
  `valori-metadata::CollectionRegistry` — port `MAX_NAMESPACES` cap +
  sidecar persistence, delete one of the two.
