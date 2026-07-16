# Phase A5 — valori-planner

## Goal

Implement the Planner crate: the layer that converts an `Operation` (immutable,
content-addressed unit of user intent) + a `PlanningContext` (node capabilities,
shard count, cluster epoch) into a deterministic `ExecutionGraph` (DAG of
`TaskSpec`s). Cache graphs at two layers: in-process `ExecutionCache` (O(1)
`RwLock<HashMap>`) and durable `MetadataDb` (redb).

## Delivered

| File | Contents |
|---|---|
| `crates/valori-planner/Cargo.toml` | New crate; deps: valori-core (std), valori-metadata, blake3, bincode, serde, serde_json, thiserror, tracing, tokio (sync) |
| `crates/valori-planner/src/error.rs` | `PlannerError` with `From<MetadataError>` and `From<serde_json::Error>`; `PlannerResult<T>` alias |
| `crates/valori-planner/src/operation.rs` | `OperationHash`, `OperationId`, `OperationKind` (14 variants), `OperationInputs` (per-kind, planning params only), `ConsistencyLevel`, `ExecutionPolicy`, `ResourceBudget`, `Operation`, `compute_operation_hash()` |
| `crates/valori-planner/src/context.rs` | `PlannerFingerprint` with `compute()` and `zero()`, `PlanningContext` (fully-typed struct, no HashMap), `CapabilitySet`, `PlanningContextHash` with `compute()` |
| `crates/valori-planner/src/graph.rs` | `TaskId`, `TaskKind` (12 variants), `TaskSpec`, `TaskEdge`, `GraphHash`, `ExecutionGraph` with `build()` + Kahn's topological sort, `compute_graph_hash()` |
| `crates/valori-planner/src/registry.rs` | `ExecutionStatus`, `ExecutionHandle` (tokio watch channel), `TaskState`, `ExecutionContext`, `CacheKey`, `ExecutionCache` (bounded `RwLock<HashMap>`), `ExecutionRegistry` |
| `crates/valori-planner/src/planner.rs` | `Planner` trait, `plan_with_cache()` (in-process cache → DB cache → fresh plan), `NoOpPlanner`, `IngestPlanner` |
| `crates/valori-planner/src/lib.rs` | Module declarations + top-level re-exports |
| `crates/valori-planner/README.md` | Crate README with type table, cache key doc, usage example |
| `Cargo.toml` (workspace) | Added `valori-planner` to members, default-members, `[workspace.dependencies]` |

## Findings

- **`OperationInputs` excludes actual data**: captures only planning parameters
  (k, collection, shard_id, rerank, embed_enabled) — not vectors or text. This
  is intentional: two searches with the same config share the same cached
  `ExecutionGraph`, regardless of query vector. Documented in RFC-0001 §3.4.
- **No uuid dependency**: `OperationId` wraps `valori-core::ExecutionId` (128-bit
  custom struct with `new_random()` via time+address entropy). No uuid crate needed.
- **`PlannerCacheKey` in valori-metadata uses hex strings** (not `[u8; 32]`), so
  the planner converts via `to_hex()` / `hash_hex()` when calling `cache_put/get`.
- **`ExecutionId` Display** emits lowercase hex; used directly as `%op.id.0` in
  tracing spans (no extra `to_hex()` method on `ExecutionId`).

## Validation

```
cargo test -p valori-planner
# 16 passed; 0 failed

cargo test -p valori-kernel -p valori-node
# All passing (kernel: 21, node: integration suite all green)
```

Tests cover: hash determinism, `compute_operation_hash`, `PlanningContextHash`,
`PlannerFingerprint`, `ExecutionGraph::build` with Kahn's sort, `ExecutionHandle`
watch transitions, `ExecutionCache` insert/get, `ExecutionRegistry` retire,
`NoOpPlanner`, `IngestPlanner` (with + without embed), graph hash stability,
`plan_with_cache` in-process cache hit path.

## Follow-ups

| Deferred to | What |
|---|---|
| Phase A6 | `Effect` enum + `EffectBus` dispatch: the runtime consumes `TaskSpec`s from the graph and dispatches them as effects |
| Phase A7 | `ExecutionRegistry` wired into `valori-node`: executor task picks up handles and drives task execution |
| Phase A8 | `Receipt` + `ReceiptAssembler`: topological receipt chain built from completed task outputs |
| Phase A9 | Remove `valori-storage/src/recovery.rs` dead code; remove `valori-node` source files superseded by A3–A8 |
