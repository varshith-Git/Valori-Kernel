// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! CommunityDetectTask and CommunitySearchTask.
use crate::effect::{Effect, EffectId, EffectPayload};
use crate::error::{EffectError, EffectResult};
use crate::task::{Task, TaskContext, TaskOutput};
use async_trait::async_trait;
use serde::Deserialize;

// ── CommunityDetectTask ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CommunityDetectInputs {
    shard_id: u8,
    namespace_id: u16,
    #[serde(default = "default_max_iter")]
    max_iter: u32,
}

fn default_max_iter() -> u32 {
    10
}

pub struct CommunityDetectTask;

#[async_trait]
impl Task for CommunityDetectTask {
    fn name(&self) -> &'static str {
        "community_detect"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: CommunityDetectInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("CommunityDetectTask bad inputs: {e}")))?;

        let result = ctx
            .capabilities
            .kernel
            .community_detect(inputs.shard_id, inputs.namespace_id, inputs.max_iter)
            .await?;

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "community_detections".into(),
                    value: 1.0,
                },
            ))
            .await;

        let state_hash = ctx.capabilities.kernel.state_hash(inputs.shard_id);
        Ok(TaskOutput::with_value(result, state_hash))
    }
}

// ── CommunitySearchTask ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CommunitySearchInputs {
    shard_id: u8,
    namespace_id: u16,
    vector: Vec<f32>,
    k: u32,
    #[serde(default = "default_depth")]
    depth: u32,
    #[serde(default)]
    drill_in: bool,
}

fn default_depth() -> u32 {
    1
}

pub struct CommunitySearchTask;

#[async_trait]
impl Task for CommunitySearchTask {
    fn name(&self) -> &'static str {
        "community_search"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: CommunitySearchInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("CommunitySearchTask bad inputs: {e}")))?;

        let result = ctx
            .capabilities
            .kernel
            .community_search(
                inputs.shard_id,
                inputs.namespace_id,
                inputs.vector,
                inputs.k,
                inputs.depth,
                inputs.drill_in,
            )
            .await?;

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "community_searches".into(),
                    value: 1.0,
                },
            ))
            .await;

        let state_hash = ctx.capabilities.kernel.state_hash(inputs.shard_id);
        Ok(TaskOutput::with_value(result, state_hash))
    }
}
