// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Planner trait + `plan_with_cache()` function.
//!
//! The Planner turns an `(Operation, PlanningContext)` pair into an
//! `ExecutionGraph`. It consults the in-process `ExecutionCache` first, then the
//! durable `MetadataDb` cache, and only constructs a new graph on a double miss.
use std::sync::Arc;
use tracing::{debug, instrument};

use valori_metadata::db::MetadataDb;
use valori_metadata::planner_cache::{PlannerCacheEntry, PlannerCacheKey};

use crate::context::{PlannerFingerprint, PlanningContext, PlanningContextHash};
use crate::error::{PlannerError, PlannerResult};
use crate::graph::ExecutionRetentionPolicy;
use crate::graph::{ExecutionGraph, TaskEdge, TaskId, TaskKind, TaskSpec};
use crate::operation::Operation;
use crate::registry::{CacheKey, ExecutionCache};

// ── Planner trait ─────────────────────────────────────────────────────────────

/// The Planner converts an `Operation` and a `PlanningContext` into an `ExecutionGraph`.
///
/// Implementors must be deterministic: equal inputs must always produce graphs
/// with the same `GraphHash` (invariant I-03).
pub trait Planner: Send + Sync + 'static {
    /// Construct a new `ExecutionGraph` for the given operation and context.
    ///
    /// This is called only on cache miss — never call it directly; use
    /// `plan_with_cache()` instead.
    fn plan(
        &self,
        op: &Operation,
        ctx: &PlanningContext,
        fp: &PlannerFingerprint,
    ) -> PlannerResult<ExecutionGraph>;
}

// ── plan_with_cache ───────────────────────────────────────────────────────────

/// Try the in-process cache, then the durable DB cache, then invoke the planner.
///
/// Cache lookup order:
///   1. `ExecutionCache` (in-process, O(1))
///   2. `MetadataDb::cache_get` (redb on disk)
///   3. `planner.plan()` (fresh construction)
///   4. Store result in both caches for future use.
#[instrument(skip_all, fields(op_id = %op.id.0, kind = ?op.kind))]
pub async fn plan_with_cache(
    planner: &dyn Planner,
    op: &Operation,
    ctx: &PlanningContext,
    fp: &PlannerFingerprint,
    cache: &ExecutionCache,
    db: Option<&MetadataDb>,
) -> PlannerResult<Arc<ExecutionGraph>> {
    let ctx_hash = PlanningContextHash::compute(ctx);
    let cache_key = CacheKey::new(op.hash, fp, ctx_hash);

    // 1. In-process cache hit.
    if let Some(graph) = cache.get(&cache_key).await {
        debug!("planner: in-process cache hit");
        return Ok(graph);
    }

    // 2. Durable DB cache hit.
    if let Some(db) = db {
        let db_key = PlannerCacheKey {
            operation_hash: op.hash.to_hex(),
            planner_fingerprint_hash: fp.hash_hex(),
            planning_context_hash: ctx_hash.to_hex(),
        };
        if let Ok(Some(entry)) = db.cache_get(&db_key) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if !entry.is_expired(now) {
                if let Ok(graph) = serde_json::from_str::<ExecutionGraph>(&entry.graph_json) {
                    let graph = Arc::new(graph);
                    cache.insert(cache_key, graph.clone()).await;
                    debug!("planner: DB cache hit");
                    return Ok(graph);
                }
            }
        }
    }

    // 3. Cache miss: build a new graph.
    let graph = planner.plan(op, ctx, fp)?;
    let graph = Arc::new(graph);

    // 4. Populate both caches.
    cache.insert(cache_key.clone(), graph.clone()).await;
    if let Some(db) = db {
        let db_key = PlannerCacheKey {
            operation_hash: op.hash.to_hex(),
            planner_fingerprint_hash: fp.hash_hex(),
            planning_context_hash: ctx_hash.to_hex(),
        };
        if let Ok(graph_json) = serde_json::to_string(graph.as_ref()) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let entry = PlannerCacheEntry {
                graph_json,
                cached_at: now,
                expires_at: 0,
            };
            let _ = db.cache_put(&db_key, &entry);
        }
    }

    debug!("planner: cache miss — new graph constructed");
    Ok(graph)
}

// ── NoOpPlanner ───────────────────────────────────────────────────────────────

/// A minimal `Planner` that produces an empty single-task graph.
///
/// Used in unit tests and as a placeholder before the real planner is wired in.
/// For `HealthCheck` operations it produces a single `ProofFragment` task.
/// For all other operations it produces a single `ReadIndex` task as a stub.
pub struct NoOpPlanner;

impl Planner for NoOpPlanner {
    fn plan(
        &self,
        op: &Operation,
        _ctx: &PlanningContext,
        fp: &PlannerFingerprint,
    ) -> PlannerResult<ExecutionGraph> {
        use crate::operation::OperationKind;

        let ctx_hash = PlanningContextHash::compute(_ctx);

        let (kind, inputs_json) = match op.kind {
            OperationKind::HealthCheck => (TaskKind::ProofFragment, r#"{"probe":true}"#.into()),
            _ => (
                TaskKind::ReadIndex,
                format!(r#"{{"kind":"{:?}"}}"#, op.kind),
            ),
        };

        let task = TaskSpec {
            id: TaskId(0),
            kind,
            inputs_json,
            shard_id: None,
            topological_index: 0,
        };

        let graph = ExecutionGraph::build(
            op.hash,
            fp.clone(),
            ctx_hash,
            vec![task],
            vec![],
            ExecutionRetentionPolicy::default(),
        );
        Ok(graph)
    }
}

// ── IngestPlanner ─────────────────────────────────────────────────────────────

/// A concrete `Planner` for `Ingest` operations.
///
/// Produces: `[Embed → InsertRecord → InsertNode → InsertEdge]`
/// when embed is enabled, or `[InsertRecord → InsertNode]` when disabled.
///
/// Kept here to show the pattern; the real implementation will do more.
pub struct IngestPlanner;

impl Planner for IngestPlanner {
    fn plan(
        &self,
        op: &Operation,
        ctx: &PlanningContext,
        fp: &PlannerFingerprint,
    ) -> PlannerResult<ExecutionGraph> {
        use crate::operation::OperationInputs;

        let ctx_hash = PlanningContextHash::compute(ctx);

        let (embed_enabled, shard_id) = match &op.inputs {
            OperationInputs::Ingest {
                embed_enabled,
                shard_id,
                ..
            } => (*embed_enabled, *shard_id),
            _ => {
                return Err(PlannerError::InvalidOperation(
                    "IngestPlanner received non-Ingest operation".into(),
                ))
            }
        };

        let (tasks, edges) = if embed_enabled {
            let embed = TaskSpec {
                id: TaskId(0),
                kind: TaskKind::Embed,
                inputs_json: "{}".into(),
                shard_id: None,
                topological_index: 0,
            };
            let insert = TaskSpec {
                id: TaskId(1),
                kind: TaskKind::InsertRecord,
                inputs_json: "{}".into(),
                shard_id: Some(shard_id),
                topological_index: 0,
            };
            let node = TaskSpec {
                id: TaskId(2),
                kind: TaskKind::InsertNode,
                inputs_json: "{}".into(),
                shard_id: Some(shard_id),
                topological_index: 0,
            };
            let edge = TaskSpec {
                id: TaskId(3),
                kind: TaskKind::InsertEdge,
                inputs_json: "{}".into(),
                shard_id: Some(shard_id),
                topological_index: 0,
            };
            let edges = vec![
                TaskEdge {
                    from: TaskId(0),
                    to: TaskId(1),
                },
                TaskEdge {
                    from: TaskId(1),
                    to: TaskId(2),
                },
                TaskEdge {
                    from: TaskId(2),
                    to: TaskId(3),
                },
            ];
            (vec![embed, insert, node, edge], edges)
        } else {
            let insert = TaskSpec {
                id: TaskId(0),
                kind: TaskKind::InsertRecord,
                inputs_json: "{}".into(),
                shard_id: Some(shard_id),
                topological_index: 0,
            };
            let node = TaskSpec {
                id: TaskId(1),
                kind: TaskKind::InsertNode,
                inputs_json: "{}".into(),
                shard_id: Some(shard_id),
                topological_index: 0,
            };
            let edges = vec![TaskEdge {
                from: TaskId(0),
                to: TaskId(1),
            }];
            (vec![insert, node], edges)
        };

        Ok(ExecutionGraph::build(
            op.hash,
            fp.clone(),
            ctx_hash,
            tasks,
            edges,
            ExecutionRetentionPolicy::default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{CapabilitySet, PlanningContext};
    use crate::operation::{ExecutionPolicy, Operation, OperationInputs, OperationKind};
    use crate::registry::ExecutionCache;

    fn ctx() -> PlanningContext {
        PlanningContext {
            capability_set: CapabilitySet {
                embed: true,
                llm: false,
                object_store: false,
                cluster: false,
                shard_count: 1,
            },
            schema_version: 1,
            shard_count: 1,
            cluster_epoch: 0,
            cluster_mode: false,
        }
    }

    fn fp() -> PlannerFingerprint {
        PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1)
    }

    #[test]
    fn noop_planner_health_check() {
        let op = Operation::new(
            OperationKind::HealthCheck,
            OperationInputs::HealthCheck,
            ExecutionPolicy::default(),
        );
        let graph = NoOpPlanner.plan(&op, &ctx(), &fp()).unwrap();
        assert_eq!(graph.tasks.len(), 1);
        assert_eq!(graph.tasks[0].kind, crate::graph::TaskKind::ProofFragment);
    }

    #[test]
    fn ingest_planner_with_embed() {
        let inputs = OperationInputs::Ingest {
            strategy: "tree".into(),
            collection: "default".into(),
            shard_id: 0,
            embed_enabled: true,
        };
        let op = Operation::new(OperationKind::Ingest, inputs, ExecutionPolicy::default());
        let graph = IngestPlanner.plan(&op, &ctx(), &fp()).unwrap();
        assert_eq!(graph.tasks.len(), 4);
    }

    #[test]
    fn ingest_planner_without_embed() {
        let inputs = OperationInputs::Ingest {
            strategy: "auto".into(),
            collection: "default".into(),
            shard_id: 0,
            embed_enabled: false,
        };
        let op = Operation::new(OperationKind::Ingest, inputs, ExecutionPolicy::default());
        let graph = IngestPlanner.plan(&op, &ctx(), &fp()).unwrap();
        assert_eq!(graph.tasks.len(), 2);
    }

    #[test]
    fn ingest_graph_hash_is_stable() {
        let inputs = OperationInputs::Ingest {
            strategy: "tree".into(),
            collection: "default".into(),
            shard_id: 0,
            embed_enabled: true,
        };
        let op = Operation::new(OperationKind::Ingest, inputs, ExecutionPolicy::default());
        let fp = fp();
        let ctx = ctx();
        let g1 = IngestPlanner.plan(&op, &ctx, &fp).unwrap();
        let g2 = IngestPlanner.plan(&op, &ctx, &fp).unwrap();
        assert_eq!(g1.graph_hash, g2.graph_hash);
    }

    #[tokio::test]
    async fn plan_with_cache_hit_on_second_call() {
        let inputs = OperationInputs::Ingest {
            strategy: "auto".into(),
            collection: "default".into(),
            shard_id: 0,
            embed_enabled: false,
        };
        let op = Operation::new(OperationKind::Ingest, inputs, ExecutionPolicy::default());
        let fp = fp();
        let ctx = ctx();
        let cache = ExecutionCache::new(64);

        let g1 = plan_with_cache(&IngestPlanner, &op, &ctx, &fp, &cache, None)
            .await
            .unwrap();
        let g2 = plan_with_cache(&IngestPlanner, &op, &ctx, &fp, &cache, None)
            .await
            .unwrap();
        // Same Arc pointer — came from the cache.
        assert!(Arc::ptr_eq(&g1, &g2));
    }
}
