// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! InsertRecordTask — inserts one vector record via KernelCapability.
//!
//! Inputs:  `{"namespace_id": 0, "shard_id": 0, "values": [...], "text": null, "metadata": null, "tag": 0, "request_id": null}`
//! Outputs: `{"record_id": 42, "state_hash_after": "..."}`
//! Effects: `KernelWrite(KernelCommand)` — Durable
//!          `Counter("records_inserted", 1.0)` — Ephemeral
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::effect::{Effect, EffectId, EffectPayload, KernelCommand, KernelCommandBody};
use crate::error::{EffectError, EffectResult};
use crate::task::{Task, TaskContext, TaskOutput};

#[derive(Debug, Deserialize)]
struct InsertInputs {
    namespace_id: u16,
    shard_id: u8,
    values: Vec<f32>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    tag: u8,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct InsertOutput {
    record_id: u32,
    state_hash_after: String,
}

pub struct InsertRecordTask;

#[async_trait]
impl Task for InsertRecordTask {
    fn name(&self) -> &'static str { "insert_record" }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: InsertInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("InsertRecordTask bad inputs: {e}")))?;

        let request_id = inputs.request_id.unwrap_or_else(|| {
            let eid = EffectId::new(&ctx.execution_id, ctx.topological_index, 99);
            eid.to_hex()
        });

        let cmd = KernelCommand {
            shard_id: inputs.shard_id,
            namespace_id: inputs.namespace_id,
            body: KernelCommandBody::InsertRecord {
                values: inputs.values,
                text: inputs.text,
                metadata: inputs.metadata,
                tag: inputs.tag,
            },
            request_id,
        };

        let write_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let result = ctx.bus.dispatch(Effect::durable(write_id, EffectPayload::KernelWrite(cmd))).await?;

        let record_id = result.get("record_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let state_hash = result.get("state_hash").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 1);
        let _ = ctx.bus.dispatch(Effect::ephemeral(
            metric_id,
            EffectPayload::Counter { name: "records_inserted".into(), value: 1.0 },
        )).await;

        let out = InsertOutput { record_id, state_hash_after: state_hash.clone() };
        Ok(TaskOutput::with_value(serde_json::to_value(out).map_err(EffectError::Serde)?, state_hash))
    }
}
