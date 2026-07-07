// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Task trait and TaskContext.
//!
//! The executor drives task lifecycle:
//! 1. Construct `TaskContext` from `TaskSpec` + `ExecutionHandle`.
//! 2. Call `task.run(inputs, &ctx)`.
//! 3. Await all Durable effects dispatched through `ctx.bus`.
//! 4. Mark task Done in `ExecutionContext`.
//! 5. On failure: retry up to `policy.retry_limit` times, then fail the execution.
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;

use valori_core::id::ExecutionId;
use valori_planner::graph::TaskId;
use valori_planner::operation::ResourceBudget;

use crate::bus::EffectBus;
use crate::capability::CapabilityRegistry;
use crate::error::EffectResult;

// ── TaskOutput ────────────────────────────────────────────────────────────────

/// The result produced by a task and consumed by successor tasks.
///
/// Stored as JSON so tasks remain decoupled from each other's concrete types.
/// The executor deserializes `TaskOutput.json` when threading outputs from
/// predecessor to successor tasks.
#[derive(Clone, Debug)]
pub struct TaskOutput {
    /// JSON-encoded output value. `null` for tasks that produce no output.
    pub json: Value,
    /// BLAKE3 hex of the state hash after this task completed.
    pub state_hash_after: String,
}

impl TaskOutput {
    pub fn empty(state_hash: String) -> Self {
        TaskOutput { json: Value::Null, state_hash_after: state_hash }
    }

    pub fn with_value(json: Value, state_hash: String) -> Self {
        TaskOutput { json, state_hash_after: state_hash }
    }
}

// ── TaskContext ───────────────────────────────────────────────────────────────

/// Runtime context injected into every task.
///
/// Tasks use `ctx.bus.dispatch(effect)` for all side effects.
/// They must not call capabilities directly.
#[derive(Clone)]
pub struct TaskContext {
    pub task_id: TaskId,
    pub execution_id: ExecutionId,
    /// Position of this task in the topological execution order.
    pub topological_index: u32,
    pub capabilities: Arc<CapabilityRegistry>,
    /// All effects flow through here for dedup + routing.
    pub bus: Arc<EffectBus>,
    /// Resource consumption limits enforced by the executor.
    pub budget: ResourceBudget,
}

impl TaskContext {
    pub fn new(
        task_id: TaskId,
        execution_id: ExecutionId,
        topological_index: u32,
        capabilities: Arc<CapabilityRegistry>,
        bus: Arc<EffectBus>,
        budget: ResourceBudget,
    ) -> Self {
        TaskContext { task_id, execution_id, topological_index, capabilities, bus, budget }
    }
}

// ── Task trait ────────────────────────────────────────────────────────────────

/// A Task is the atomic unit of execution driven by the executor.
///
/// Implementors must be deterministic given equal inputs and an equal starting
/// kernel state. All side effects must go through `ctx.bus.dispatch()` so the
/// `EffectBus` can enforce dedup, budgets, and receipt assembly.
///
/// **Lifecycle invariant (I-08)**: one task = one atomic transaction in the kernel.
/// If a task emits multiple `KernelWrite` effects, they must all target the same
/// atomic boundary (i.e., a single Raft log entry or a single slab write).
#[async_trait]
pub trait Task: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    /// Execute the task with the given JSON-encoded inputs.
    ///
    /// `predecessor_outputs` is the map from `TaskId.0` → `TaskOutput` for
    /// every task that this task depends on. The task may deserialize these to
    /// chain outputs (e.g., pass embeddings from `EmbedTask` into `InsertRecordTask`).
    async fn run(
        &self,
        inputs_json: &str,
        predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput>;
}

// ── NoOpTask ──────────────────────────────────────────────────────────────────

/// A no-op task used for health-check operations and tests.
pub struct NoOpTask;

#[async_trait]
impl Task for NoOpTask {
    fn name(&self) -> &'static str { "noop" }

    async fn run(
        &self,
        _inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        use crate::effect::{Effect, EffectId, EffectPayload};
        let effect_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let e = Effect::ephemeral(effect_id, EffectPayload::Counter { name: "noop_runs".into(), value: 1.0 });
        ctx.bus.dispatch(e).await.ok();
        let state_hash = ctx.capabilities.kernel.state_hash(0);
        Ok(TaskOutput::empty(state_hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::EffectBus;
    use crate::capability::{CapabilityRegistry, NoOpKernelCapability};
    use valori_planner::graph::TaskId;
    use valori_planner::operation::ResourceBudget;

    fn make_ctx() -> TaskContext {
        let caps = Arc::new(CapabilityRegistry {
            kernel: Arc::new(NoOpKernelCapability { shard_count: 1 }),
            embed: None, llm: None, storage: None, http: None, proof: None, scheduler: None,
        });
        let bus = Arc::new(EffectBus::new(caps.clone()));
        let eid = ExecutionId { hi: 1, lo: 2 };
        TaskContext::new(TaskId(0), eid, 0, caps, bus, ResourceBudget::default())
    }

    #[tokio::test]
    async fn noop_task_returns_output() {
        let ctx = make_ctx();
        let out = NoOpTask.run("{}", &[], &ctx).await.unwrap();
        assert_eq!(out.json, Value::Null);
        assert_eq!(out.state_hash_after.len(), 64);
    }
}
