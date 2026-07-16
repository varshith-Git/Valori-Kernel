# Phase A6 — valori-effect

## Goal

Implement the Effect system: the single routing layer between task execution and
subsystems. All kernel writes, receipt fragments, audit entries, and metrics flow
as typed `Effect` values through an `EffectBus` that deduplicates by `EffectId`,
enabling safe task retries. Define seven `Capability` traits and the `Task` trait
that produces effects via `TaskContext`.

## Delivered

| File | Contents |
|---|---|
| `crates/valori-effect/Cargo.toml` | New crate; deps: valori-core (std), valori-planner, async-trait, serde, serde_json, thiserror, tracing, tokio (sync+time), blake3, bytes |
| `crates/valori-effect/src/error.rs` | `EffectError` with `CapabilityUnavailable`, `Dispatch`, `TaskFailed`, `BudgetExceeded`, `Serde`, `Duplicate`; `EffectResult<T>` |
| `crates/valori-effect/src/effect.rs` | `EffectId` (`BLAKE3(exec_id ‖ task_idx ‖ effect_idx)`), `EffectDurability` (Durable/Ephemeral), `KernelCommand`, `ReceiptFragment` with `read_only()`, `Effect` + `EffectPayload` (KernelWrite/Receipt/Audit/Counter/Gauge) |
| `crates/valori-effect/src/capability.rs` | `Capability` base trait; 7 async traits: `KernelCapability`, `EmbedCapability`, `LlmCapability`, `StorageCapability`, `HttpCapability`, `ProofCapability`, `SchedulerCapability`; `CapabilityRegistry` with helper accessors; `NoOpKernelCapability` for tests |
| `crates/valori-effect/src/bus.rs` | `EffectBus`: deduplication set (`Mutex<HashSet<EffectId>>`), `dispatch()`, `dispatch_all()`, `durable_count()`; routes KernelWrite → `KernelCapability`, Receipt/Audit → `ProofCapability`, Counter/Gauge → debug log |
| `crates/valori-effect/src/task.rs` | `TaskOutput` (`{ json: Value, state_hash_after }`), `TaskContext` (task_id, execution_id, topological_index, capabilities, bus, budget), `Task` async trait, `NoOpTask` |
| `crates/valori-effect/src/tasks/embed.rs` | `EmbedTask`: calls `EmbedCapability`, emits `Counter("embed_calls", n)` |
| `crates/valori-effect/src/tasks/insert_record.rs` | `InsertRecordTask`: emits `KernelWrite` (Durable) + `Counter("records_inserted", 1)` (Ephemeral) |
| `crates/valori-effect/src/tasks/search.rs` | `SearchTask`: emits `Receipt(ReceiptFragment{ mutated: false })` (Durable) + `Counter("searches", 1)` (Ephemeral) |
| `crates/valori-effect/src/tasks/mod.rs` | Module declarations |
| `crates/valori-effect/src/lib.rs` | Module declarations + top-level re-exports |
| `crates/valori-effect/README.md` | Crate README with architecture diagram, type table, dedup spec |
| `Cargo.toml` (workspace) | Added `valori-effect` to members, default-members, `[workspace.dependencies]` |

## Findings

- **`EffectId` dedup scope**: Scoped to one `ExecutionId` (one EffectBus per execution).
  A task retried in the *same* execution sees the same `EffectId`s (dedup catches double-writes).
  A task retried in a *new* execution (new `ExecutionId`) uses a fresh bus — intentional,
  since a new execution means the prior attempt was abandoned.
- **`SearchTask` inputs**: `namespace_id` and `k` are deserialized but not used in A6 —
  the actual search against the kernel happens in A7 when the TaskRunner threads results
  through `TaskOutput`. Marked `#[allow(dead_code)]` with a note.
- **No uuid dependency**: `InsertRecordTask` generates request IDs from `EffectId`
  (which uses `BLAKE3(exec_id ‖ task_idx ‖ 99)`) instead of `uuid::Uuid::new_v4()`.
- **Probe capability**: `ProofCapability::append_fragment` is optional; if absent the bus
  logs a debug message and continues (no hard failure). Receipt/audit entries are silently
  dropped — correct for standalone nodes without a proof sink configured.

## Validation

```
cargo test -p valori-effect
# 9 passed; 0 failed

cargo test -p valori-kernel -p valori-node
# All previously-passing tests still pass (effect crate adds no deps to node/kernel)
```

Tests cover: `EffectId` stability, `ReceiptFragment::read_only`, `Effect` JSON roundtrip,
`NoOpKernelCapability` zero hash, `CapabilityRegistry` absent-capability error, counter
dispatch, Durable dedup returns `Err(Duplicate)`, `dispatch_all` skips dedup errors,
`NoOpTask` returns empty output with state hash.

## Follow-ups

| Deferred to | What |
|---|---|
| Phase A7 | `ExecutionRegistry` wired into `valori-node`: TaskRunner drives `ExecutionGraph` → `Task::run` calls, threads outputs, marks handles terminal |
| Phase A7 | Real `KernelCapability` impl wrapping `SharedEngine` / `RaftHandle` |
| Phase A7 | Real `EmbedCapability` impl calling Ollama/OpenAI |
| Phase A8 | `ReceiptAssembler`: collects `ReceiptFragment`s in topological order → `Receipt` |
| Phase A9 | Remove dead code from `valori-storage/src/recovery.rs`; remove redundant node source files |
