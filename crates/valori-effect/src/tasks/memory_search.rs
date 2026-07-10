// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! MemorySearchTask — vector search with optional decay, rerank, and metadata filter.
//!
//! Inputs:  `{"shard_id":0,"namespace_id":0,"vector":[...],"k":5,...}`
//! Outputs: `[{"memory_id":…,"record_id":…,"score":…,"metadata":…}]`
//! Effects: `Counter("memory_searches", 1.0)` — Ephemeral
use async_trait::async_trait;
use serde::Deserialize;
use crate::effect::{Effect, EffectId, EffectPayload};
use crate::error::{EffectError, EffectResult};
use crate::task::{Task, TaskContext, TaskOutput};

#[derive(Debug, Deserialize)]
struct MemorySearchInputs {
    shard_id: u8,
    namespace_id: u16,
    vector: Vec<f32>,
    k: u32,
    #[serde(default)]
    decay_half_life_secs: Option<f64>,
    #[serde(default)]
    rerank: bool,
    #[serde(default)]
    query_text: Option<String>,
    #[serde(default)]
    metadata_filter: Option<serde_json::Value>,
}

pub struct MemorySearchTask;

#[async_trait]
impl Task for MemorySearchTask {
    fn name(&self) -> &'static str { "memory_search" }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: MemorySearchInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("MemorySearchTask bad inputs: {e}")))?;

        let results = ctx.capabilities.kernel.memory_search(
            inputs.shard_id,
            inputs.namespace_id,
            inputs.vector,
            inputs.k,
            inputs.decay_half_life_secs,
            inputs.rerank,
            inputs.query_text,
            inputs.metadata_filter,
        ).await?;

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let _ = ctx.bus.dispatch(Effect::ephemeral(
            metric_id,
            EffectPayload::Counter { name: "memory_searches".into(), value: 1.0 },
        )).await;

        let state_hash = ctx.capabilities.kernel.state_hash(inputs.shard_id);
        Ok(TaskOutput::with_value(results, state_hash))
    }
}
