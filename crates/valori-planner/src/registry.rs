// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! ExecutionRegistry — in-process cache and history of ExecutionGraphs.
//!
//! The registry has two responsibilities:
//! 1. **Cache** (`ExecutionCache`): maps `(OperationHash, FP, CtxHash)` → `ExecutionGraph`.
//!    A cache hit means the planner skips graph construction.
//! 2. **Handle** (`ExecutionHandle`): tracks the runtime state of one in-flight or
//!    completed execution via tokio watch channels.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};

use crate::context::{PlannerFingerprint, PlanningContextHash};
use crate::graph::ExecutionGraph;
use crate::operation::{OperationHash, OperationId};

// ── ExecutionStatus ───────────────────────────────────────────────────────────

/// The lifecycle state of one execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionStatus {
    /// Scheduled but not yet running.
    Pending,
    /// At least one task is executing.
    Running { completed_tasks: u32, total_tasks: u32 },
    /// All tasks completed successfully.
    Succeeded,
    /// At least one task failed and the execution was abandoned.
    Failed { reason: String },
    /// Execution was cancelled by the caller.
    Cancelled,
}

impl ExecutionStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed { .. } | Self::Cancelled)
    }
}

// ── ExecutionHandle ───────────────────────────────────────────────────────────

/// A cheaply-cloneable handle to a running or completed execution.
///
/// Backed by a `tokio::sync::watch` channel: callers can `subscribe()` to receive
/// status updates without polling.
#[derive(Clone, Debug)]
pub struct ExecutionHandle {
    pub id: OperationId,
    sender: Arc<watch::Sender<ExecutionStatus>>,
    receiver: watch::Receiver<ExecutionStatus>,
}

impl ExecutionHandle {
    pub fn new(id: OperationId) -> Self {
        let (sender, receiver) = watch::channel(ExecutionStatus::Pending);
        ExecutionHandle { id, sender: Arc::new(sender), receiver }
    }

    /// Update the execution status. Notifies all subscribers.
    pub fn update(&self, status: ExecutionStatus) {
        let _ = self.sender.send(status);
    }

    /// Subscribe to future status updates.
    pub fn subscribe(&self) -> watch::Receiver<ExecutionStatus> {
        self.sender.subscribe()
    }

    /// Return the current status without blocking.
    pub fn current_status(&self) -> ExecutionStatus {
        self.receiver.borrow().clone()
    }
}

// ── TaskState ─────────────────────────────────────────────────────────────────

/// The state of a single task within an execution context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskState {
    Waiting,
    Running,
    Done,
    Failed(String),
    Skipped,
}

// ── ExecutionContext ──────────────────────────────────────────────────────────

/// Runtime context maintained by the executor for one active execution.
///
/// Created when the executor picks up an `ExecutionHandle`; dropped on completion.
#[derive(Debug)]
pub struct ExecutionContext {
    pub handle: ExecutionHandle,
    pub graph: Arc<ExecutionGraph>,
    pub task_states: Vec<TaskState>,
}

impl ExecutionContext {
    pub fn new(handle: ExecutionHandle, graph: Arc<ExecutionGraph>) -> Self {
        let task_count = graph.tasks.len();
        ExecutionContext {
            handle,
            graph,
            task_states: vec![TaskState::Waiting; task_count],
        }
    }

    pub fn completed_count(&self) -> u32 {
        self.task_states.iter().filter(|s| **s == TaskState::Done).count() as u32
    }

    pub fn total_count(&self) -> u32 {
        self.task_states.len() as u32
    }

    pub fn is_complete(&self) -> bool {
        self.task_states.iter().all(|s| matches!(s, TaskState::Done | TaskState::Failed(_) | TaskState::Skipped))
    }
}

// ── CacheKey ─────────────────────────────────────────────────────────────────

/// The triple cache key: `(OperationHash, PlannerFingerprint.hash, PlanningContextHash)`.
/// All three must match for a cached graph to be reused (RFC-0001 §3.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub op_hash: OperationHash,
    pub fp_hash: [u8; 32],
    pub ctx_hash: PlanningContextHash,
}

impl CacheKey {
    pub fn new(op_hash: OperationHash, fp: &PlannerFingerprint, ctx_hash: PlanningContextHash) -> Self {
        CacheKey { op_hash, fp_hash: fp.hash, ctx_hash }
    }
}

// ── ExecutionCache ────────────────────────────────────────────────────────────

/// In-process LRU-free graph cache. Bounded by `capacity`.
///
/// The durable cache is in `valori-metadata` (`MetadataDb::cache_put/get`).
/// This in-process layer avoids the redb round-trip for hot paths.
#[derive(Debug)]
pub struct ExecutionCache {
    capacity: usize,
    inner: RwLock<HashMap<CacheKey, Arc<ExecutionGraph>>>,
}

impl ExecutionCache {
    pub fn new(capacity: usize) -> Self {
        ExecutionCache { capacity, inner: RwLock::new(HashMap::new()) }
    }

    pub async fn get(&self, key: &CacheKey) -> Option<Arc<ExecutionGraph>> {
        self.inner.read().await.get(key).cloned()
    }

    pub async fn insert(&self, key: CacheKey, graph: Arc<ExecutionGraph>) {
        let mut map = self.inner.write().await;
        if map.len() >= self.capacity && !map.contains_key(&key) {
            // Simple eviction: remove an arbitrary entry.
            if let Some(evict_key) = map.keys().next().cloned() {
                map.remove(&evict_key);
            }
        }
        map.insert(key, graph);
    }

    pub async fn invalidate(&self, key: &CacheKey) -> bool {
        self.inner.write().await.remove(key).is_some()
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }
}

// ── ExecutionRegistry ─────────────────────────────────────────────────────────

/// The top-level registry: cache + active-handle index.
///
/// The registry is owned by the planner instance. The executor holds a clone
/// of the inner `Arc`s so it can update handles without holding the planner lock.
pub struct ExecutionRegistry {
    pub cache: Arc<ExecutionCache>,
    active: RwLock<HashMap<OperationId, ExecutionHandle>>,
}

impl ExecutionRegistry {
    pub fn new(cache_capacity: usize) -> Self {
        ExecutionRegistry {
            cache: Arc::new(ExecutionCache::new(cache_capacity)),
            active: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new execution handle.
    pub async fn register(&self, handle: ExecutionHandle) {
        self.active.write().await.insert(handle.id, handle);
    }

    /// Retrieve a handle for an in-progress or recently completed execution.
    pub async fn get_handle(&self, id: &OperationId) -> Option<ExecutionHandle> {
        self.active.read().await.get(id).cloned()
    }

    /// Remove a terminal handle from the active index (free memory).
    pub async fn retire(&self, id: &OperationId) -> bool {
        let mut map = self.active.write().await;
        if let Some(h) = map.get(id) {
            if h.current_status().is_terminal() {
                map.remove(id);
                return true;
            }
        }
        false
    }

    /// Number of currently-active (non-terminal) handles.
    pub async fn active_count(&self) -> usize {
        self.active.read().await
            .values()
            .filter(|h| !h.current_status().is_terminal())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::OperationId;

    #[tokio::test]
    async fn handle_status_transitions() {
        let id = OperationId::new();
        let h = ExecutionHandle::new(id);
        assert_eq!(h.current_status(), ExecutionStatus::Pending);
        h.update(ExecutionStatus::Running { completed_tasks: 1, total_tasks: 3 });
        assert!(!h.current_status().is_terminal());
        h.update(ExecutionStatus::Succeeded);
        assert!(h.current_status().is_terminal());
    }

    #[tokio::test]
    async fn cache_insert_and_get() {
        use crate::operation::{OperationHash, OperationKind, OperationInputs, ExecutionPolicy, compute_operation_hash};
        use crate::context::{PlannerFingerprint, PlanningContextHash, PlanningContext, CapabilitySet};
        use crate::graph::ExecutionGraph;
        use valori_metadata::history::ExecutionRetentionPolicy;

        let op = compute_operation_hash(OperationKind::HealthCheck, &OperationInputs::HealthCheck, &ExecutionPolicy::default());
        let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
        let ctx = PlanningContextHash::compute(&PlanningContext {
            capability_set: CapabilitySet { embed: false, llm: false, object_store: false, cluster: false, shard_count: 1 },
            schema_version: 1, shard_count: 1, cluster_epoch: 0, cluster_mode: false,
        });
        let key = CacheKey::new(op, &fp, ctx);
        let graph = Arc::new(ExecutionGraph::build(op, fp, ctx, vec![], vec![], ExecutionRetentionPolicy::default()));

        let cache = ExecutionCache::new(8);
        cache.insert(key.clone(), graph.clone()).await;
        let hit = cache.get(&key).await;
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().graph_hash, graph.graph_hash);
    }

    #[tokio::test]
    async fn registry_retire_terminal_handle() {
        let reg = ExecutionRegistry::new(8);
        let id = OperationId::new();
        let h = ExecutionHandle::new(id);
        h.update(ExecutionStatus::Succeeded);
        reg.register(h).await;
        let retired = reg.retire(&id).await;
        assert!(retired);
    }
}
