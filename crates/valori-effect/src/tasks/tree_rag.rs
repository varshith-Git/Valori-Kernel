// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! TreeBuildTask, TreeQueryTask, TreeHybridTask.
use crate::effect::{Effect, EffectId, EffectPayload};
use crate::error::{EffectError, EffectResult};
use crate::task::{Task, TaskContext, TaskOutput};
use async_trait::async_trait;
use serde::Deserialize;

// ── TreeBuildTask ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TreeBuildInputs {
    text: String,
    #[serde(default = "default_doc_name")]
    doc_name: String,
}

fn default_doc_name() -> String {
    "document".into()
}

pub struct TreeBuildTask;

#[async_trait]
impl Task for TreeBuildTask {
    fn name(&self) -> &'static str {
        "tree_build"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: TreeBuildInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("TreeBuildTask bad inputs: {e}")))?;

        let result = ctx
            .capabilities
            .kernel
            .tree_build(inputs.text, inputs.doc_name)
            .await?;

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "tree_builds".into(),
                    value: 1.0,
                },
            ))
            .await;

        let state_hash = ctx.capabilities.kernel.state_hash(0);
        Ok(TaskOutput::with_value(result, state_hash))
    }
}

// ── TreeQueryTask ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TreeQueryInputs {
    tree: serde_json::Value,
    query: String,
    #[serde(default = "default_k")]
    k: u32,
    #[serde(default)]
    prev_hash: Option<String>,
}

fn default_k() -> u32 {
    5
}

pub struct TreeQueryTask;

#[async_trait]
impl Task for TreeQueryTask {
    fn name(&self) -> &'static str {
        "tree_query"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: TreeQueryInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("TreeQueryTask bad inputs: {e}")))?;

        let result = ctx
            .capabilities
            .kernel
            .tree_query(inputs.tree, inputs.query, inputs.k, inputs.prev_hash)
            .await?;

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "tree_queries".into(),
                    value: 1.0,
                },
            ))
            .await;

        let state_hash = ctx.capabilities.kernel.state_hash(0);
        Ok(TaskOutput::with_value(result, state_hash))
    }
}

// ── TreeHybridTask ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TreeHybridInputs {
    shard_id: u8,
    namespace_id: u16,
    query: String,
    k: u32,
    params: serde_json::Value,
}

pub struct TreeHybridTask;

#[async_trait]
impl Task for TreeHybridTask {
    fn name(&self) -> &'static str {
        "tree_hybrid"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: TreeHybridInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("TreeHybridTask bad inputs: {e}")))?;

        let result = ctx
            .capabilities
            .kernel
            .tree_hybrid(
                inputs.shard_id,
                inputs.namespace_id,
                inputs.query,
                inputs.k,
                inputs.params,
            )
            .await?;

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "tree_hybrid_queries".into(),
                    value: 1.0,
                },
            ))
            .await;

        let state_hash = ctx.capabilities.kernel.state_hash(inputs.shard_id);
        Ok(TaskOutput::with_value(result, state_hash))
    }
}
