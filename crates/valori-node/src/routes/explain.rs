// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `_execution` observability block — one payload, every surface.
//!
//! When a request carries `?explain=true`, handlers attach an `_execution`
//! object to their JSON response describing HOW the request was executed:
//! the planner model, the ExecutionGraph (content-addressed hash + tasks +
//! edges), and the post-execution state hash. The CLI, Python SDK, UI, MCP,
//! and benchmarks all consume this one shape.
//!
//! Gating is opt-in (EXPLAIN-style): default responses are byte-for-byte
//! unchanged, so no existing client breaks and hot-path payloads stay lean.
//!
//! v1 (this module) exposes STRUCTURE + state hash — all reachable today.
//! Per-task Input/Output/Duration is v2 and needs the runner to time each
//! task (the same instrumentation as the per-crate flamegraph).

use serde::Deserialize;
use serde_json::{json, Value};
use valori_planner::graph::ExecutionGraph;

/// Planner evolution marker. Bump as the migration advances:
///   A13  = inline builder, cache inactive (today)
///   A15+ = structural planner, cached ExecutionPlan + runtime bindings
pub const PLANNER_VERSION: &str = "A13";

/// `?explain=true` query flag. Absent/false ⇒ no `_execution` block.
#[derive(Deserialize, Default)]
pub struct ExplainParams {
    #[serde(default)]
    pub explain: bool,
}

impl ExplainParams {
    #[inline]
    pub fn on(&self) -> bool {
        self.explain
    }
}

fn hex32(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Build the `_execution` block.
///
/// `graph` is `Some` for planner-routed ops (model INLINE) and `None` for
/// direct engine calls (model DIRECT). `state_hash` is the post-execution
/// BLAKE3 state hash, observed on both paths.
pub fn execution_block(
    operation: &str,
    graph: Option<&ExecutionGraph>,
    state_hash: &[u8; 32],
    duration_ms: Option<f64>,
) -> Value {
    match graph {
        Some(g) => {
            let tasks: Vec<Value> = g
                .tasks
                .iter()
                .map(|t| {
                    json!({
                        "id": t.id.0,
                        "kind": format!("{:?}", t.kind),
                        "shard": t.shard_id,
                    })
                })
                .collect();
            let edges: Vec<Value> = g
                .edges
                .iter()
                .map(|e| {
                    json!({
                        "from": e.from.0,
                        "to": e.to.0,
                    })
                })
                .collect();
            json!({
                "operation": operation,
                "model": "INLINE",
                "graph_hash": hex32(&g.graph_hash.0),
                "operation_hash": hex32(&g.operation_hash.0),
                "tasks": tasks,
                "edges": edges,
                "state_hash": hex32(state_hash),
                "duration_ms": duration_ms,
                "planner": { "model": "INLINE", "cache": false, "version": PLANNER_VERSION },
            })
        }
        None => json!({
            "operation": operation,
            "model": "DIRECT",
            "graph_hash": Value::Null,
            "operation_hash": Value::Null,
            "tasks": [],
            "edges": [],
            "state_hash": hex32(state_hash),
            "duration_ms": duration_ms,
            "planner": { "model": "DIRECT", "cache": false, "version": PLANNER_VERSION },
        }),
    }
}

/// Serialize `body`, then attach `_execution` as a SIBLING key (not a wrapper),
/// so `?explain=true` adds a field and default responses are unchanged.
pub fn with_execution<T: serde::Serialize>(body: T, execution: Option<Value>) -> Value {
    let mut v = serde_json::to_value(body).unwrap_or_else(|_| json!({}));
    if let (Some(ex), Some(map)) = (execution, v.as_object_mut()) {
        map.insert("_execution".to_string(), ex);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_planner::context::{
        CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash,
    };
    use valori_planner::graph::{
        ExecutionGraph, ExecutionRetentionPolicy, TaskId, TaskKind, TaskSpec,
    };
    use valori_planner::operation::{
        compute_operation_hash, ExecutionPolicy, OperationInputs, OperationKind,
    };

    fn sample_graph() -> ExecutionGraph {
        let op = compute_operation_hash(
            OperationKind::MemorySearch,
            &OperationInputs::MemorySearch {
                k: 5,
                collection: "bench".into(),
                shard_id: 0,
                decay: false,
            },
            &ExecutionPolicy::default(),
        );
        let fp = PlannerFingerprint::compute("0.2.4", [0u8; 32], [0u8; 32], 1);
        let ctx = PlanningContextHash::compute(&PlanningContext {
            capability_set: CapabilitySet {
                embed: false,
                llm: false,
                object_store: false,
                cluster: false,
                shard_count: 1,
            },
            schema_version: 1,
            shard_count: 1,
            cluster_epoch: 0,
            cluster_mode: false,
        });
        ExecutionGraph::build(
            op,
            fp,
            ctx,
            vec![TaskSpec {
                id: TaskId(0),
                kind: TaskKind::MemorySearch,
                inputs_json: "{}".into(),
                shard_id: Some(0),
                topological_index: 0,
            }],
            vec![],
            ExecutionRetentionPolicy::default(),
        )
    }

    #[test]
    fn inline_block_has_real_graph_hash() {
        let g = sample_graph();
        let block = execution_block("MemorySearch", Some(&g), &[0u8; 32], Some(0.47));
        assert_eq!(block["model"], "INLINE");
        assert_eq!(block["graph_hash"].as_str().unwrap().len(), 64); // 32 bytes hex
        assert_eq!(block["operation_hash"].as_str().unwrap().len(), 64);
        assert_eq!(block["tasks"][0]["kind"], "MemorySearch");
        assert_eq!(block["duration_ms"], 0.47);
        assert_eq!(block["planner"]["cache"], false);
    }

    #[test]
    fn direct_block_has_null_graph() {
        let block = execution_block("Search", None, &[1u8; 32], None);
        assert_eq!(block["model"], "DIRECT");
        assert!(block["graph_hash"].is_null());
        assert!(block["operation_hash"].is_null());
        assert_eq!(block["tasks"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn with_execution_adds_sibling_key_only_when_present() {
        #[derive(serde::Serialize)]
        struct R {
            results: Vec<u32>,
        }
        let none = with_execution(
            R {
                results: vec![1, 2],
            },
            None,
        );
        assert!(none.get("_execution").is_none());
        assert_eq!(none["results"], json!([1, 2]));

        let some = with_execution(R { results: vec![1] }, Some(json!({"model": "DIRECT"})));
        assert_eq!(some["_execution"]["model"], "DIRECT");
        assert_eq!(some["results"], json!([1]));
    }
}
