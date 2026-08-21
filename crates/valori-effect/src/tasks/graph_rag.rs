// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! GraphRagTask — kNN vector search + subgraph expansion.
//!
//! Inputs:
//!   `{"shard_id":0,"namespace_id":0,"vector":[...],"k":5,"depth":2,
//!     "final_k":5,"max_graph_candidates":100,
//!     "max_nodes":null,"max_edges":null,"graph_weight":0.3}`
//! Outputs: `{"hits":[…],"seed_nodes":[…],"subgraph":{"nodes":[…],"edges":[…]}}`
//! Effects: `Counter("graphrag_queries", 1.0)` — Ephemeral
//!
//! `k` = retrieval_k (how many vector seeds); `final_k` = result cap (defaults to k);
//! `max_graph_candidates` = budget on graph-only candidates before final_k;
//! `max_nodes`/`max_edges` = Phase 5.4 BFS traversal budgets;
//! `graph_weight` = Phase 5.4 β coefficient for the combined reranking score.
use crate::effect::{Effect, EffectId, EffectPayload};
use crate::error::{EffectError, EffectResult};
use crate::task::{Task, TaskContext, TaskOutput};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GraphRagInputs {
    shard_id: u8,
    namespace_id: u16,
    vector: Vec<f32>,
    /// retrieval_k — vector seed count (serialised as "k" for task-level compat).
    k: u32,
    #[serde(default = "default_depth")]
    depth: u32,
    /// Maximum returned hits. None = no truncation.
    #[serde(default)]
    final_k: Option<u32>,
    /// Budget on graph-only candidates before final_k. 0 = unlimited.
    #[serde(default = "default_max_graph_candidates")]
    max_graph_candidates: u32,
    /// Phase 5.4: maximum nodes visited during BFS. None = unlimited.
    #[serde(default)]
    max_nodes: Option<u32>,
    /// Phase 5.4: maximum edges emitted during BFS. None = unlimited.
    #[serde(default)]
    max_edges: Option<u32>,
    /// Phase 5.4: β coefficient for combined reranking (0.0–1.0; default 0.3).
    #[serde(default = "default_graph_weight")]
    graph_weight: f32,
}

fn default_depth() -> u32 {
    2
}

fn default_max_graph_candidates() -> u32 {
    100
}

fn default_graph_weight() -> f32 {
    0.3
}

pub struct GraphRagTask;

#[async_trait]
impl Task for GraphRagTask {
    fn name(&self) -> &'static str {
        "graph_rag"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: GraphRagInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("GraphRagTask bad inputs: {e}")))?;

        let result = ctx
            .capabilities
            .kernel
            .graph_rag(
                inputs.shard_id,
                inputs.namespace_id,
                inputs.vector,
                inputs.k,
                inputs.depth,
                inputs.final_k,
                inputs.max_graph_candidates,
                inputs.max_nodes,
                inputs.max_edges,
                inputs.graph_weight,
            )
            .await?;

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "graphrag_queries".into(),
                    value: 1.0,
                },
            ))
            .await;

        let state_hash = ctx.capabilities.kernel.state_hash(inputs.shard_id);
        Ok(TaskOutput::with_value(result, state_hash))
    }
}
