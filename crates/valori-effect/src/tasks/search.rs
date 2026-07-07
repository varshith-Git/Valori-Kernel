// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! SearchTask — emits a read-only ReceiptFragment and a counter metric.
//!
//! Inputs:  `{"namespace_id": 0, "shard_id": 0, "k": 5}`
//! Outputs: `{"hits": [...], "state_hash_after": "..."}`
//! Effects: `Receipt(ReceiptFragment{ mutated: false })` — Durable
//!          `Counter("searches", 1.0)` — Ephemeral
//!
//! NOTE: The actual vector search is not performed here — this task emits the
//! provenance trail. In Phase A7 the TaskRunner threads real search results
//! from the engine into `TaskOutput`. This file defines the effect wiring.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::effect::{Effect, EffectId, EffectPayload, ReceiptFragment};
use crate::error::{EffectError, EffectResult};
use crate::task::{Task, TaskContext, TaskOutput};

#[derive(Debug, Deserialize)]
struct SearchInputs {
    #[allow(dead_code)]
    namespace_id: u16,
    shard_id: u8,
    #[allow(dead_code)]
    k: u32,
}

#[derive(Debug, Serialize)]
struct SearchOutput {
    state_hash_after: String,
}

pub struct SearchTask;

#[async_trait]
impl Task for SearchTask {
    fn name(&self) -> &'static str { "search" }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: SearchInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("SearchTask bad inputs: {e}")))?;

        let state_hash = ctx.capabilities.kernel.state_hash(inputs.shard_id);

        // Emit a read-only receipt fragment (Durable — proves the read happened).
        let frag = ReceiptFragment::read_only(ctx.topological_index, state_hash.clone());
        let receipt_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        ctx.bus.dispatch(Effect::durable(receipt_id, EffectPayload::Receipt(frag))).await?;

        // Emit a counter metric (Ephemeral).
        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 1);
        let _ = ctx.bus.dispatch(Effect::ephemeral(
            metric_id,
            EffectPayload::Counter { name: "searches".into(), value: 1.0 },
        )).await;

        let out = SearchOutput { state_hash_after: state_hash.clone() };
        Ok(TaskOutput::with_value(serde_json::to_value(out).map_err(EffectError::Serde)?, state_hash))
    }
}
