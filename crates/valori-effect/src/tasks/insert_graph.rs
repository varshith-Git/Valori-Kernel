// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! InsertNodeTask and InsertEdgeTask — inserts graph nodes and edges via KernelCapability.
//!
//! # InsertNodeTask
//! Inputs:  `{"namespace_id": 0, "shard_id": 0, "kind": 0, "record_id": null, "request_id": null}`
//! Outputs: `{"node_id": 1, "state_hash_after": "..."}`
//! Effects: `KernelWrite(KernelCommand)` — Durable
//!          `Counter("nodes_created", 1.0)` — Ephemeral
//!
//! # InsertEdgeTask
//! Inputs:  `{"namespace_id": 0, "shard_id": 0, "from": 1, "to": 2, "kind": 6, "request_id": null}`
//! Outputs: `{"edge_id": 1, "state_hash_after": "..."}`
//! Effects: `KernelWrite(KernelCommand)` — Durable
//!          `Counter("edges_created", 1.0)` — Ephemeral

use crate::effect::{Effect, EffectId, EffectPayload, KernelCommand, KernelCommandBody};
use crate::error::{EffectError, EffectResult};
use crate::task::{Task, TaskContext, TaskOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ── InsertNodeTask ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct InsertNodeInputs {
    namespace_id: u16,
    shard_id: u8,
    kind: u8,
    #[serde(default)]
    record_id: Option<u32>,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct InsertNodeOutput {
    node_id: u32,
    state_hash_after: String,
}

pub struct InsertNodeTask;

#[async_trait]
impl Task for InsertNodeTask {
    fn name(&self) -> &'static str {
        "insert_node"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: InsertNodeInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("InsertNodeTask bad inputs: {e}")))?;

        let request_id = inputs.request_id.unwrap_or_else(|| {
            let eid = EffectId::new(&ctx.execution_id, ctx.topological_index, 99);
            eid.to_hex()
        });

        let cmd = KernelCommand {
            shard_id: inputs.shard_id,
            namespace_id: inputs.namespace_id,
            body: KernelCommandBody::CreateNode {
                kind: inputs.kind,
                record_id: inputs.record_id,
            },
            request_id,
        };

        let write_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let result = ctx
            .bus
            .dispatch(Effect::durable(write_id, EffectPayload::KernelWrite(cmd)))
            .await?;

        let node_id = result.get("node_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let state_hash = result
            .get("state_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 1);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "nodes_created".into(),
                    value: 1.0,
                },
            ))
            .await;

        let out = InsertNodeOutput {
            node_id,
            state_hash_after: state_hash.clone(),
        };
        Ok(TaskOutput::with_value(
            serde_json::to_value(out).map_err(EffectError::Serde)?,
            state_hash,
        ))
    }
}

// ── InsertEdgeTask ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct InsertEdgeInputs {
    namespace_id: u16,
    shard_id: u8,
    from: u32,
    to: u32,
    kind: u8,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct InsertEdgeOutput {
    edge_id: u32,
    state_hash_after: String,
}

pub struct InsertEdgeTask;

#[async_trait]
impl Task for InsertEdgeTask {
    fn name(&self) -> &'static str {
        "insert_edge"
    }

    async fn run(
        &self,
        inputs_json: &str,
        _predecessor_outputs: &[Option<TaskOutput>],
        ctx: &TaskContext,
    ) -> EffectResult<TaskOutput> {
        let inputs: InsertEdgeInputs = serde_json::from_str(inputs_json)
            .map_err(|e| EffectError::TaskFailed(format!("InsertEdgeTask bad inputs: {e}")))?;

        let request_id = inputs.request_id.unwrap_or_else(|| {
            let eid = EffectId::new(&ctx.execution_id, ctx.topological_index, 99);
            eid.to_hex()
        });

        let cmd = KernelCommand {
            shard_id: inputs.shard_id,
            namespace_id: inputs.namespace_id,
            body: KernelCommandBody::CreateEdge {
                from: inputs.from,
                to: inputs.to,
                kind: inputs.kind,
            },
            request_id,
        };

        let write_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 0);
        let result = ctx
            .bus
            .dispatch(Effect::durable(write_id, EffectPayload::KernelWrite(cmd)))
            .await?;

        let edge_id = result.get("edge_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let state_hash = result
            .get("state_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let metric_id = EffectId::new(&ctx.execution_id, ctx.topological_index, 1);
        let _ = ctx
            .bus
            .dispatch(Effect::ephemeral(
                metric_id,
                EffectPayload::Counter {
                    name: "edges_created".into(),
                    value: 1.0,
                },
            ))
            .await;

        let out = InsertEdgeOutput {
            edge_id,
            state_hash_after: state_hash.clone(),
        };
        Ok(TaskOutput::with_value(
            serde_json::to_value(out).map_err(EffectError::Serde)?,
            state_hash,
        ))
    }
}
