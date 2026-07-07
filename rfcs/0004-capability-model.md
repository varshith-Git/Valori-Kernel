# RFC-0004: Capability & Task Model

**Status:** Draft  
**Depends on:** [`rfcs/0000-glossary.md`](0000-glossary.md), [`rfcs/0001-operation-lifecycle.md`](0001-operation-lifecycle.md), [`rfcs/0002-kernel-contract.md`](0002-kernel-contract.md), [`rfcs/0003-receipt-spec.md`](0003-receipt-spec.md)  
**Implements:** Phase A6 (Effect system + EffectBus)

---

## 1. Motivation

Tasks currently interact with subsystems through ad-hoc means: direct `engine.write()`,
direct HTTP calls, direct metric emissions. This creates several problems:

- No deduplication across retried tasks.
- No backpressure signal between the runtime and subsystems.
- Receipt assembly requires knowing which tasks wrote what — impossible without a
  unified communication channel.
- The capability model (what a node is allowed to do) is implicit in env vars and
  compilation, not in a queryable runtime type.

This RFC defines:

1. The `Capability` trait and capability registry.
2. The `Effect` type and its variants.
3. The `EffectBus` — the single routing layer between tasks and subsystems.
4. The task contract: inputs, outputs, lifecycle.

---

## 2. Capabilities

A capability is a named, runtime-queryable authorization to interact with a subsystem.
Capabilities are checked at task dispatch time, not at compile time.

```rust
pub trait Capability: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn is_available(&self) -> bool;
}

/// All capabilities available to the node.
pub struct CapabilityRegistry {
    pub kernel: Arc<dyn KernelCapability>,
    pub embed: Option<Arc<dyn EmbedCapability>>,
    pub llm: Option<Arc<dyn LlmCapability>>,
    pub storage: Arc<dyn StorageCapability>,
    pub http: Arc<dyn HttpCapability>,
    pub proof: Arc<dyn ProofCapability>,
    pub scheduler: Arc<dyn SchedulerCapability>,
}
```

### Capability traits

```rust
pub trait KernelCapability: Capability {
    fn shard_count(&self) -> u8;
    fn submit_command(&self, cmd: KernelCommand) -> impl Future<Output = Result<(), KernelError>>;
    fn state_hash(&self, shard_id: ShardId) -> StateHash;
}

pub trait EmbedCapability: Capability {
    fn embed(&self, texts: Vec<String>) -> impl Future<Output = Result<Vec<Vec<f32>>, EmbedError>>;
    fn model_name(&self) -> &str;
}

pub trait LlmCapability: Capability {
    fn complete(&self, prompt: String, model: &str) -> impl Future<Output = Result<String, LlmError>>;
}

pub trait StorageCapability: Capability {
    fn write_object(&self, key: &str, bytes: Bytes) -> impl Future<Output = Result<(), StorageError>>;
    fn read_object(&self, key: &str) -> impl Future<Output = Result<Bytes, StorageError>>;
}

pub trait HttpCapability: Capability {
    fn get(&self, url: &str) -> impl Future<Output = Result<Bytes, HttpError>>;
}

pub trait ProofCapability: Capability {
    fn submit_fragment(&self, fragment: ReceiptFragment) -> impl Future<Output = ()>;
    fn assemble(&self, execution_id: ExecutionId) -> impl Future<Output = Option<Receipt>>;
}

pub trait SchedulerCapability: Capability {
    fn spawn_task(&self, spec: TaskSpec, inputs: TaskInputs) -> ExecutionHandle;
}
```

The `CapabilityRegistry` is constructed at node startup from env vars and config.
A task that requires `EmbedCapability` and receives `None` must return an error at
plan time (the Planner checks capability availability before building the graph).

---

## 3. Effect variants

```rust
pub enum Effect {
    /// Writes to the kernel on a specific shard.
    KernelCommand {
        effect_id: EffectId,
        command: KernelCommand,
        durability: EffectDurability::Durable,
    },
    /// Contributes a fragment to receipt assembly.
    ReceiptFragment {
        effect_id: EffectId,
        fragment: ReceiptFragment,
        durability: EffectDurability::Durable,
    },
    /// Stores a snapshot artifact in the object store.
    SnapshotArtifact {
        effect_id: EffectId,
        shard_id: ShardId,
        bytes: Bytes,
        durability: EffectDurability::Durable,
    },
    /// Writes arbitrary bytes to durable storage.
    StorageWrite {
        effect_id: EffectId,
        key: String,
        bytes: Bytes,
        durability: EffectDurability::Durable,
    },
    /// Emits a metric sample. Fire-and-forget.
    Metric {
        effect_id: EffectId,
        name: &'static str,
        value: f64,
        labels: Vec<(&'static str, String)>,
        durability: EffectDurability::Ephemeral,
    },
    /// Broadcasts a notification to subscribers. Fire-and-forget.
    Notification {
        effect_id: EffectId,
        topic: String,
        payload: Bytes,
        durability: EffectDurability::Ephemeral,
    },
}

pub enum EffectDurability { Durable, Ephemeral }

pub struct EffectId(ulid::Ulid);
```

`EffectId` is a ULID — monotonically increasing within a process, globally unique.
ULIDs are used (not UUIDs) so the `EffectBus` can log effects in time order without
a secondary sort.

---

## 4. EffectBus

The `EffectBus` is the single routing layer between tasks and subsystems.
It is constructed at node startup and passed to every task via `TaskContext`.

```rust
pub struct EffectBus {
    seen_effects: DashSet<EffectId>,    // deduplication
    receipt_assembler: Arc<ReceiptAssembler>,
    capabilities: Arc<CapabilityRegistry>,
    metrics_tx: mpsc::Sender<MetricSample>,
    notification_tx: broadcast::Sender<Notification>,
}

impl EffectBus {
    /// Route an effect to its subsystem. Durable effects block until acknowledged.
    pub async fn dispatch(&self, effect: Effect) -> Result<(), EffectError> {
        if self.seen_effects.contains(&effect.effect_id()) {
            return Ok(());  // idempotent dedup
        }
        self.seen_effects.insert(effect.effect_id());
        match effect {
            Effect::KernelCommand { command, .. } =>
                self.capabilities.kernel.submit_command(command).await?,
            Effect::ReceiptFragment { fragment, .. } =>
                self.receipt_assembler.submit(fragment),
            Effect::SnapshotArtifact { shard_id, bytes, .. } =>
                self.capabilities.storage.write_object(&snapshot_key(shard_id), bytes).await?,
            Effect::StorageWrite { key, bytes, .. } =>
                self.capabilities.storage.write_object(&key, bytes).await?,
            Effect::Metric { name, value, labels, .. } =>
                let _ = self.metrics_tx.try_send(MetricSample { name, value, labels });
            Effect::Notification { topic, payload, .. } =>
                let _ = self.notification_tx.send(Notification { topic, payload });
        }
        Ok(())
    }
}
```

`dispatch` returns `Ok` for `Ephemeral` effects even if the channel is full (dropped
under backpressure). For `Durable` effects it propagates errors — the task runner
will retry the task according to `ExecutionPolicy.retry_limit`.

---

## 5. Task contract

```rust
/// Implemented by every concrete task type.
#[async_trait]
pub trait Task: Send + 'static {
    type Inputs: Send + 'static;
    type Outputs: Send + 'static;

    async fn run(self, inputs: Self::Inputs, ctx: &TaskContext) -> Result<Self::Outputs, TaskError>;
}

pub struct TaskContext {
    pub task_id: TaskId,
    pub execution_id: ExecutionId,
    pub topological_index: u32,
    pub capabilities: Arc<CapabilityRegistry>,
    pub effect_bus: Arc<EffectBus>,
    pub budget: ResourceBudget,
}
```

A task emits effects by calling `ctx.effect_bus.dispatch(effect).await`. It does not
call `capabilities.kernel.submit_command()` directly — all kernel writes go through
`Effect::KernelCommand` so the `EffectBus` can deduplicate and track them for receipt
assembly.

### Task lifecycle

```
1. TaskRunner receives TaskSpec + inputs from ExecutionGraph.
2. TaskRunner constructs TaskContext.
3. TaskRunner calls task.run(inputs, &ctx).
4. Task emits Effects via ctx.effect_bus.dispatch().
5. All Durable Effects are awaited before run() returns.
6. TaskRunner records task as Complete in ExecutionContext.
7. If run() returns Err: TaskRunner checks retry_limit.
   - If retries remain: re-run with same TaskContext (EffectId dedup prevents double-writes).
   - If no retries: mark execution as Failed, cancel sibling tasks.
```

---

## 6. Task examples

### EmbedTask

```
Inputs:  Vec<String> (text chunks)
Outputs: Vec<Vec<f32>> (embeddings)
Effects: [Metric("embed_calls", 1.0)]
```

Pure computation — no kernel writes, no receipt fragments.

### InsertRecordTask

```
Inputs:  (embedding: Vec<f32>, metadata: Option<Vec<u8>>, namespace_id: NamespaceId, shard_id: ShardId)
Outputs: RecordId
Effects: [
    KernelCommand(InsertRecord { ... }),     // Durable
    ReceiptFragment { state_hash_before, state_hash_after, ... },  // Durable
    Metric("records_inserted", 1.0),         // Ephemeral
]
```

### SearchTask

```
Inputs:  (query: Vec<f32>, k: usize, namespace_id: NamespaceId, shard_id: ShardId)
Outputs: Vec<SearchHit>
Effects: [
    ReceiptFragment { state_hash_before == state_hash_after, ... },  // read-only proof
    Metric("searches", 1.0),
]
```

---

## 7. Capability checking at plan time

The Planner checks capability availability before building an `ExecutionGraph`.
If a required capability is absent (`Option<Arc<dyn Cap>> = None`), the Planner
returns `PlanError::CapabilityUnavailable` instead of producing a graph.

This means the HTTP handler gets a `400 Bad Request` immediately rather than
letting a task fail at runtime.

```rust
fn plan(&self, op: &Operation, ctx: &PlanningContext) -> Result<ExecutionGraph, PlanError> {
    if matches!(op.kind, OperationKind::Ingest) && ctx.capability_set.embed == false {
        return Err(PlanError::CapabilityUnavailable("embed"));
    }
    // ...
}
```

---

## 8. Files to create/modify

| File | Change |
|---|---|
| `crates/valori-node/src/capability.rs` | `Capability` trait, `KernelCapability`, `EmbedCapability`, `LlmCapability`, `StorageCapability`, `HttpCapability`, `ProofCapability`, `SchedulerCapability`, `CapabilityRegistry` |
| `crates/valori-node/src/effect.rs` | `Effect` enum, `EffectId`, `EffectDurability` |
| `crates/valori-node/src/effect_bus.rs` | `EffectBus`, dispatch logic, dedup |
| `crates/valori-node/src/task.rs` | `Task` trait, `TaskContext`, task lifecycle |
| `crates/valori-node/src/tasks/embed.rs` | `EmbedTask` |
| `crates/valori-node/src/tasks/insert_record.rs` | `InsertRecordTask` |
| `crates/valori-node/src/tasks/search.rs` | `SearchTask` |
| *(additional task files per operation kind)* | |
