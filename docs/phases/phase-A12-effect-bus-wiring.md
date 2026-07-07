# Phase A12 — Effect Bus Wiring

## Goal

Wire the `valori-effect` and `valori-planner` frameworks into the live `POST /v1/records` request handler (standalone path), so kernel writes flow through `EffectBus → KernelCapability → Engine` instead of calling the engine directly.

## Delivered

| File | Change |
|---|---|
| `crates/valori-effect/src/error.rs` | Added `EffectError::Capacity(String)` variant for pool-full propagation |
| `crates/valori-node/src/capabilities.rs` | Full rewrite: `EngineKernelCapability`, `RaftKernelCapability`, `NoRaftKernelCapability` updated to new `apply_command(body: &KernelCommandBody) -> Result<serde_json::Value, EffectError>` signature; `CapabilityRegistryBuilder` for standalone-mode registry construction |
| `crates/valori-node/src/server.rs` | `build_router_with_keys` builds `CapabilityRegistry` + `TaskRegistry` at startup and injects them as axum Extensions; `insert_record` handler replaced with effect-bus path via `run_graph_inline` |

### Effect path for `POST /v1/records` (standalone)

```
HTTP handler
  → resolve_collection (read lock, brief)
  → build ExecutionGraph (single InsertRecord task)
  → run_graph_inline
      → InsertRecordTask::run
          → EffectBus::dispatch(KernelWrite)
              → EngineKernelCapability::apply_command
                  → engine.write().await
                  → insert_record_from_f32_ns + reranker_insert
                  → returns {record_id, state_hash}
  → extract record_id from TaskOutput
  → emit receipt via receipt_bridge
```

## Findings

- `EngineError::Kernel(KernelError::CapacityExceeded)` (HTTP 507) was previously propagated directly; the effect chain converted it to a generic 500. Fixed by adding `EffectError::Capacity` and mapping it back to `KernelError::CapacityExceeded` in the handler.
- `capabilities.rs` already existed with old `apply_command(&str) -> Result<String>` signatures — updated in place rather than created new.
- The `sm` field on `RaftKernelCapability` is present for future use; suppressed with `#[allow(dead_code)]`.
- `cluster_server.rs` cluster path for `POST /v1/records` remains on the existing direct Raft path — `RaftKernelCapability` is implemented and correct but not yet wired into a cluster router Extension (follow-up: A13).

## Validation

```
cargo test -p valori-kernel -p valori-node
```

All test suites passed. Zero failures. Key regression confirmed passing:
- `test_http_insert_returns_507_when_full` — 507 Insufficient Storage still returned when pool is full

## Follow-ups

| Item | Phase |
|---|---|
| Wire `cluster_server.rs` `insert_record` through `RaftKernelCapability` + `run_graph_inline` | A13 |
| Wire `batch_insert` through effect bus (both paths) | A13 |
| Wire `delete_record` (soft + hard) through effect bus (both paths) | A13 |
| Add `EffectError::DimensionMismatch` to propagate 400 from dimension errors | A13 |
