# Phase A1 — `valori-core`: Zero-dependency type foundation

## Goal

Extract all shared platform types (IDs, enums, version, errors) into a new
`valori-core` crate that has zero domain dependencies. Every crate in the
workspace depends on `valori-core`; `valori-core` depends on nothing except
`serde` and `thiserror` (both `no_std`-compatible).

This is Phase 1 of the architectural redesign:
`valori-core` → `valori-storage` → `valori-query` → `valori-planner` → …

## Delivered

### New crate: `crates/valori-core/`

| File | Contents |
|---|---|
| `Cargo.toml` | Zero-dep manifest; `no_std` by default; `std` feature opt-in |
| `src/lib.rs` | Root module; re-exports everything at the crate root |
| `src/id.rs` | `RecordId`, `NodeId`, `EdgeId`, `NamespaceId`, `CollectionId` (alias), `ExecutionId`, `ShardId`, `ClusterEpoch`; `DEFAULT_NS`, `NS_LIST_NIL`, `MAX_NAMESPACES` |
| `src/enums.rs` | `NodeKind`, `EdgeKind` |
| `src/version.rs` | `Version` — monotonic schema counter |
| `src/error.rs` | `CoreError`, `Result<T>` |

### New types (did not exist before)

| Type | Purpose |
|---|---|
| `CollectionId` | User-facing alias for `NamespaceId` — same bits, cleaner API boundary |
| `ExecutionId` | 128-bit ID for execution graphs (used by Phase A4 planner) |
| `ShardId` | Shard identifier (previously a bare `u32` in `consensus/types.rs`) |
| `ClusterEpoch` | Cluster membership version counter |

### Modified: `crates/valori-kernel/`

- `Cargo.toml` — added `valori-core` workspace dependency; propagated `std` feature
- `src/types/id.rs` — now re-exports from `valori-core` instead of defining its own types
- `src/types/enums.rs` — now re-exports `NodeKind`/`EdgeKind` from `valori-core`

All existing kernel consumers (`valori_kernel::types::id::RecordId`, etc.) continue
to work without changes — the re-export keeps the public API identical.

### Modified: `Cargo.toml` (workspace root)

- `valori-core` added to `members`, `default-members`, and `[workspace.dependencies]`

## Findings

1. `Version` was defined in `kernel/src/types/id.rs` alongside IDs — semantically it belongs in core (it's a schema-version concept, not a kernel algorithm).
2. `ShardId` was a bare `u32` in `valori-consensus/src/types.rs`. It is now a typed newtype in core, preventing accidental mixing with `NodeId` or `RecordId`.
3. `valoricore-ffi` links against Python symbols and cannot build without maturin — excluded from workspace default build (pre-existing, not introduced here).

## Validation

```
cargo build -p valori-core                           ✓
cargo build -p valori-kernel                         ✓
cargo build -p valori-node -p valori-consensus …     ✓  (all non-ffi crates)
cargo build -p valori-core --target wasm32-unknown-unknown   ✓
cargo build -p valori-kernel --target wasm32-unknown-unknown ✓
cargo test -p valori-core -p valori-kernel -p valori-node    ✓  all pass
```

## Follow-ups

| Item | Phase |
|---|---|
| Make `valori-consensus` import `ShardId` from `valori-core` instead of defining its own `u32` wrapper | A1 cleanup |
| Make `valori-node` import `CollectionId` from `valori-core` instead of the raw `u16` | A1 cleanup |
| Create `valori-storage` — extract WAL, events, object store, recovery from `valori-node` | Phase A2 |
| Create `valori-query` — typed request/response types (no parser yet) | Phase A3 |
