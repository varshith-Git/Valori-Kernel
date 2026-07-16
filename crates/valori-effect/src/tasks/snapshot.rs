// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! SnapshotArtifactTask — persists current kernel state to disk.
//!
//! Inputs:  `{"shard_id": 0, "path": null}`
//! Outputs: `{"state_hash": "..."}`
//! Effects: `Counter("snapshots_saved", 1.0)` — Ephemeral
use crate::effect::{Effect, EffectId, EffectPayload};
use crate::error::{EffectError, EffectResult};
use crate::task::{Task, TaskContext, TaskOutput};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SnapshotInputs {
    shard_id: u8,
    #[serde(default)]
    path: Option<String>,
}

pub struct SnapshotArtifactTask;

#[async_trait]
impl Task for SnapshotArtifactTask {
    fn name(&self) -> &'static str {
        "snapshot_artifact"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: SnapshotInputs = serde_json::from_str(inputs_json).map_err(|e| {
            EffectError::TaskFailed(format!("SnapshotArtifactTask bad inputs: {e}"))
        })?;

        let state_hash = ctx
            .capabilities
            .kernel
            .save_snapshot(inputs.shard_id, inputs.path.as_deref())
            .await?;

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "snapshots_saved".into(),
                    value: 1.0,
                },
            ))
            .await;

        Ok(TaskOutput::with_value(
            serde_json::json!({ "state_hash": state_hash }),
            state_hash,
        ))
    }
}
