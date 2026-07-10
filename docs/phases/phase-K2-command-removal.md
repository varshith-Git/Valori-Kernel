# Phase K2 — Command removal + ValoriKernel deletion

## Goal
Eliminate the `Command` enum from `valori-kernel` entirely and delete the legacy
`ValoriKernel` prototype struct, completing the cleanup started in K1. After K2,
the WAL writes `(KernelEvent, namespace_id)` pairs directly (v2 format); the
kernel has no knowledge of the WAL format.

## Delivered

| File | Change |
|---|---|
| `crates/valori-storage/src/wal_compat.rs` | **New.** `LegacyWalCommand` enum (private to storage) + `legacy_to_event()` — the only remaining reference to Command-shaped data, used exclusively for reading v1 WAL files |
| `crates/valori-storage/src/wal_reader.rs` | Rewritten. Version-aware: v1 files deserialize `LegacyWalCommand` and translate; v2 files deserialize `(KernelEvent, u16)` directly. Iterator now yields `WalResult<(KernelEvent, u16)>` — format hidden from callers |
| `crates/valori-storage/src/wal_writer.rs` | Rewritten. Writes v2 header (version=2) for new files; errors on existing v1 files. `append_event(&KernelEvent, u16)` replaces `append_command(&Command)` |
| `crates/valori-storage/src/lib.rs` | Added `mod wal_compat;` (private) |
| `crates/valori-storage/src/recovery.rs` | Removed `command_to_event()` and `Command` import; replay loop now destructures `(KernelEvent, u16)` directly from the reader; test updated to use `append_event` |
| `crates/valori-state/src/bootstrap.rs` | Same cleanup as `recovery.rs` |
| `crates/valori-node/src/commit/persistence.rs` | Removed `command_for()` + `Command` import; `Persistence::Wal` arm now calls `w.append_event(event, namespace_id)` |
| `crates/valori-kernel/src/state/command.rs` | **Deleted.** `Command` no longer exists in the kernel crate |
| `crates/valori-kernel/src/state/mod.rs` | Removed `pub mod command;` |
| `crates/valori-kernel/src/kernel.rs` | **Deleted.** `ValoriKernel` struct + CRC64 `state_hash()` + legacy binary `apply_event(&[u8])` gone |
| `crates/valori-kernel/src/lib.rs` | Removed `pub mod kernel;`, `pub use kernel::ValoriKernel;`, stale docstring references to both |
| `crates/valori-kernel/Cargo.toml` | Removed `crc64fast` dependency (was only used by `ValoriKernel::state_hash()`) |
| `crates/valori-cli/src/bin/bench_filter.rs` | **Deleted.** Legacy ValoriKernel bench |
| `crates/valori-cli/src/bin/bench_ingest.rs` | **Deleted.** Legacy binary-payload bench |
| `crates/valori-cli/src/bin/bench_recall.rs` | **Deleted.** Legacy SIFT recall bench using binary protocol |
| `crates/valori-cli/Cargo.toml` | Removed three deleted bin entries |

## Findings

1. **WAL v1 backward-compat**: v1 files can still be _read_ (via `wal_compat.rs`
   + `WalReader`), but the writer refuses to _append_ to them. This is acceptable
   because any node still on v1 WAL must replay it to recover state, then all new
   writes go to an EventLog. The WAL path is already legacy (Phase 23+).

2. **bench_ingest / bench_recall used raw binary protocol**: Both depended on
   `ValoriKernel::apply_event(&[u8])` — the pre-K1 binary command format. These
   bins could not be trivially migrated to `KernelState` because the SIFT loader
   + memory-mapped path was tightly coupled to the HNSW index's internal `i32`
   vectors (different from `FxpVector`). Deleted rather than ported; proper
   benchmarks are tracked as Priority 5.

3. **crc64fast dep removed**: The only remaining `crc64fast` usage in the kernel
   was `ValoriKernel::state_hash()`. Removing the dep reduces the `no_std`
   feature closure.

## Validation

- `cargo build --workspace` — zero errors, pre-existing warnings only
- `cargo build -p valori-kernel --target wasm32-unknown-unknown` — clean
- `cargo test -p valori-kernel -p valori-node` — 91 tests passing (22 kernel, rest node)
- `cargo test -p valori-storage -p valori-state` — 27 tests passing (including WAL roundtrip, recovery replay, and bootstrap replay)
- WASM build verified post-change

## Follow-ups

- **P5 benchmark suite**: Replace deleted bench bins with proper benchmarks against
  `KernelState` (insert throughput, search latency p50/p95/p99, snapshot encode/decode,
  replay events/sec, BF vs BQ recall+latency). Tracked as Priority 5.
- **WAL v1 cleanup**: Once no production nodes have v1 WAL files, `wal_compat.rs`
  and the v1 reader branch can be deleted. No urgency — reading is safe.
- **`crc64fast` still in `valori-cli/Cargo.toml`**: Direct dependency for the CLI's
  timeline BLAKE3 output (not the kernel CRC). No action needed.
