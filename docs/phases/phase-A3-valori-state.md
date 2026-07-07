# Phase A3 — `valori-state`: State lifecycle extraction

## Goal

Create a new `valori-state` crate that owns state lifecycle orchestration —
bootstrap, crash recovery, manifest, and graceful shutdown — and corrects the
placement error from Phase A2, where `recovery.rs` landed in `valori-storage`
despite orchestrating a state lifecycle, not raw byte movement.

This is Phase 3 of the architectural redesign:
`valori-core` → `valori-storage` → **`valori-state`** → `valori-planner` → …

## Delivered

### New crate: `crates/valori-state/`

| File | Contents | Lines |
|---|---|---|
| `Cargo.toml` | Manifest; deps: valori-core, valori-kernel, valori-storage, blake3, bincode, serde, serde_json, thiserror, tracing, metrics | — |
| `src/lib.rs` | Module declarations + re-exports `StateError`, `StateLifecycle`, `StateManifest` | 20 |
| `src/error.rs` | `StateError` (Kernel / InvalidInput / Io); `StateResult<T>`; `From<StorageError>` + `From<KernelError>` | 30 |
| `src/bootstrap.rs` | `recover_from_events`, `has_event_log`, `replay_wal`, `has_wal`, `load_snapshot`, `validate_snapshot`; `BootstrapMode` enum; 3 unit tests | 155 |
| `src/manifest.rs` | `StateManifest` (snapshot_path, event_log_segments, last_applied_height, state_hash); `save()` / `load()` | 55 |
| `src/lifecycle.rs` | `StateLifecycle` enum (Recovering / Ready / Snapshotting); `is_ready()`, `is_recovering()`, `Display` | 45 |
| `src/shutdown.rs` | `shutdown_snapshot(state, path)` — encodes `KernelState` and writes to disk synchronously | 35 |

### Modified: `crates/valori-node/`

- `Cargo.toml` — added `valori-state` workspace dependency
- `src/lib.rs` — replaced `pub use valori_storage::recovery` with `pub use valori_state::bootstrap as recovery`; zero call-site changes in `engine.rs` or any other file
- `src/errors.rs` — added `From<valori_state::StateError> for EngineError`

### Modified: `Cargo.toml` (workspace root)

- `valori-state` added to `members`, `default-members`, and `[workspace.dependencies]`

## Findings

1. **Placement correction confirmed.** `recovery.rs` in `valori-storage` was
   architecturally wrong (confirmed by RFC-0002 §11). The functions it contained
   are state lifecycle operations, not raw I/O. Moving them to `valori-state::bootstrap`
   puts them in the correct layer.

2. **`valori-storage::recovery` still exists** and is still exported by `valori-storage`
   (unchanged). `valori-node` now uses `valori-state::bootstrap` instead. The old
   `recovery.rs` in `valori-storage` is dead code from `valori-node`'s perspective —
   it will be removed in Phase A9 (cleanup pass).

3. **`encode_state` appends to `Vec<u8>`** (returns `Result<()>`, not `usize`). The
   `shutdown_snapshot` implementation was corrected to use a pre-allocated `Vec` and
   write the whole buffer rather than slicing by a non-existent length return.

4. **`From<StorageError> for StateError`** is the glue between the two layers. Since
   `valori-state` depends on `valori-storage`, this conversion is one-way and
   does not create a circular dependency.

5. **`valori-storage` cannot re-export from `valori-state`** — that would be circular
   since `valori-state` depends on `valori-storage`. The two error types coexist;
   `valori-node` uses `StateError` for the recovery path.

## Validation

```
cargo build -p valori-state                  ✓  (no errors, 1 pre-existing kernel warning)
cargo build -p valori-node                   ✓  (clean)
cargo test -p valori-state                    3 passed / 0 failed
cargo test -p valori-kernel -p valori-node   all pre-existing tests pass
```

Total tests passing after this phase: 335+ (3 new in `valori-state`; all prior passing).

## Follow-ups

| Item | Phase |
|---|---|
| Remove `valori-storage/src/recovery.rs` (now dead code from node's perspective) | A9 |
| Wire `valori-node` graceful-shutdown path to `valori_state::shutdown::shutdown_snapshot` (currently inline in `engine.rs`) | A6 |
| Add `StateManifest` persistence to `engine.rs` bootstrap so restarts can skip WAL replay when the manifest is current | A4/A5 |
| `valori-consensus` uses ad-hoc string errors for recovery paths — replace with `StateError` | A6 |
