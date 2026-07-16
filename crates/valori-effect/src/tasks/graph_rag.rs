// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! GraphRagTask — kNN vector search + subgraph expansion.
//!
//! Inputs:  `{"shard_id":0,"namespace_id":0,"vector":[...],"k":5,"depth":2}`
//! Outputs: `{"hits":[…],"seed_nodes":[…],"subgraph":{"nodes":[…],"edges":[…]}}`
//! Effects: `Counter("graphrag_queries", 1.0)` — Ephemeral
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
    k: u32,
    #[serde(default = "default_depth")]
    depth: u32,
}

fn default_depth() -> u32 {
    2
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
