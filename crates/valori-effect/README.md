# valori-effect

Effect system and EffectBus for Valori.

All side effects from task execution — kernel writes, receipt fragments, audit
entries, and metrics — flow through this crate. It defines the seven capability
traits, the Effect enum, and the EffectBus routing + deduplication layer.

## Crate boundary

`valori-effect` is **std-only**. It must not be added as a dependency of
`valori-kernel` or `valori-core`.

## Architecture

```
Task
 └─ ctx.bus.dispatch(Effect) ──→ EffectBus
      ├─ dedup by EffectId (BLAKE3(exec_id ‖ task_idx ‖ effect_idx))
      ├─ KernelWrite ──────────→ KernelCapability::apply_command
      ├─ Receipt ──────────────→ ProofCapability::append_fragment
      ├─ Audit ────────────────→ ProofCapability::append_fragment
      └─ Counter/Gauge ────────→ (log / metrics sink)
```

## Key types

| Type | Role |
|---|---|
| `Effect` | `{ id: EffectId, durability: EffectDurability, payload: EffectPayload }` |
| `EffectId` | `BLAKE3(execution_id ‖ task_index ‖ effect_index)` — stable across retries |
| `EffectDurability` | `Durable` (awaited before task Done) or `Ephemeral` (fire-and-forget) |
| `EffectPayload` | `KernelWrite`, `Receipt`, `Audit`, `Counter`, `Gauge` |
| `EffectBus` | Routes effects; deduplicates Durable by `EffectId` |
| `KernelCapability` | Apply kernel events; query state hash |
| `EmbedCapability` | Text → vector via configured provider |
| `LlmCapability` | LLM completion / entity extraction |
| `StorageCapability` | Object store read/write/list |
| `HttpCapability` | Outbound HTTP GET |
| `ProofCapability` | Append receipt fragments to BLAKE3 audit chain |
| `SchedulerCapability` | Schedule deferred operations |
| `CapabilityRegistry` | Holds all capabilities; optionals return `Err(CapabilityUnavailable)` |
| `Task` | Async trait: `run(inputs_json, predecessor_outputs, ctx) → TaskOutput` |
| `TaskContext` | Provides `bus`, `capabilities`, `budget` to a running task |
| `TaskOutput` | `{ json: Value, state_hash_after: String }` |

## Capability invariant

Tasks must not call capabilities directly. All side effects must flow through
`ctx.bus.dispatch(Effect)` so the EffectBus can enforce dedup, track receipts,
and respect resource budgets (RFC-0004 §5).

## Concrete tasks

| Task | Effects |
|---|---|
| `EmbedTask` | `Counter("embed_calls", n)` — ephemeral |
| `InsertRecordTask` | `KernelWrite` — durable; `Counter("records_inserted", 1)` — ephemeral |
| `InsertNodeTask` / `InsertEdgeTask` | `KernelWrite` — durable; `Counter` — ephemeral |
| `SearchTask` | `Receipt(ReceiptFragment{ mutated: false })` — durable; `Counter("searches", 1)` — ephemeral |
| `SnapshotArtifactTask` | `Counter("snapshots_saved", 1)` — ephemeral |
| `GraphRagTask` | `Counter("graphrag_queries", 1)` — ephemeral |
| `MemorySearchTask` | `Counter("memory_searches", 1)` — ephemeral |
| `CommunityDetectTask` | `Counter("community_detections", 1)` — ephemeral |
| `CommunitySearchTask` | `Counter("community_searches", 1)` — ephemeral |
| `TreeBuildTask` | `Counter("tree_builds", 1)` — ephemeral |
| `TreeQueryTask` | `Counter("tree_queries", 1)` — ephemeral |
| `TreeHybridTask` | `Counter("tree_hybrid_queries", 1)` — ephemeral |
| `NoOpTask` | `Counter("noop_runs", 1)` — ephemeral |

## Deduplication

`EffectId` is deterministic:
```
BLAKE3(execution_id.hi ‖ execution_id.lo ‖ task_topological_index ‖ effect_index_within_task)
```
A task retried with the same `ExecutionId` produces the same `EffectId`s. The bus
skips any Durable effect whose id was already dispatched, returning
`Err(EffectError::Duplicate)` which the executor silently ignores.

## Receipt system (Phase A8)

Every completed operation produces a `Receipt` — a self-describing,
offline-verifiable proof of what ran and what state changed.

| Type | Role |
|---|---|
| `Receipt` | Identity, execution contract, state transition, Merkle DAG, provenance |
| `ReceiptHash` | `BLAKE3(op_hash ‖ graph_hash ‖ state_before ‖ state_after ‖ sorted(parent_hashes) ‖ shard_id ‖ committed_height)` — `produced_at` excluded |
| `StateHash` | Opaque BLAKE3 hex of a kernel state snapshot |
| `ReceiptEnvelope` | Versioned outer wrapper (`version: u8`) |
| `ReceiptAssembler` | Collects `ReceiptFragment`s, sorts by `task_index`, assembles the final `Receipt` |
| `verify_receipt(&receipt)` | Offline verifier: recompute hash, check fragment chain, check outer consistency |
| `ReceiptStore` | In-process last-N cache (`Mutex<HashMap>`); evicts oldest; `insert/get/latest/list_ids` |

HTTP endpoints (both standalone and cluster):

- `GET /v1/proof/receipt` — most recently assembled receipt; `404` if none yet.
- `GET /v1/proof/receipt/:id` — receipt by `receipt_id`; `404` if not found.

Both are injected via `axum::Extension<Arc<ReceiptStore>>`.
