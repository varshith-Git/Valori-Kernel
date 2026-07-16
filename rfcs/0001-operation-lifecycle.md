# RFC-0001: Operation & Execution Lifecycle

**Status:** Draft  
**Owner:** valori-planner  
**Stability:** Alpha — lifecycle types are being finalized; breaking changes expected before freeze  
**Last reviewed:** 2026-07-08  
**Depends on:** [`rfcs/0000-glossary.md`](0000-glossary.md)  
**Implements:** Phase A5 (`valori-planner`), Phase A7 (`ExecutionGraph` + `ExecutionRegistry`)

---

## 1. Motivation

The current `valori-node` has no explicit model of *why* an operation was requested,
*how* it was planned, or *what* happened during execution. Each HTTP handler encodes
its own ad-hoc flow: embed, write, respond. This makes it impossible to:

- Prove that a response was produced by a specific planner version.
- Cache or replay a planned computation without re-planning.
- Track resource usage per logical operation across multiple tasks.
- Retire a planner version while keeping old receipts verifiable.

This RFC defines the lifecycle of an `Operation` from creation through planning,
execution, and archival.

---

## 2. Lifecycle overview

```
User request
     │
     ▼
[1] Operation creation
     │  OperationId (UUID), OperationHash = BLAKE3(kind ‖ inputs ‖ policy)
     ▼
[2] Planner cache check
     │  key = (OperationHash, PlannerFingerprint.hash, PlanningContextHash)
     │  HIT  → load ExecutionGraph from ExecutionCache / ExecutionHistory
     │  MISS → plan() → new ExecutionGraph
     ▼
[3] ExecutionGraph registration
     │  ExecutionRegistry: write to ExecutionCache (in-process) + ExecutionHistory (durable)
     │  ExecutionHandle returned to caller for status polling / cancellation
     ▼
[4] Runtime execution
     │  Tasks scheduled in topological order
     │  Each Task emits Effects → EffectBus routes them
     │  Durable Effects acknowledged before Task marked complete
     ▼
[5] Receipt assembly
     │  ReceiptAssembler (owned by EffectBus) sorts ReceiptFragments by topological order
     │  Receipt sealed: KernelABI + PlannerFingerprint + CapabilitySet + state_hash_before/after
     ▼
[6] Archival
     │  Receipt written to ExecutionHistory in valori-metadata
     │  ExecutionGraph logical record retained per ExecutionRetentionPolicy
     │  Execution trace discarded
     ▼
[7] Response to caller
     │  Includes receipt hash for verification
```

---

## 3. Types

### 3.1 Operation

```rust
pub struct Operation {
    pub id: OperationId,           // UUID, assigned at creation
    pub kind: OperationKind,       // typed enum, not a string
    pub inputs: OperationInputs,   // typed union matching kind
    pub policy: ExecutionPolicy,
    pub hash: OperationHash,       // BLAKE3(kind ‖ inputs ‖ policy), computed at creation
    pub created_at: u64,           // Unix seconds; excluded from hash computation
}

pub struct OperationId(uuid::Uuid);
pub struct OperationHash([u8; 32]);

pub enum OperationKind {
    Ingest,
    Search,
    GraphRag,
    MemoryUpsert,
    MemorySearch,
    Consolidate,
    Contradict,
    CommunityDetect,
    CommunitySearch,
    // ... extensible; adding a variant is backward-compatible
}
```

### 3.2 ExecutionPolicy

```rust
pub struct ExecutionPolicy {
    pub timeout_secs: Option<u32>,
    pub retry_limit: u8,           // 0 = no retries
    pub consistency: Consistency,  // Local | Linearizable
    pub resource_budget: ResourceBudget,
}

pub struct ResourceBudget {
    pub max_kernel_writes: u32,
    pub max_embed_calls: u32,
    pub max_llm_tokens: u32,
}
```

### 3.3 PlanningContext

```rust
pub struct PlanningContext {
    pub capability_set: CapabilitySet,
    pub schema_version: u32,         // valori-metadata schema version
    pub shard_topology: ShardTopology,
    // All fields must be deterministically serializable.
    // No HashMap, no timestamps, no random IDs.
}

pub struct PlanningContextHash([u8; 32]);  // BLAKE3(bincode::encode(context))
```

### 3.4 PlannerFingerprint

```rust
pub struct PlannerFingerprint {
    pub version: semver::Version,
    pub routing_config_hash: [u8; 32],
    pub feature_flags_hash: [u8; 32],
    pub metadata_schema_version: u32,
    pub hash: [u8; 32],  // BLAKE3(version ‖ routing_config_hash ‖ feature_flags_hash ‖ metadata_schema_version)
}
```

The `hash` field is the cache key component and the value embedded in `Receipt`s.

### 3.5 ExecutionGraph

```rust
pub struct ExecutionGraph {
    pub id: ExecutionId,
    pub operation_hash: OperationHash,
    pub planner_fingerprint: PlannerFingerprint,
    pub planning_context_hash: PlanningContextHash,
    pub graph_hash: GraphHash,      // BLAKE3(operation_hash ‖ fp.hash ‖ ctx_hash ‖ topo_order)
    pub tasks: Vec<TaskSpec>,
    pub edges: Vec<TaskEdge>,
    pub retention: ExecutionRetentionPolicy,
}

pub struct TaskEdge {
    pub from: TaskId,
    pub to: TaskId,
    pub condition: Option<Condition>,  // None today; reserved for speculative execution
}

pub struct ExecutionRetentionPolicy {
    pub logical_graph_ttl_secs: u64,  // default: 30 days
}
```

### 3.6 ExecutionHandle

```rust
pub struct ExecutionHandle {
    pub execution_id: ExecutionId,
    pub graph_hash: GraphHash,
    pub status_rx: tokio::sync::watch::Receiver<ExecutionStatus>,
    pub cancel_tx: tokio::sync::oneshot::Sender<()>,
}

pub enum ExecutionStatus {
    Pending,
    Running { completed_tasks: usize, total_tasks: usize },
    Complete { receipt_hash: ReceiptHash },
    Failed { error: String },
    Cancelled,
}
```

---

## 4. ExecutionRegistry

The `ExecutionRegistry` is the single authoritative store for execution state.
It is split into three layers with different scopes:

### 4.1 ExecutionCache (process-local)

In-process `DashMap<ExecutionId, Arc<ExecutionContext>>`. Fast path for status
polling and handle lookup. Entries are evicted when the execution completes and
the receipt is archived. Not persisted — rebuilt on restart from `ExecutionHistory`.

### 4.2 ExecutionHistory (durable)

Persisted in `valori-metadata`. Stores:
- Completed `ExecutionGraph` (logical, not trace) per `ExecutionRetentionPolicy`.
- Final `ExecutionStatus` per `ExecutionId`.
- `Receipt` reference (receipt hash + archive path).

The planner cache lookup reads from `ExecutionHistory` when the `ExecutionCache`
misses (e.g. after a restart).

### 4.3 ExecutionAnalytics (optional, time-series)

Tracks resource usage (kernel writes, embed calls, LLM tokens, latency) per
`OperationKind` and `PlannerFingerprint`. Not on the critical path. Written
asynchronously via an `Ephemeral` Effect. May be omitted entirely in minimal deployments.

---

## 5. ExecutionContext

`ExecutionContext` is the runtime view of a single in-flight execution. It does not
own `ReceiptAssembler` — that is owned by the `EffectBus` (see RFC-0004).

```rust
pub struct ExecutionContext {
    pub handle: ExecutionHandle,
    pub graph: Arc<ExecutionGraph>,
    pub task_states: DashMap<TaskId, TaskState>,
    pub started_at: Instant,         // not hashed; for timeout enforcement only
}

pub enum TaskState {
    Pending,
    Running,
    Complete,
    Failed(String),
}
```

---

## 6. Planner cache

The Planner checks `ExecutionHistory` before planning. Cache hit returns an existing
`ExecutionGraph`; the runtime re-uses it without re-planning.

```
Cache key: (OperationHash, PlannerFingerprint.hash, PlanningContextHash)
Cache hit condition: all three components match exactly.
Cache miss action: call Planner::plan(operation, context) → new ExecutionGraph.
```

On a cache hit, a new `ExecutionId` is assigned (the execution is new even if the graph
is reused). The `graph_hash` is the same as the cached graph's.

---

## 7. Lifecycle invariants

| # | Invariant |
|---|---|
| I-01 | An `Operation` is never mutated after creation (INVARIANTS.md I-01). |
| I-02 | `OperationHash` is computed from `kind ‖ inputs ‖ policy` only (I-02). |
| I-03 | The Planner produces the same `ExecutionGraph` for the same triple (I-03). |
| I-13 | A Task is not marked complete until all `Durable` Effects are acknowledged (I-13). |

---

## 8. Open questions

- **Cancellation**: `ExecutionHandle` carries a `cancel_tx`. Cancellation must be
  propagated to in-flight tasks and result in a `Cancelled` status in `ExecutionHistory`.
  The exact cancellation protocol (best-effort vs transactional rollback) is deferred to
  the runtime implementation phase (A7).

- **Speculative execution**: `TaskEdge.condition` is reserved. The condition evaluation
  model (static predicate vs. dynamic result-based branching) is deferred.

- **Multi-operation graphs**: Today one `Operation` = one `ExecutionGraph`. Future work
  may allow an `Operation` to delegate to sub-operations with their own receipts, forming
  a receipt tree. The `parent_receipts` field in `Receipt` accommodates this without
  requiring a schema change.

---

## 9. Files to create/modify

| File | Change |
|---|---|
| `crates/valori-planner/src/operation.rs` | `Operation`, `OperationId`, `OperationHash`, `OperationKind`, `OperationInputs`, `ExecutionPolicy`, `ResourceBudget` |
| `crates/valori-planner/src/context.rs` | `PlanningContext`, `PlanningContextHash`, `PlannerFingerprint` |
| `crates/valori-planner/src/graph.rs` | `ExecutionGraph`, `TaskSpec`, `TaskEdge`, `GraphHash`, `ExecutionRetentionPolicy` |
| `crates/valori-planner/src/registry.rs` | `ExecutionRegistry`, `ExecutionCache`, `ExecutionHandle`, `ExecutionStatus`, `ExecutionContext`, `TaskState` |
| `crates/valori-metadata/src/history.rs` | `ExecutionHistory` persistence (created in Phase A4) |
| `crates/valori-metadata/src/analytics.rs` | `ExecutionAnalytics` (optional; created in Phase A4) |
