# Phase K1 — Replay Unification

## Goal

Eliminate the `Command` intermediate type from the kernel's internal apply path, making `KernelEvent → KernelState::apply_event_ns` the single authoritative mutation path. Every subsystem that mutates state — standalone engine, cluster Raft apply, WAL recovery, test helpers — now goes through exactly one code path.

## Delivered

### `crates/valori-kernel/src/state/kernel.rs`
- Removed `#![allow(deprecated)]` module attribute and `Command` import.
- Inlined the entire body of `apply()` directly into `apply_event_ns()`. Each KernelEvent variant now contains its implementation directly — no intermediate translation.
- `AutoInsertRecordEncrypted` delegates to `InsertRecordEncrypted` via a single `return self.apply_event_ns(...)` call, eliminating a former three-hop chain (`AutoInsertRecordEncrypted → Command::InsertRecordEncrypted → apply() → apply_event_ns(InsertRecordEncrypted)`).
- Added `self.version = self.version.next()` at the end of `apply_event_ns` — this also fixes pre-existing version-bump omissions for `UpdateRecordMetadata`, `SetMeta`, `InsertRecordEncrypted`, and `ShredKey` which had no version bump on the direct-apply path.
- `create_node()` and `create_edge()` convenience methods now call `apply_event_ns` directly.
- Deleted `apply(cmd: &Command)` method entirely.

### `crates/valori-kernel/src/replay.rs` — **deleted**
- `replay_and_hash` had zero callers outside the file itself (confirmed by grep).
- `WalHeader` moved to `valori-storage/src/wal_reader.rs` (where it logically belongs — it describes the WAL file format).

### `crates/valori-kernel/src/lib.rs`
- Removed `pub mod replay`.

### `crates/valori-storage/src/wal_reader.rs`
- Added local `WalHeader` struct definition (moved from kernel).

### `crates/valori-storage/src/wal_writer.rs`
- Updated `WalHeader` import from `crate::wal_reader`.

### `crates/valori-storage/src/recovery.rs`
- Added `command_to_event()` helper that translates legacy `Command` → `(KernelEvent, namespace_id)`.
- `replay_wal` now calls `state.apply_event_ns(&evt, ns)` instead of `state.apply(&cmd)`.

### `crates/valori-state/src/bootstrap.rs`
- Same `command_to_event()` helper added.
- `replay_wal` updated to use `apply_event_ns`.

### `crates/valori-node/src/engine.rs`
- `create_collection`: replaced `Command::CreateNamespace` + `state.apply()` with `state.apply_event_ns(KernelEvent::AutoCreateNamespace)`.
- `drop_collection`: replaced `Command::DropNamespace` + `state.apply()` with `state.apply_event_ns(KernelEvent::DropNamespace)`.
- `apply_raw_for_test` renamed to `apply_event_for_test`, now takes `&KernelEvent` instead of `&Command`.

### `crates/valori-node/tests/replication_divergence.rs`
- Updated divergence-corruption test to use `apply_event_for_test(KernelEvent::SoftDeleteRecord)`.

## Findings

- **Version-bump bug fixed**: `UpdateRecordMetadata`, `SetMeta`, `InsertRecordEncrypted`, and `ShredKey` were previously not bumping `self.version` when applied via `apply_event_ns` directly (the cluster path). The version bump only fired via the Command path. Inlining fixed this by placing a single bump at the end of `apply_event_ns`.
- **WalHeader misplaced**: `WalHeader` defined in `valori-kernel::replay` had no business in the kernel — it's pure WAL file format. Moved to the storage crate where the reader/writer live.
- **`Command` still exists** in `state/command.rs` and is used by: `valori-storage` (WAL reader/writer), `valori-node/src/commit/persistence.rs` (legacy WAL write path), `valori-state/src/bootstrap.rs` (WAL replay), and `valori-cli` bench binaries. These are all WAL-format related. The Command type is now purely a storage serialization artifact — the kernel no longer creates or processes it internally.

## Validation

```
cargo test -p valori-kernel --features std
  → 93 tests: ok (all suites)

cargo test -p valori-node
  → all suites: 0 failed

cargo build -p valori-kernel --target wasm32-unknown-unknown
  → Finished (no_std invariant preserved)
```

## Follow-ups

- **Priority 2 (partial)**: `Command` in `state/command.rs` is now only a WAL serialization type. The next step is migrating `valori-storage` WAL read/write to use `KernelEvent` directly, then deleting `Command` entirely.
- **`ValoriKernel` in `kernel.rs`**: Still exists, gated to `#[cfg(feature = "std")]`. CLI bench bins still reference it. Delete after migrating those bins to `Engine`.
- **`command_to_event` duplication**: Same helper exists in `recovery.rs` and `bootstrap.rs`. If the WAL legacy path is deleted together (Priority 2 follow-through), both disappear at once.
