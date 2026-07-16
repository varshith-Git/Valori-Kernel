// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! EmbedTask — calls `EmbedCapability` to convert text chunks into vectors.
//!
//! Inputs:  `{"texts": ["chunk 1", "chunk 2", ...]}`
//! Outputs: `{"embeddings": [[...], [...], ...]}`
//! Effects: `Counter("embed_calls", texts.len())`   — ephemeral
use crate::effect::{Effect, EffectId, EffectPayload};
use crate::error::{EffectError, EffectResult};
use crate::task::{Task, TaskContext, TaskOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct EmbedInputs {
    texts: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EmbedOutputs {
    embeddings: Vec<Vec<f32>>,
    model: String,
}

pub struct EmbedTask;

#[async_trait]
impl Task for EmbedTask {
    fn name(&self) -> &'static str {
        "embed"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: EmbedInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("EmbedTask bad inputs: {e}")))?;

        let embed_cap = ctx.capabilities.embed()?;

        if inputs.texts.is_empty() {
            return Ok(TaskOutput::empty(ctx.capabilities.kernel.state_hash(0)));
        }

        // Budget check.
        if ctx.budget.max_embed_calls > 0 && inputs.texts.len() as u32 > ctx.budget.max_embed_calls
        {
            return Err(EffectError::BudgetExceeded("max_embed_calls"));
        }

        let embeddings = embed_cap.embed(inputs.texts).await?;
        let model = embed_cap.model_name().to_string();

        // Emit metrics (ephemeral — fire-and-forget).
        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "embed_calls".into(),
                    value: embeddings.len() as f64,
                },
            ))
            .await;

        let out = EmbedOutputs { embeddings, model };
        let json = serde_json::to_value(out).map_err(EffectError::Serde)?;
        Ok(TaskOutput::with_value(
            json,
            ctx.capabilities.kernel.state_hash(0),
        ))
    }
}
