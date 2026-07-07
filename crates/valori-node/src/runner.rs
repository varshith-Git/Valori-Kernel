// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! TaskRunner — drives one ExecutionGraph to completion.
//!
//! The runner walks tasks in topological order, constructs a `TaskContext` for
//! each task, dispatches to the registered concrete `Task` implementation, and
//! collects `TaskOutput`s for successor tasks.
//!
//! ## Lifecycle (RFC-0004 §5)
//!
//! ```text
//! 1. ExecutionHandle.update(Running { completed: 0, total })
//! 2. For each task in topological order:
//!    a. Build TaskContext (bus, capabilities, budget)
//!    b. Resolve predecessor outputs
//!    c. task.run(inputs_json, predecessors, &ctx)
//!    d. Retry up to policy.retry_limit on EffectError::TaskFailed
//!    e. Propagate non-retryable errors → mark execution Failed
//! 3. ExecutionHandle.update(Succeeded)
//! ```
use std::collections::HashMap;
use std::sync::Arc;

use tracing::{debug, error, info, warn};

use valori_effect::bus::EffectBus;
use valori_effect::capability::CapabilityRegistry;
use valori_effect::error::{EffectError, EffectResult};
use valori_effect::task::{NoOpTask, Task, TaskContext, TaskOutput};
use valori_effect::tasks::embed::EmbedTask;
use valori_effect::tasks::insert_record::InsertRecordTask;
use valori_effect::tasks::insert_graph::{InsertNodeTask, InsertEdgeTask};
use valori_effect::tasks::search::SearchTask;
use valori_planner::graph::{ExecutionGraph, TaskKind};
use valori_planner::operation::ExecutionPolicy;
use valori_planner::registry::{ExecutionHandle, ExecutionStatus};

// ── TaskRegistry ──────────────────────────────────────────────────────────────

/// Maps `TaskKind` to a concrete `Task` implementation.
///
/// Constructed once at node startup and shared (via `Arc`) across all executions.
pub struct TaskRegistry {
    tasks: HashMap<String, Arc<dyn Task>>,
    pub jobs: Arc<tokio::sync::RwLock<HashMap<String, serde_json::Value>>>,
}

impl TaskRegistry {
    /// Build a default registry with all built-in tasks wired up.
    pub fn default_registry() -> Self {
        let mut tasks: HashMap<String, Arc<dyn Task>> = HashMap::new();
        tasks.insert("embed".into(),         Arc::new(EmbedTask));
        tasks.insert("insert_record".into(), Arc::new(InsertRecordTask));
        tasks.insert("insert_node".into(),   Arc::new(InsertNodeTask));
        tasks.insert("insert_edge".into(),   Arc::new(InsertEdgeTask));
        tasks.insert("search".into(),        Arc::new(SearchTask));
        tasks.insert("noop".into(),          Arc::new(NoOpTask));
        // Additional task kinds use NoOpTask as a passthrough stub until A8.
        for kind_name in &[
            "soft_delete_record",
            "graph_rag", "llm_complete", "http_fetch",
            "read_index", "snapshot_artifact", "proof_fragment",
        ] {
            tasks.insert(kind_name.to_string(), Arc::new(NoOpTask));
        }
        TaskRegistry { tasks, jobs: Arc::new(tokio::sync::RwLock::new(HashMap::new())) }
    }

    pub fn get(&self, kind: &TaskKind) -> Option<Arc<dyn Task>> {
        let key = format!("{:?}", kind).to_lowercase()
            .replace("insertrecord", "insert_record")
            .replace("insertnode", "insert_node")
            .replace("insertedge", "insert_edge")
            .replace("softdeleterecord", "soft_delete_record")
            .replace("graphrag", "graph_rag")
            .replace("llmcomplete", "llm_complete")
            .replace("httpfetch", "http_fetch")
            .replace("readindex", "read_index")
            .replace("snapshotartifact", "snapshot_artifact")
            .replace("prooffragment", "proof_fragment");
        self.tasks.get(&key).cloned()
    }
}

fn kind_to_key(kind: &TaskKind) -> &'static str {
    match kind {
        TaskKind::Embed              => "embed",
        TaskKind::InsertRecord       => "insert_record",
        TaskKind::InsertNode         => "insert_node",
        TaskKind::InsertEdge         => "insert_edge",
        TaskKind::SoftDeleteRecord   => "soft_delete_record",
        TaskKind::Search             => "search",
        TaskKind::GraphRag           => "graph_rag",
        TaskKind::LlmComplete        => "llm_complete",
        TaskKind::HttpFetch          => "http_fetch",
        TaskKind::ReadIndex          => "read_index",
        TaskKind::SnapshotArtifact   => "snapshot_artifact",
        TaskKind::ProofFragment      => "proof_fragment",
    }
}

impl TaskRegistry {
    pub fn get_by_key(&self, kind: &TaskKind) -> Option<Arc<dyn Task>> {
        self.tasks.get(kind_to_key(kind)).cloned()
    }
}

// ── TaskRunner ────────────────────────────────────────────────────────────────

/// Drives one `ExecutionGraph` to completion.
///
/// Constructed per-execution and dropped when the graph finishes or fails.
pub struct TaskRunner {
    graph: Arc<ExecutionGraph>,
    capabilities: Arc<CapabilityRegistry>,
    registry: Arc<TaskRegistry>,
    policy: ExecutionPolicy,
}

impl TaskRunner {
    pub fn new(
        graph: Arc<ExecutionGraph>,
        capabilities: Arc<CapabilityRegistry>,
        registry: Arc<TaskRegistry>,
        policy: ExecutionPolicy,
    ) -> Self {
        TaskRunner { graph, capabilities, registry, policy }
    }

    /// Run the graph to completion, updating `handle` as tasks progress.
    ///
    /// Returns the per-task outputs (indexed by TaskId) on success, or `Err` after
    /// exhausting the retry limit. HTTP handlers index into the vec to read back
    /// assigned IDs (e.g., `outputs[0]["record_id"]`).
    pub async fn run(self, handle: ExecutionHandle) -> EffectResult<Vec<Option<TaskOutput>>> {
        let total = self.graph.tasks.len() as u32;
        handle.update(ExecutionStatus::Running { completed_tasks: 0, total_tasks: total });

        let bus = Arc::new(EffectBus::new(self.capabilities.clone()));
        let execution_id = self.graph.id;

        // `outputs[task_id]` holds the output once a task completes.
        let mut outputs: Vec<Option<TaskOutput>> = vec![None; self.graph.tasks.len()];

        // Collect task specs in topo order upfront so we don't borrow self.graph
        // across an .await boundary (which would make the future non-Send).
        let sorted_task_ids: Vec<_> = self.graph.tasks_in_topo_order()
            .into_iter()
            .map(|t| t.id)
            .collect();

        for task_id in sorted_task_ids {
            let task_spec = match self.graph.task(task_id) {
                Some(t) => t.clone(),
                None => continue,
            };
            let task_impl = self.registry.get_by_key(&task_spec.kind)
                .ok_or_else(|| EffectError::TaskFailed(format!("no impl for {:?}", task_spec.kind)))?;

            // Build predecessor output slice.
            let predecessors: Vec<Option<TaskOutput>> = self.graph.predecessors(task_spec.id)
                .iter()
                .map(|pred_id| outputs.get(pred_id.0 as usize).and_then(|o| o.clone()))
                .collect();

            let ctx = TaskContext::new(
                task_spec.id,
                execution_id,
                task_spec.topological_index,
                self.capabilities.clone(),
                bus.clone(),
                self.policy.resource_budget.clone(),
            );

            let output = self.run_with_retry(
                task_impl,
                &task_spec.inputs_json,
                &predecessors,
                &ctx,
            ).await?;

            if let Some(slot) = outputs.get_mut(task_spec.id.0 as usize) {
                *slot = Some(output);
            }

            let completed = outputs.iter().filter(|o| o.is_some()).count() as u32;
            handle.update(ExecutionStatus::Running { completed_tasks: completed, total_tasks: total });
            debug!("task completed");
        }

        let durable_effects = bus.durable_count().await;
        info!(durable_effects, "execution succeeded");
        handle.update(ExecutionStatus::Succeeded);
        Ok(outputs)
    }

    async fn run_with_retry(
        &self,
        task: Arc<dyn Task>,
        inputs_json: &str,
        predecessors: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let mut attempts = 0u8;
        let max_attempts = self.policy.retry_limit + 1;

        loop {
            match task.run(inputs_json, predecessors, ctx).await {
                Ok(output) => return Ok(output),
                Err(EffectError::TaskFailed(_reason)) if attempts < max_attempts => {
                    attempts += 1;
                    warn!(attempts, "task failed — retrying");
                }
                Err(e) => {
                    error!("task failed with non-retryable error: {e}");
                    return Err(e);
                }
            }
        }
    }
}

// ── run_graph (standalone convenience fn) ─────────────────────────────────────

/// Execute one `ExecutionGraph` against the given capabilities.
///
/// This is the primary entry point for the HTTP handlers (Phase A9).
/// Creates a fresh `TaskRunner`, spawns it on the current tokio runtime, and
/// returns the `ExecutionHandle` so the caller can await or poll status.
pub fn run_graph(
    graph: Arc<ExecutionGraph>,
    capabilities: Arc<CapabilityRegistry>,
    registry: Arc<TaskRegistry>,
    policy: ExecutionPolicy,
) -> ExecutionHandle {
    use valori_planner::operation::OperationId;
    let handle = ExecutionHandle::new(OperationId(graph.id));
    let handle_clone = handle.clone();

    tokio::spawn(async move {
        let runner = TaskRunner::new(graph, capabilities, registry, policy);
        match runner.run(handle_clone.clone()).await {
            Ok(_) => {}
            Err(e) => handle_clone.update(ExecutionStatus::Failed { reason: e.to_string() }),
        }
    });

    handle
}

// ── run_graph_inline ─────────────────────────────────────────────────────────

/// Execute one `ExecutionGraph` synchronously on the current task.
///
/// Unlike `run_graph`, this does not spawn a background task — the caller
/// awaits the result directly and receives the per-task outputs. Use this in
/// HTTP handlers that need to read back assigned IDs from the task output.
pub async fn run_graph_inline(
    graph: Arc<ExecutionGraph>,
    capabilities: Arc<CapabilityRegistry>,
    registry: Arc<TaskRegistry>,
    policy: ExecutionPolicy,
) -> EffectResult<Vec<Option<TaskOutput>>> {
    use valori_planner::operation::OperationId;
    let handle = ExecutionHandle::new(OperationId(graph.id));
    let runner = TaskRunner::new(graph, capabilities, registry, policy);
    runner.run(handle).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use valori_effect::capability::CapabilityRegistry;
    use valori_effect::NoOpKernelCapability;
    use valori_planner::context::{CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash};
    use valori_planner::graph::ExecutionGraph;
    use valori_planner::operation::{ExecutionPolicy, compute_operation_hash, OperationKind, OperationInputs};
    use valori_metadata::history::ExecutionRetentionPolicy;
    use valori_planner::registry::ExecutionHandle;
    use valori_planner::operation::OperationId;

    fn caps() -> Arc<CapabilityRegistry> {
        Arc::new(CapabilityRegistry {
            kernel: Arc::new(NoOpKernelCapability { shard_count: 1 }),
            embed: None, llm: None, storage: None, http: None, proof: None, scheduler: None,
        })
    }

    fn empty_graph() -> Arc<ExecutionGraph> {
        let op = compute_operation_hash(OperationKind::HealthCheck, &OperationInputs::HealthCheck, &ExecutionPolicy::default());
        let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
        let ctx_hash = PlanningContextHash::compute(&PlanningContext {
            capability_set: CapabilitySet { embed: false, llm: false, object_store: false, cluster: false, shard_count: 1 },
            schema_version: 1, shard_count: 1, cluster_epoch: 0, cluster_mode: false,
        });
        Arc::new(ExecutionGraph::build(op, fp, ctx_hash, vec![], vec![], ExecutionRetentionPolicy::default()))
    }

    #[tokio::test]
    async fn empty_graph_succeeds() {
        let graph = empty_graph();
        let handle = ExecutionHandle::new(OperationId(graph.id));
        let runner = TaskRunner::new(graph, caps(), Arc::new(TaskRegistry::default_registry()), ExecutionPolicy::default());
        runner.run(handle.clone()).await.unwrap();
        assert_eq!(handle.current_status(), ExecutionStatus::Succeeded);
    }

    #[tokio::test]
    async fn run_graph_convenience_fn() {
        let graph = empty_graph();
        let handle = run_graph(graph, caps(), Arc::new(TaskRegistry::default_registry()), ExecutionPolicy::default());
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(handle.current_status(), ExecutionStatus::Succeeded);
    }

    #[tokio::test]
    async fn task_registry_default_has_all_kinds() {
        let reg = TaskRegistry::default_registry();
        for kind in &[
            TaskKind::Embed, TaskKind::InsertRecord, TaskKind::Search,
            TaskKind::InsertNode, TaskKind::InsertEdge, TaskKind::ProofFragment,
        ] {
            assert!(reg.get_by_key(kind).is_some(), "missing impl for {:?}", kind);
        }
    }
}
