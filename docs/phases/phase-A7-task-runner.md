# Phase A7 — TaskRunner + Real Capabilities

## Goal

Wire the effect system (A6) into `valori-node` by implementing: (1) concrete
capability adapters that bridge `KernelCapability` and `EmbedCapability` to the
live `SharedEngine` / HTTP client, and (2) a `TaskRunner` that drives one
`ExecutionGraph` to completion in topological order, threads `TaskOutput`s
between tasks, enforces retry limits, and marks the `ExecutionHandle` terminal.

## Delivered

| File | Contents |
|---|---|
| `crates/valori-node/src/capabilities.rs` | `EngineKernelCapability` — wraps `SharedEngine`: deserializes `event_json` → `KernelEvent`, calls `apply_committed_event_ns()`, returns BLAKE3 state hash. `state_hash()` uses `try_read()` to avoid blocking. `NoRaftKernelCapability` — stub for cluster path (Phase A9). `HttpEmbedCapability` — wraps `EmbedConfig` + `reqwest::Client`, delegates to existing `embed_batch()`. `PassthroughHttpCapability` — outbound GET via reqwest. `CapabilityRegistryBuilder` — standalone convenience builder. |
| `crates/valori-node/src/runner.rs` | `TaskRegistry` — maps `TaskKind → Arc<dyn Task>` for all 12 task kinds (real impls for Embed/InsertRecord/Search; `NoOpTask` stub for the rest). `TaskRunner` — `run()` walks topo order, resolves predecessor outputs, builds `TaskContext`, calls `run_with_retry()`, updates handle at each step, marks Succeeded/Failed. `run_graph()` — spawns a `TaskRunner` on the tokio runtime, returns `ExecutionHandle`. |
| `crates/valori-node/src/lib.rs` | Added `pub mod capabilities` and `pub mod runner` |
| `crates/valori-node/Cargo.toml` | Added `valori-core`, `valori-planner`, `valori-effect`, `valori-metadata`, `async-trait` |

## Findings

- **`tokio::spawn` + tracing format args**: `tracing!` macros with `%` (Display)
  hold `Arguments<'_>` across `.await`, making the future non-Send. Fixed by
  materializing values before the await (e.g., `let n = bus.durable_count().await;
  info!(n, "done");`) — a subtle footgun when mixing tracing with async.
- **`EngineKernelCapability::state_hash()` is synchronous**: uses `try_read()`
  rather than `read().await` to avoid blocking the executor. Returns zero-hash on
  lock contention — acceptable because this call is proof-trail only; the authoritative
  hash is computed by the audit chain.
- **`NoRaftKernelCapability` is always-unavailable**: the cluster data plane is
  restructured in Phase A9. The stub ensures no cluster handler accidentally calls
  into the effect system before the real impl exists.
- **Predecessor output threading**: predecessors are resolved by `TaskId.0` index
  into the `outputs` vec. The graph guarantees predecessors run before successors
  (Kahn's sort in A5), so the slot is always populated before it is read.

## Validation

```
cargo build -p valori-node          # ✅ 0 errors
cargo test -p valori-node runner    # ✅ 3 passed; 0 failed
cargo test -p valori-kernel -p valori-node  # ✅ all previously-passing tests pass
```

Runner tests:
- `empty_graph_succeeds` — 0-task graph transitions directly to Succeeded
- `run_graph_convenience_fn` — `run_graph()` completes within 50ms for empty graph
- `task_registry_default_has_all_kinds` — all 12 `TaskKind` variants have a registered impl

## Follow-ups

| Deferred to | What |
|---|---|
| Phase A8 | `ReceiptAssembler`: collect `ReceiptFragment`s from bus in topo order → final `Receipt`; expose via `/v1/proof` |
| Phase A9 | Replace `NoRaftKernelCapability` with real `DataPlaneState`-backed impl calling `raft.client_write()` |
| Phase A9 | Hook `run_graph()` into HTTP handlers to replace direct `engine.write().await` calls |
| Phase A9 | Remove dead `valori-storage/src/recovery.rs` (superseded by `valori-state::bootstrap`) |
