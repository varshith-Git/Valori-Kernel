# RFC-0000: Canonical Glossary

**Status:** Frozen  
**Owner:** Core team (all crates)  
**Stability:** Stable — terms are frozen; additions require a PR touching all affected docs  
**Last reviewed:** 2026-07-08  
**Branch:** Node-scaleup  

Every Valori crate and document uses exactly the terms defined here.
Ambiguous terms (e.g. "snapshot") are resolved by which subsystem owns them — see **Owner**.
Every public type mentioned in any RFC must have an entry here first.

---

## Term reference

### Operation

| Field | Value |
|---|---|
| **Definition** | A single, immutable, content-addressed unit of user intent. An Operation captures *what* the user wants to do, completely and unambiguously, before any planning begins. It does not describe *how* it will be done — that is the Planner's job. |
| **Owner** | `valori-planner` — creates and validates; `valori-metadata` — persists. |
| **Lifetime** | Permanent. Once assigned an `OperationId`, an Operation is never mutated. If the intent changes, a new Operation is created. |
| **Invariant** | `OperationHash = BLAKE3(kind ‖ inputs ‖ policy)`. Two Operations with equal hashes are identical in meaning. The hash is computed before any planning and does not include `PlannerFingerprint` or `PlanningContextHash`. |

---

### OperationHash

| Field | Value |
|---|---|
| **Definition** | The BLAKE3 digest of an Operation's content fields: `kind ‖ inputs ‖ policy`. |
| **Owner** | `valori-planner` — computes; `valori-metadata` — stores as part of the planner cache key. |
| **Lifetime** | Immutable; derived from the Operation it describes. |
| **Invariant** | Equal hashes mean equal intent. The planner cache key is the triple `(OperationHash, PlannerFingerprint.hash, PlanningContextHash)` — all three must match to reuse a cached `ExecutionGraph`. |

---

### ExecutionGraph

| Field | Value |
|---|---|
| **Definition** | A deterministic DAG of Tasks produced by the Planner for a specific `(Operation, PlannerFingerprint, PlanningContext)` triple. Carries `graph_hash`, `planner_fingerprint`, `operation_hash`, and `planning_context_hash`. The logical graph (task topology + policy) is stored with a TTL controlled by `ExecutionRetentionPolicy`. The execution trace (timing, retries, resource usage) is ephemeral and discarded after the `Receipt` is assembled. |
| **Owner** | `valori-planner` — produces; `valori-metadata` — caches the logical graph; `valori-node` runtime — executes. |
| **Lifetime** | Logical graph: persisted according to `ExecutionRetentionPolicy` (default 30 days). Execution trace: ephemeral, discarded after `Receipt` is assembled. |
| **Invariant** | `graph_hash = BLAKE3(operation_hash ‖ planner_fingerprint.hash ‖ planning_context_hash ‖ topological_task_order)`. The same triple always produces the same graph when replayed with the same Planner version. Edges may carry an optional `condition` field reserved for future speculative execution; today conditions are always `None`. |

---

### Task

| Field | Value |
|---|---|
| **Definition** | The smallest schedulable unit of work in an `ExecutionGraph`. A Task receives typed inputs, performs exactly one logical operation (e.g. embed a chunk, write a `KernelCommand`, query a namespace), and returns `Vec<Effect>`. |
| **Owner** | `valori-node` runtime — schedules and executes. |
| **Lifetime** | Ephemeral. A Task exists only during execution. Its outputs (`Effect`s) may be durable. |
| **Invariant** | A Task never accesses `KernelState` directly. All writes to the kernel go via `KernelCommand` wrapped in an `Effect`. All `KernelCommand`s from one Task target the same shard (one Task = one atomic transaction). |

---

### Effect

| Field | Value |
|---|---|
| **Definition** | A typed, side-effecting output from a Task. Every interaction between a Task and an external subsystem (kernel, storage, network, metrics) is expressed as an Effect routed through the `EffectBus`. Each Effect carries an `EffectId` (ULID), `source_task`, `attempt`, and `EffectDurability`. |
| **Owner** | `valori-node` runtime — emits; `EffectBus` — routes and deduplicates. |
| **Lifetime** | `Durable` Effects are acknowledged by their subsystem before the Task is marked complete. `Ephemeral` Effects are fire-and-forget. |
| **Invariant** | `EffectId` is globally unique per emission (ULID monotonicity). The `EffectBus` deduplicates on `EffectId` before dispatch — a retried Task emitting the same logical Effect twice is safe. |

---

### EffectDurability

| Field | Value |
|---|---|
| **Definition** | A tag on every Effect indicating whether its acknowledgement must precede task completion. `Durable` variants: `KernelCommand`, `ReceiptFragment`, `SnapshotArtifact`, `StorageWrite`. `Ephemeral` variants: `Metric`, `Notification`. |
| **Owner** | Defined alongside `Effect` in `valori-node`. |
| **Lifetime** | Static per effect variant — determined at compile time, not at runtime. |
| **Invariant** | A Task is not marked complete until all `Durable` Effects it emitted have been acknowledged. `Ephemeral` Effects may be dropped under backpressure without affecting correctness or the audit chain. |

---

### Command (KernelCommand)

| Field | Value |
|---|---|
| **Definition** | A typed instruction from the runtime to the Kernel, wrapping one or more `KernelEvent`s that should be applied atomically. A `CommandId` (UUID) is carried for exactly-once deduplication in the Raft state machine and standalone engine. |
| **Owner** | `valori-node` — constructs; `valori-consensus` (cluster) or standalone engine — applies to `KernelState`. |
| **Lifetime** | Ephemeral after application. The resulting `KernelEvent`s are durable in the audit log. |
| **Invariant** | All `KernelCommand`s from one Task target the same shard. The state machine checks and records `CommandId` before applying — duplicate IDs are silently dropped, never double-applied. |

---

### KernelEvent

| Field | Value |
|---|---|
| **Definition** | The atomic mutation record appended to the BLAKE3-chained audit log after a `KernelCommand` is successfully applied to `KernelState`. `KernelEvent`s are the source of truth for replay. |
| **Owner** | `valori-kernel` — emits; `valori-storage` — persists in the event log. |
| **Lifetime** | Permanent. `KernelEvent`s are append-only and never deleted or mutated. |
| **Invariant** | Sequence is always `DEDUP CHECK → KERNEL APPLY → AUDIT WRITE`. A `KernelEvent` is written to the log only after the kernel apply succeeds and only if the command was not a duplicate. No event is written for rejected or duplicate commands. |

---

### KernelABI

| Field | Value |
|---|---|
| **Definition** | The versioned interface of the Kernel: `semantic_version + event_schema_hash + state_schema_hash`. The ABI changes only when the wire format of `KernelEvent`s or the serialized `KernelState` changes in a backward-incompatible way. Internal refactors that do not change the wire format do not change the ABI. |
| **Owner** | `valori-kernel` — defines and exports; `valori-planner` and receipt consumers — read for compatibility checks. |
| **Lifetime** | Persisted in every `Receipt`. Consumers compare against the ABI version they were compiled against. |
| **Invariant** | A `Receipt` is valid only for the `KernelABI` version under which it was produced. Replaying events from a `Receipt` against a different ABI version requires an explicit migration step declared in `COMPATIBILITY.md`. |

---

### Receipt

| Field | Value |
|---|---|
| **Definition** | A versioned, verifiable proof artifact produced after an Operation completes. Carries: `KernelABI`, `PlannerFingerprint`, `CapabilitySet`, `state_hash_before`, `state_hash_after`, `parent_receipts: Vec<ReceiptHash>` (Merkle DAG, single-parent today), `operation_hash`, `graph_hash`. For read-only operations, `state_hash_before == state_hash_after` — no `ReadEvent` is needed; the state hash is the proof anchor. |
| **Owner** | `ReceiptAssembler` (owned by `EffectBus`) — assembles from `ReceiptFragment` Effects; `valori-metadata` — archives. |
| **Lifetime** | Permanent. Receipts are never deleted; they form the verifiable audit backbone. |
| **Invariant** | `ReceiptFragment`s are sorted by topological order in the `ExecutionGraph`, not by arrival or completion order. Two independent replays of the same `ExecutionGraph` produce byte-identical receipts. |

---

### KernelSnapshot

| Field | Value |
|---|---|
| **Definition** | A point-in-time binary serialization of `KernelState` in the current snapshot format (V6). Used for fast node restart, avoiding full WAL replay. |
| **Owner** | `valori-kernel` — encodes/decodes; `valori-storage` — persists and rotates; `valori-node` — triggers save/restore. |
| **Lifetime** | Until superseded by a newer snapshot. Old snapshots are pruned by `ObjectStoreBackend` according to `VALORI_OBJECT_STORE_KEEP`. |
| **Invariant** | `KernelState` is fully reconstructable from `KernelSnapshot + KernelEvent`s that follow it. A snapshot accelerates restart but does not replace the event log. |

---

### ExecutionSnapshot

| Field | Value |
|---|---|
| **Definition** | A point-in-time capture of runtime control-plane state: active `ExecutionGraph`s, in-flight `Effect`s, `EffectBus` queue depths. Used only for debugging and hot-standby transfer — never for audit. |
| **Owner** | `valori-node` runtime. |
| **Lifetime** | Ephemeral. Discarded after use. |
| **Invariant** | An `ExecutionSnapshot` is never used as a source of truth for `KernelState`. Its contents are informational only and carry no cryptographic proof. |

---

### KnowledgeGraph

| Field | Value |
|---|---|
| **Definition** | The directed graph of `GraphNode`s and `GraphEdge`s stored inside `KernelState`. Distinct from `ExecutionGraph` (which is a task DAG used internally by the runtime). The `KnowledgeGraph` is the user-facing semantic graph: documents, chunks, entities, relationships. |
| **Owner** | `valori-kernel` — stores and mutates; `valori-node` — queries via `/v1/graphrag` and `/graph/subgraph`. |
| **Lifetime** | Persistent. Mutated by `KernelEvent`s. |
| **Invariant** | The `KnowledgeGraph` is never mutated from outside the kernel. All mutations go through `KernelCommand → KernelEvent`. |

---

### KernelState

| Field | Value |
|---|---|
| **Definition** | The in-memory, fully-deterministic state of the vector store, knowledge graph, and namespace registry inside one shard. Reconstructable solely from a `KernelSnapshot` plus subsequent `KernelEvent`s. |
| **Owner** | `valori-kernel`. |
| **Lifetime** | Lives in memory for the duration of a node process. Persisted as `KernelSnapshot`. |
| **Invariant** | `KernelState` is the single source of truth for all reads. No read path may bypass it to read raw storage files directly. |

---

### ClusterState

| Field | Value |
|---|---|
| **Definition** | The union of `KernelState` across all shards on all nodes in a cluster, plus the Raft log (committed index, term, leader identity). |
| **Owner** | `valori-consensus` — manages Raft; `valori-node` — exposes summary via `/v1/cluster/*`. |
| **Lifetime** | Continuous; updated on every Raft commit. |
| **Invariant** | Every committed `KernelEvent` in the Raft log is applied identically on every live replica. Divergence is detected by the `state_hash_match` gauge. |

---

### PlannerFingerprint

| Field | Value |
|---|---|
| **Definition** | A stable digest of the Planner's behavioral configuration at a specific deployment: `BLAKE3(version ‖ routing_config ‖ feature_flags ‖ metadata_schema_version)`. Changes on any behavioral change to the Planner, regardless of whether the semantic version bumps. |
| **Owner** | `valori-planner` — computes and exports. |
| **Lifetime** | Immutable once computed for a given Planner deployment. Stored in every `Receipt` so receipts are self-describing. |
| **Invariant** | A cached `ExecutionGraph` is reusable only when `PlannerFingerprint.hash` matches exactly. Changing routing logic or feature flags requires a new fingerprint and invalidates the planner cache for prior graphs. |

---

### PlanningContextHash

| Field | Value |
|---|---|
| **Definition** | The BLAKE3 digest of the `PlanningContext` struct: the fully-typed inputs to the planner beyond the Operation itself (system prompt snapshot, active capability set, resource budget, schema version). Must be a typed struct — no `HashMap<String, serde_json::Value>`. |
| **Owner** | `valori-planner` — computes. |
| **Lifetime** | Ephemeral as an input; immutable once incorporated into `graph_hash`. |
| **Invariant** | `PlanningContext` must be a deterministically serializable typed struct. Non-deterministic fields (timestamps, random IDs) must be excluded. |

---

### Collection

| Field | Value |
|---|---|
| **Definition** | A named, isolated namespace of records within a project. Mapped to a `NamespaceId` (u16) at the kernel level. The collection name is stored in the control-plane manifest; the `NamespaceId` is the internal routing key. |
| **Owner** | `valori-metadata` — stores name→`NamespaceId` mapping; `valori-kernel` — stores records per namespace. |
| **Lifetime** | Persistent until explicitly deleted via the collection drop API. |
| **Invariant** | `shard_for_namespace(ns_id, shard_count) = ns_id % shard_count`. A collection is never split across shards. |

---

### Shard

| Field | Value |
|---|---|
| **Definition** | An independent `KernelState` instance, backed by its own Raft group (cluster) or logical partition (standalone). Each shard owns its own BLAKE3-chained audit log (`events-shardN.log`). |
| **Owner** | `valori-node` — hosts; `valori-consensus` — replicates per shard. |
| **Lifetime** | Matches the node process lifetime. |
| **Invariant** | `shard_for_namespace(ns_id, shard_count) = ns_id % shard_count`. All collections whose `NamespaceId % shard_count == k` live on shard `k`. All Commands from one Task land on one shard. |

---

## Term relationships

```
Operation ──(OperationHash)─────────────────────────────────────────┐
                                                                     │
PlannerFingerprint + PlanningContextHash ──────────────────────────►│
                                                                     ▼
                                                           Planner → ExecutionGraph (graph_hash)
                                                                     │
                                                             Tasks (DAG nodes)
                                                                     │
                                                             Effects ──► EffectBus
                                                                            │
                                                    ┌───────────────────────┤──────────────────┐
                                                    ▼                       ▼                  ▼
                                             KernelCommand          ReceiptFragment       Metric/Notification
                                                    │                       │
                                       EventCommitter/Raft          ReceiptAssembler
                                                    │                       │
                                             KernelEvent               Receipt
                                                    │                (permanent, Merkle DAG)
                                             KernelState
                                           (+ audit log)
```

---

*This document is referenced by all RFCs and all crate-level documentation.
If a term appears in code or docs that is not defined here, add it to this file before merging the code that introduces it.*
