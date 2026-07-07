// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! ExecutionGraph — the deterministic DAG of Tasks produced by the Planner.
use serde::{Deserialize, Serialize};
use valori_core::id::ExecutionId;
use valori_metadata::history::ExecutionRetentionPolicy;

use crate::operation::{OperationHash};
use crate::context::{PlannerFingerprint, PlanningContextHash};

// ── TaskId ────────────────────────────────────────────────────────────────────

/// A zero-based index into `ExecutionGraph.tasks`.
/// Tasks are referred to by index, not by UUID, to keep the graph compact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub u32);

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "task#{}", self.0)
    }
}

// ── TaskKind ──────────────────────────────────────────────────────────────────

/// The kind of work a Task performs.
///
/// The runtime maps each `TaskKind` to a concrete task implementation.
/// Adding a variant is backward-compatible; removing one requires an ABI bump.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Call the embedding provider to convert text to a vector.
    Embed,
    /// Insert a vector record into the kernel.
    InsertRecord,
    /// Insert a graph node into the kernel's knowledge graph.
    InsertNode,
    /// Insert a directed edge into the knowledge graph.
    InsertEdge,
    /// Soft-delete a record.
    SoftDeleteRecord,
    /// Search for k-nearest neighbors.
    Search,
    /// GraphRAG: k-NN + connected subgraph traversal.
    GraphRag,
    /// Call an LLM for completion / entity extraction.
    LlmComplete,
    /// Fetch a URL via HTTP.
    HttpFetch,
    /// Read-index check for linearizable reads.
    ReadIndex,
    /// Write a snapshot artifact to object store.
    SnapshotArtifact,
    /// Emit a proof receipt fragment.
    ProofFragment,
}

// ── TaskSpec ──────────────────────────────────────────────────────────────────

/// A declarative specification for one Task in an `ExecutionGraph`.
///
/// The Planner produces `TaskSpec`s; the runtime constructs the concrete Task
/// implementation from `kind` + `inputs_json`. `inputs_json` is opaque to the
/// planner — only the runtime understands its schema.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: TaskId,
    pub kind: TaskKind,
    /// JSON-serialized task-specific inputs (e.g. `{"k": 5, "collection": "x"}`).
    /// The runtime deserializes this into the concrete task input type.
    pub inputs_json: String,
    /// The shard this task's kernel writes target. `None` for tasks with no kernel writes.
    pub shard_id: Option<u8>,
    /// Topological index — position in the linearized topo order.
    /// Set by `ExecutionGraph::assign_topological_indices()` after construction.
    pub topological_index: u32,
}

// ── TaskEdge ──────────────────────────────────────────────────────────────────

/// A directed dependency edge in an `ExecutionGraph`.
/// `from` must complete before `to` may start.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskEdge {
    pub from: TaskId,
    pub to: TaskId,
    /// Reserved for future speculative execution. Always `None` today.
    pub condition: Option<String>,
}

// ── GraphHash ─────────────────────────────────────────────────────────────────

/// Content-addressed hash of an `ExecutionGraph`.
///
/// `graph_hash = BLAKE3(op_hash ‖ fp.hash ‖ ctx_hash ‖ topo_order_bytes)`
///
/// Two planners running with the same inputs must produce graphs with the same
/// `graph_hash` (invariant I-03).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GraphHash(pub [u8; 32]);

impl GraphHash {
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

// ── ExecutionGraph ────────────────────────────────────────────────────────────

/// A deterministic DAG of Tasks, produced by the Planner for one Operation.
///
/// The `graph_hash` commits to the full topology; two graphs are interchangeable
/// if and only if their `graph_hash`es match.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionGraph {
    /// Unique ID for this graph instance — different from `OperationId`.
    pub id: ExecutionId,
    /// The operation this graph fulfills.
    pub operation_hash: OperationHash,
    /// The planner configuration that produced this graph.
    pub planner_fingerprint: PlannerFingerprint,
    /// The planning context hash.
    pub planning_context_hash: PlanningContextHash,
    /// The content-addressed hash of this graph.
    pub graph_hash: GraphHash,
    /// Ordered task list. `TaskSpec.id` indexes into this vec.
    pub tasks: Vec<TaskSpec>,
    /// Dependency edges.
    pub edges: Vec<TaskEdge>,
    /// How long to retain this graph in `ExecutionHistory`.
    pub retention: ExecutionRetentionPolicy,
}

impl ExecutionGraph {
    /// Build a graph and compute its `graph_hash`.
    ///
    /// Assigns `topological_index` to every `TaskSpec` in topological order
    /// before computing the hash, so the hash is stable regardless of insertion
    /// order.
    pub fn build(
        operation_hash: OperationHash,
        planner_fingerprint: PlannerFingerprint,
        planning_context_hash: PlanningContextHash,
        mut tasks: Vec<TaskSpec>,
        edges: Vec<TaskEdge>,
        retention: ExecutionRetentionPolicy,
    ) -> Self {
        let topo = topological_order(&tasks, &edges);
        for (idx, &task_idx) in topo.iter().enumerate() {
            if let Some(t) = tasks.get_mut(task_idx as usize) {
                t.topological_index = idx as u32;
            }
        }
        let graph_hash = compute_graph_hash(&operation_hash, &planner_fingerprint, &planning_context_hash, &topo);
        ExecutionGraph {
            id: ExecutionId::new_random(),
            operation_hash,
            planner_fingerprint,
            planning_context_hash,
            graph_hash,
            tasks,
            edges,
            retention,
        }
    }

    /// Return tasks sorted by `topological_index` (ascending).
    /// This is the canonical order for receipt assembly.
    pub fn tasks_in_topo_order(&self) -> Vec<&TaskSpec> {
        let mut sorted: Vec<&TaskSpec> = self.tasks.iter().collect();
        sorted.sort_by_key(|t| t.topological_index);
        sorted
    }

    /// Return the `TaskSpec` for a given `TaskId`, if it exists.
    pub fn task(&self, id: TaskId) -> Option<&TaskSpec> {
        self.tasks.get(id.0 as usize)
    }

    /// Return all `TaskId`s that `id` directly depends on (predecessors).
    pub fn predecessors(&self, id: TaskId) -> Vec<TaskId> {
        self.edges.iter()
            .filter(|e| e.to == id)
            .map(|e| e.from)
            .collect()
    }
}

/// Kahn's algorithm — produces task IDs in topological order.
/// Returns indices in the order they should execute.
fn topological_order(tasks: &[TaskSpec], edges: &[TaskEdge]) -> Vec<u32> {
    let n = tasks.len();
    let mut in_degree = vec![0u32; n];
    let mut adj: Vec<Vec<u32>> = vec![vec![]; n];

    for edge in edges {
        let from = edge.from.0 as usize;
        let to   = edge.to.0   as usize;
        if from < n && to < n {
            adj[from].push(to as u32);
            in_degree[to] += 1;
        }
    }

    let mut queue: std::collections::VecDeque<u32> = in_degree.iter()
        .enumerate()
        .filter(|(_, &d)| d == 0)
        .map(|(i, _)| i as u32)
        .collect();

    let mut order = Vec::with_capacity(n);
    while let Some(node) = queue.pop_front() {
        order.push(node);
        for &next in &adj[node as usize] {
            in_degree[next as usize] -= 1;
            if in_degree[next as usize] == 0 {
                queue.push_back(next);
            }
        }
    }
    // If cycle detected, append remaining nodes (should not happen with valid graphs)
    if order.len() < n {
        for i in 0..n {
            if !order.contains(&(i as u32)) {
                order.push(i as u32);
            }
        }
    }
    order
}

fn compute_graph_hash(
    op: &OperationHash,
    fp: &PlannerFingerprint,
    ctx: &PlanningContextHash,
    topo_order: &[u32],
) -> GraphHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&op.0);
    hasher.update(&fp.hash);
    hasher.update(&ctx.0);
    for &idx in topo_order {
        hasher.update(&idx.to_le_bytes());
    }
    GraphHash(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{OperationHash, compute_operation_hash, OperationKind, OperationInputs, ExecutionPolicy};
    use crate::context::{PlannerFingerprint, PlanningContext, PlanningContextHash, CapabilitySet};
    use valori_metadata::history::ExecutionRetentionPolicy;

    fn fingerprint() -> PlannerFingerprint {
        PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1)
    }

    fn ctx_hash() -> PlanningContextHash {
        PlanningContextHash::compute(&PlanningContext {
            capability_set: CapabilitySet { embed: false, llm: false, object_store: false, cluster: false, shard_count: 1 },
            schema_version: 1, shard_count: 1, cluster_epoch: 0, cluster_mode: false,
        })
    }

    fn op_hash() -> OperationHash {
        compute_operation_hash(OperationKind::HealthCheck, &OperationInputs::HealthCheck, &ExecutionPolicy::default())
    }

    #[test]
    fn empty_graph_hash_is_stable() {
        let fp = fingerprint();
        let ctx = ctx_hash();
        let op = op_hash();

        let g1 = ExecutionGraph::build(op, fp.clone(), ctx, vec![], vec![], ExecutionRetentionPolicy::default());
        let g2 = ExecutionGraph::build(op, fp, ctx, vec![], vec![], ExecutionRetentionPolicy::default());
        assert_eq!(g1.graph_hash, g2.graph_hash);
    }

    #[test]
    fn topological_order_two_tasks() {
        // task 0 → task 1
        let tasks = vec![
            TaskSpec { id: TaskId(0), kind: TaskKind::Embed, inputs_json: "{}".into(), shard_id: None, topological_index: 0 },
            TaskSpec { id: TaskId(1), kind: TaskKind::InsertRecord, inputs_json: "{}".into(), shard_id: Some(0), topological_index: 0 },
        ];
        let edges = vec![TaskEdge { from: TaskId(0), to: TaskId(1), condition: None }];
        let order = topological_order(&tasks, &edges);
        assert_eq!(order, vec![0, 1]);
    }
}
