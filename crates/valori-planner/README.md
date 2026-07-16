# valori-planner

Operation lifecycle and execution planning for Valori.

The planner turns an `Operation` + `PlanningContext` into a deterministic
`ExecutionGraph` — a DAG of `TaskSpec`s that the runtime executes in topological
order. Graphs are cached at two layers: in-process (`ExecutionCache`) and
durable (`MetadataDb`).

## Crate boundary

`valori-planner` is **std-only**. It must not be added as a dependency of
`valori-kernel` or `valori-core`, which must remain `no_std`.

## Key types

| Type | Role |
|---|---|
| `Operation` | Immutable unit of user intent. `hash = BLAKE3(kind ‖ inputs ‖ policy)` |
| `OperationHash` | Content address of an `Operation` |
| `OperationInputs` | Planning parameters per kind (not actual data — vectors/text excluded). Variants: `Ingest`, `Search`, `GraphRag`, `MemoryUpsert`, `MemorySearch`, `Consolidate`, `Contradict`, `CommunityDetect`, `CommunitySearch`, `TreeBuild`, `TreeQuery`, `TreeHybrid`, `Snapshot`, `HealthCheck`, `Delete`, `BatchInsert` |
| `ExecutionPolicy` | Timeout, retry limit, resource budget |
| `PlannerFingerprint` | Stable digest of planner config: `BLAKE3(version ‖ routing ‖ flags ‖ schema_ver)` |
| `PlanningContext` | Fully-typed node context (capabilities, shard count, cluster epoch) |
| `PlanningContextHash` | `BLAKE3(bincode(context))` |
| `ExecutionGraph` | DAG of `TaskSpec`s with content-addressed `GraphHash` |
| `TaskSpec` | One task: kind + JSON inputs + shard target |
| `TaskEdge` | Directed dependency: `from` must complete before `to` |
| `GraphHash` | `BLAKE3(op_hash ‖ fp.hash ‖ ctx_hash ‖ topo_order)` |
| `ExecutionCache` | In-process `RwLock<HashMap>` cache, bounded by capacity |
| `ExecutionHandle` | `tokio::watch` channel wrapping an `ExecutionStatus` |
| `ExecutionRegistry` | Top-level cache + active-handle index |

## Cache key

```
CacheKey = (OperationHash, PlannerFingerprint.hash, PlanningContextHash)
```

All three must match for a cached graph to be reused (RFC-0001 §3.4 / invariant I-03).

## Planner implementations

| Type | Description |
|---|---|
| `NoOpPlanner` | Returns a single-task stub graph; for tests and health-check ops |
| `IngestPlanner` | Produces `Embed → InsertRecord → InsertNode → InsertEdge` (or without Embed) |

Use `plan_with_cache()` to run either planner with automatic two-layer cache lookup.

## TaskKind variants (Phase A13)

| Variant | Wired in `valori-node` | Notes |
|---|---|---|
| `Embed` | ✅ | HTTP embed call |
| `InsertRecord` | ✅ | Kernel write + WAL |
| `InsertNode` / `InsertEdge` | ✅ | Graph mutations |
| `Search` | ✅ | Vector kNN |
| `SnapshotArtifact` | ✅ standalone + cluster | Standalone: `engine.save_snapshot()`. Cluster: BLAKE3 hash of cloned shard state. |
| `GraphRag` | ✅ standalone + cluster | kNN + subgraph expansion |
| `MemorySearch` | ✅ standalone | Decay + rerank + filter (cluster: via `MemoryOps` trait, not planner) |
| `CommunityDetect` | ✅ standalone + cluster | Label propagation |
| `CommunitySearch` | ✅ standalone + cluster | Centroid ranking |
| `TreeBuild` | ✅ standalone + cluster | `TreeIndex::from_markdown` |
| `TreeQuery` | ✅ standalone + cluster | `TreeIndex::answer` |
| `TreeHybrid` | ✅ standalone + cluster | Tree + vector fusion |
| `SoftDeleteRecord` / `LlmComplete` / `HttpFetch` / `ReadIndex` / `ProofFragment` | stub | `NoOpTask` |

## Usage

```rust
use valori_planner::{
    Operation, OperationKind, OperationInputs, ExecutionPolicy,
    PlannerFingerprint, PlanningContext, CapabilitySet,
    IngestPlanner, plan_with_cache, ExecutionCache,
};

let op = Operation::new(
    OperationKind::Ingest,
    OperationInputs::Ingest {
        strategy: "tree".into(),
        collection: "default".into(),
        shard_id: 0,
        embed_enabled: true,
    },
    ExecutionPolicy::default(),
);

let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
let ctx = PlanningContext {
    capability_set: CapabilitySet { embed: true, llm: false, object_store: false, cluster: false, shard_count: 1 },
    schema_version: 1,
    shard_count: 1,
    cluster_epoch: 0,
    cluster_mode: false,
};
let cache = ExecutionCache::new(256);

let graph = plan_with_cache(&IngestPlanner, &op, &ctx, &fp, &cache, None).await?;
println!("tasks: {}", graph.tasks.len());
println!("graph_hash: {}", graph.graph_hash.to_hex());
```

## Invariants

- **I-03**: Equal inputs always produce equal `GraphHash`es — the planner is deterministic.
- **I-08**: One `TaskSpec` = one atomic transaction in the kernel.
- `OperationInputs` captures planning parameters only, never actual data.
  Two searches with the same `(k, collection, rerank)` share the same cached graph.
