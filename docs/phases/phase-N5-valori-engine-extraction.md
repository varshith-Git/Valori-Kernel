# Phase N5 — valori-engine extraction

## Goal

Extract the `Engine` struct and all supporting types (`EngineConfig`, `EngineHealth`, `EngineError`, `CommitError`, `MetadataStore`, `Persistence`, `RecoveryMode`, `PoolStats`, `ExecutionResources`) from `crates/valori-node` into a new standalone `crates/valori-engine` crate, applying SOLID principles throughout.

## Delivered

| File | Change |
|---|---|
| `crates/valori-engine/` | New crate (6 files) |
| `crates/valori-engine/Cargo.toml` | Workspace-linked; depends on kernel, index, search, ingest, rag, metadata, storage, state |
| `crates/valori-engine/src/lib.rs` | Public re-exports for all 5 modules |
| `crates/valori-engine/src/config.rs` | `IndexKind`, `QuantizationKind`, `EngineConfig` (with DIP: vault + object_store injected by caller) |
| `crates/valori-engine/src/error.rs` | `CommitError`, `EngineError` (implements `IntoResponse`), all `From<>` conversions |
| `crates/valori-engine/src/metadata.rs` | `MetadataStore` (thread-safe, atomic flush) |
| `crates/valori-engine/src/persistence.rs` | `Persistence` enum (EventLog / WAL / Ephemeral) — Phase E1 durability funnel |
| `crates/valori-engine/src/engine.rs` | `Engine::with_config(EngineConfig)` — primary constructor; all methods migrated |
| `crates/valori-engine/README.md` | Full crate README |
| `crates/valori-node/src/engine.rs` | Replaced 1 743-line file with 90-line shim: re-exports + `EngineFromNodeConfig` trait |
| `crates/valori-node/src/errors.rs` | Replaced with `pub use valori_engine::EngineError;` |
| `crates/valori-node/src/metadata.rs` | Replaced with `pub use valori_engine::MetadataStore;` |
| `crates/valori-node/src/commit/persistence.rs` | Replaced with `pub use valori_engine::Persistence;` |
| `crates/valori-node/src/config.rs` | Removed `IndexKind`/`QuantizationKind` definitions; replaced with `pub use valori_engine::{…}` |
| `crates/valori-node/src/lib.rs` | Added `pub use engine::EngineFromNodeConfig;` |
| `crates/valori-node/src/main.rs` | Added `use valori_node::EngineFromNodeConfig;` |
| `crates/valori-node/tests/*.rs` (31 files) | Added `use valori_node::EngineFromNodeConfig;` |
| `crates/valori-node/examples/crash_recovery_demo.rs` | Added `use valori_node::EngineFromNodeConfig;` |
| `Cargo.toml` (workspace root) | Added valori-engine to members, default-members, workspace.dependencies |
| `crates/valori-node/Cargo.toml` | Added `valori-engine = { workspace = true }` |

## Findings

- `Engine::new(&NodeConfig)` was called in 31 test files + main.rs + examples. Moving Engine to a different crate breaks this unless a compatibility shim is provided. The `EngineFromNodeConfig` extension trait solves this with zero change to call sites (just one new `use` per file).
- `valori-kernel::crypto::VaultError` doesn't exist — the actual error type is `CryptoError`. Test NoopVault used the wrong name; also missing `key_exists` from the `KeyVault` trait.
- Several test files had module-level `//!` doc comments (`multi_arch_determinism.rs`) — the bulk `sed -i 1s/^/use .../` prepended before the comment, making it an invalid outer doc. Fixed manually.
- `commit/mod.rs` retains its own `CommitError` for the `Committer` trait (cluster seam); `valori-engine::CommitError` is the standalone persistence variant. These are intentionally separate: the cluster path goes through the `Committer` trait, not `Persistence`.

## Validation

```
cargo test -p valori-engine   →  5 passed, 0 failed
cargo test -p valori-node     →  ~220 passed, 0 failed
cargo build -p valori-engine -p valori-node  →  0 errors, warnings only (pre-existing)
```

## Follow-ups

- `valori-ffi/src/lib.rs` may also need `use valori_node::EngineFromNodeConfig;` (excluded from default-members; verify when building the Python SDK).
- `EngineConfig` currently has no `Default` impl — test code must set all fields. A builder or `Default` impl would reduce boilerplate.
- The `commit/mod.rs` `CommitError` and `valori-engine::CommitError` are structurally identical; a future phase could unify them.
