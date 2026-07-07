// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Concrete capability implementations backed by the live valori-node subsystems.
//!
//! - `EngineKernelCapability` wraps `SharedEngine` (standalone path).
//! - `RaftKernelCapability` wraps the cluster Raft + shards (cluster path).
//! - `HttpEmbedCapability` wraps `EmbedConfig` + `reqwest::Client`.
use std::sync::Arc;
use async_trait::async_trait;
use bytes::Bytes;

use valori_effect::capability::{
    Capability, EmbedCapability, HttpCapability, KernelCapability,
};
use valori_effect::effect::KernelCommandBody;
use valori_effect::error::EffectError;

use crate::embedder::{embed_batch, EmbedConfig};
use crate::server::SharedEngine;

// ── EngineKernelCapability ────────────────────────────────────────────────────

/// `KernelCapability` backed by the standalone `SharedEngine`.
pub struct EngineKernelCapability {
    engine: SharedEngine,
    shard_count: u8,
}

impl EngineKernelCapability {
    pub fn new(engine: SharedEngine, shard_count: u8) -> Self {
        EngineKernelCapability { engine, shard_count }
    }
}

impl Capability for EngineKernelCapability {
    fn name(&self) -> &'static str { "kernel_engine" }
    fn is_available(&self) -> bool { true }
}

#[async_trait]
impl KernelCapability for EngineKernelCapability {
    fn shard_count(&self) -> u8 { self.shard_count }

    async fn apply_command(
        &self,
        _shard_id: u8,
        namespace_id: u16,
        body: &KernelCommandBody,
        _request_id: &str,
    ) -> Result<serde_json::Value, EffectError> {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        match body {
            KernelCommandBody::InsertRecord { values, text, .. } => {
                let mut eng = self.engine.write().await;
                let record_id = eng.insert_record_from_f32_ns(values, namespace_id)
                    .map_err(|e| {
                        if let crate::errors::EngineError::Kernel(valori_kernel::error::KernelError::CapacityExceeded) = &e {
                            EffectError::Capacity("record pool full".into())
                        } else {
                            EffectError::Dispatch(format!("kernel insert: {e}"))
                        }
                    })?;
                if let Some(t) = text {
                    eng.reranker_insert(record_id, t);
                }
                let hash = hash_state_blake3(&eng.state)
                    .iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "record_id": record_id, "state_hash": hash }))
            }
            KernelCommandBody::SoftDeleteRecord { record_id } => {
                let mut eng = self.engine.write().await;
                eng.soft_delete_record(*record_id)
                    .map_err(|e| EffectError::Dispatch(format!("kernel soft_delete: {e}")))?;
                let hash = hash_state_blake3(&eng.state)
                    .iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "state_hash": hash }))
            }
            KernelCommandBody::HardDeleteRecord { record_id } => {
                let mut eng = self.engine.write().await;
                eng.delete_record(*record_id)
                    .map_err(|e| EffectError::Dispatch(format!("kernel delete: {e}")))?;
                let hash = hash_state_blake3(&eng.state)
                    .iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "state_hash": hash }))
            }
            KernelCommandBody::CreateNode { kind, record_id } => {
                let mut eng = self.engine.write().await;
                let node_id = eng.create_node_for_record(*record_id, *kind, namespace_id)
                    .map_err(|e| EffectError::Dispatch(format!("kernel create_node: {e}")))?;
                let hash = hash_state_blake3(&eng.state)
                    .iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "node_id": node_id, "state_hash": hash }))
            }
            KernelCommandBody::CreateEdge { from, to, kind } => {
                let mut eng = self.engine.write().await;
                let edge_id = eng.create_edge(*from, *to, *kind)
                    .map_err(|e| EffectError::Dispatch(format!("kernel create_edge: {e}")))?;
                let hash = hash_state_blake3(&eng.state)
                    .iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "edge_id": edge_id, "state_hash": hash }))
            }
        }
    }

    fn state_hash(&self, _shard_id: u8) -> String {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        if let Ok(eng) = self.engine.try_read() {
            hash_state_blake3(&eng.state).iter().map(|b| format!("{:02x}", b)).collect()
        } else {
            "0".repeat(64)
        }
    }
}

// ── RaftKernelCapability ──────────────────────────────────────────────────────

/// `KernelCapability` backed by the cluster Raft path.
///
/// Holds all shard handles keyed by `ShardId` so it can route writes to the
/// correct shard based on the `shard_id` parameter.
pub struct RaftKernelCapability {
    shards: Arc<std::collections::BTreeMap<valori_consensus::types::ShardId, crate::cluster::ShardHandle>>,
    #[allow(dead_code)]
    sm: valori_consensus::ValoriStateMachine,
    pub shard_count: u8,
}

impl RaftKernelCapability {
    pub fn new(
        shards: Arc<std::collections::BTreeMap<valori_consensus::types::ShardId, crate::cluster::ShardHandle>>,
        sm: valori_consensus::ValoriStateMachine,
        shard_count: u8,
    ) -> Self {
        RaftKernelCapability { shards, sm, shard_count }
    }
}

impl Capability for RaftKernelCapability {
    fn name(&self) -> &'static str { "kernel_raft" }
    fn is_available(&self) -> bool { true }
}

#[async_trait]
impl KernelCapability for RaftKernelCapability {
    fn shard_count(&self) -> u8 { self.shard_count }

    async fn apply_command(
        &self,
        shard_id: u8,
        namespace_id: u16,
        body: &KernelCommandBody,
        request_id: &str,
    ) -> Result<serde_json::Value, EffectError> {
        use valori_kernel::event::KernelEvent;
        use valori_kernel::types::vector::FxpVector;
        use valori_kernel::types::scalar::FxpScalar;
        use valori_kernel::config::SCALE;
        use valori_consensus::types::{ClientRequest, ShardId, CURRENT_SCHEMA_VERSION};

        let req_id_bytes: Option<[u8; 16]> = {
            let bytes = request_id.as_bytes();
            if bytes.len() >= 16 {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes[..16]);
                Some(arr)
            } else {
                None
            }
        };

        let sid = ShardId(shard_id as u32);
        let shard = self.shards.get(&sid)
            .ok_or_else(|| EffectError::Dispatch(format!("shard {shard_id} not found")))?;

        match body {
            KernelCommandBody::InsertRecord { values, metadata: _, tag, .. } => {
                let fxp: Result<Vec<_>, _> = values.iter().map(|&v| {
                    if v > 32767.99 || v < -32768.0 {
                        Err(EffectError::TaskFailed("value out of Q16.16 range".into()))
                    } else {
                        Ok(FxpScalar((v * SCALE as f32) as i32))
                    }
                }).collect();
                let vector = FxpVector { data: fxp? };

                let cr = ClientRequest {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id,
                    event: KernelEvent::AutoInsertRecord {
                        vector,
                        metadata: None,
                        tag: *tag as u64,
                    },
                    request_id: req_id_bytes,
                };

                let resp = shard.raft.client_write(cr).await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let record_id = resp.data.allocated_record_id.unwrap_or(0);
                let hash = resp.data.state_hash.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "record_id": record_id, "log_index": resp.data.log_index, "state_hash": hash }))
            }
            KernelCommandBody::SoftDeleteRecord { record_id } => {
                let cr = ClientRequest {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id,
                    event: KernelEvent::SoftDeleteRecord { id: valori_kernel::types::id::RecordId(*record_id) },
                    request_id: req_id_bytes,
                };
                let resp = shard.raft.client_write(cr).await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let hash = resp.data.state_hash.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "state_hash": hash }))
            }
            KernelCommandBody::HardDeleteRecord { record_id } => {
                let cr = ClientRequest {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id,
                    event: KernelEvent::DeleteRecord { id: valori_kernel::types::id::RecordId(*record_id) },
                    request_id: req_id_bytes,
                };
                let resp = shard.raft.client_write(cr).await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let hash = resp.data.state_hash.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "state_hash": hash }))
            }
            KernelCommandBody::CreateNode { kind, record_id } => {
                let cr = ClientRequest {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id,
                    event: KernelEvent::AutoCreateNode {
                        kind: valori_kernel::types::enums::NodeKind::from_u8(*kind).unwrap_or(valori_kernel::types::enums::NodeKind::Document),
                        record: record_id.map(valori_kernel::types::id::RecordId),
                    },
                    request_id: req_id_bytes,
                };
                let resp = shard.raft.client_write(cr).await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let node_id = resp.data.allocated_node_id.unwrap_or(0);
                let hash = resp.data.state_hash.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "node_id": node_id, "log_index": resp.data.log_index, "state_hash": hash }))
            }
            KernelCommandBody::CreateEdge { from, to, kind } => {
                let cr = ClientRequest {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id,
                    event: KernelEvent::AutoCreateEdge {
                        from: valori_kernel::types::id::NodeId(*from),
                        to: valori_kernel::types::id::NodeId(*to),
                        kind: valori_kernel::types::enums::EdgeKind::from_u8(*kind).unwrap_or(valori_kernel::types::enums::EdgeKind::RefersTo),
                    },
                    request_id: req_id_bytes,
                };
                let resp = shard.raft.client_write(cr).await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let edge_id = resp.data.allocated_edge_id.unwrap_or(0);
                let hash = resp.data.state_hash.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                Ok(serde_json::json!({ "edge_id": edge_id, "log_index": resp.data.log_index, "state_hash": hash }))
            }
        }
    }

    fn state_hash(&self, _shard_id: u8) -> String {
        "0".repeat(64)
    }
}

// ── NoRaftKernelCapability (placeholder for tests) ────────────────────────────

pub struct NoRaftKernelCapability { pub shard_count: u8 }

impl Capability for NoRaftKernelCapability {
    fn name(&self) -> &'static str { "kernel_raft_stub" }
    fn is_available(&self) -> bool { false }
}

#[async_trait]
impl KernelCapability for NoRaftKernelCapability {
    fn shard_count(&self) -> u8 { self.shard_count }
    async fn apply_command(&self, _: u8, _: u16, _: &KernelCommandBody, _: &str) -> Result<serde_json::Value, EffectError> {
        Err(EffectError::CapabilityUnavailable("kernel_raft_stub"))
    }
    fn state_hash(&self, _shard_id: u8) -> String { "0".repeat(64) }
}

// ── HttpEmbedCapability ───────────────────────────────────────────────────────

pub struct HttpEmbedCapability {
    config: Arc<EmbedConfig>,
    client: reqwest::Client,
}

impl HttpEmbedCapability {
    pub fn new(config: EmbedConfig, client: reqwest::Client) -> Self {
        HttpEmbedCapability { config: Arc::new(config), client }
    }
}

impl Capability for HttpEmbedCapability {
    fn name(&self) -> &'static str { "embed_http" }
    fn is_available(&self) -> bool { true }
}

#[async_trait]
impl EmbedCapability for HttpEmbedCapability {
    fn model_name(&self) -> &str { &self.config.model }
    fn dim(&self) -> usize { 0 }

    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, EffectError> {
        embed_batch(&texts, &self.config, &self.client).await
            .map_err(|e| EffectError::Dispatch(e.to_string()))
    }
}

// ── PassthroughHttpCapability ─────────────────────────────────────────────────

pub struct PassthroughHttpCapability {
    client: reqwest::Client,
}

impl PassthroughHttpCapability {
    pub fn new(client: reqwest::Client) -> Self { PassthroughHttpCapability { client } }
}

impl Capability for PassthroughHttpCapability {
    fn name(&self) -> &'static str { "http_passthrough" }
    fn is_available(&self) -> bool { true }
}

#[async_trait]
impl HttpCapability for PassthroughHttpCapability {
    async fn get(&self, url: &str) -> Result<Bytes, EffectError> {
        self.client.get(url)
            .send().await
            .map_err(|e| EffectError::Dispatch(e.to_string()))?
            .bytes().await
            .map_err(|e| EffectError::Dispatch(e.to_string()))
    }
}

// ── CapabilityRegistryBuilder ─────────────────────────────────────────────────

/// Builds a `CapabilityRegistry` for standalone mode.
pub struct CapabilityRegistryBuilder {
    engine: SharedEngine,
    shard_count: u8,
    embed_config: Option<EmbedConfig>,
    http_client: reqwest::Client,
}

impl CapabilityRegistryBuilder {
    pub fn new(engine: SharedEngine, shard_count: u8, http_client: reqwest::Client) -> Self {
        CapabilityRegistryBuilder { engine, shard_count, embed_config: None, http_client }
    }

    pub fn with_embed(mut self, cfg: EmbedConfig) -> Self {
        self.embed_config = Some(cfg);
        self
    }

    pub fn build(self) -> valori_effect::capability::CapabilityRegistry {
        use valori_effect::capability::CapabilityRegistry;

        let kernel: Arc<dyn KernelCapability> = Arc::new(
            EngineKernelCapability::new(self.engine, self.shard_count)
        );

        let embed = self.embed_config.map(|cfg| {
            let cap: Arc<dyn EmbedCapability> = Arc::new(
                HttpEmbedCapability::new(cfg, self.http_client.clone())
            );
            cap
        });

        let http: Option<Arc<dyn HttpCapability>> = Some(Arc::new(
            PassthroughHttpCapability::new(self.http_client)
        ));

        CapabilityRegistry {
            kernel,
            embed,
            llm: None,
            storage: None,
            http,
            proof: None,
            scheduler: None,
        }
    }

    pub fn build_cluster(
        shards: Arc<std::collections::BTreeMap<valori_consensus::types::ShardId, crate::cluster::ShardHandle>>,
        sm: valori_consensus::ValoriStateMachine,
        shard_count: u8,
        embed_config: Option<EmbedConfig>,
        http_client: reqwest::Client,
    ) -> valori_effect::capability::CapabilityRegistry {
        use valori_effect::capability::CapabilityRegistry;

        let kernel: Arc<dyn KernelCapability> = Arc::new(
            RaftKernelCapability::new(shards, sm, shard_count)
        );

        let embed = embed_config.map(|cfg| {
            let cap: Arc<dyn EmbedCapability> = Arc::new(
                HttpEmbedCapability::new(cfg, http_client.clone())
            );
            cap
        });

        let http: Option<Arc<dyn HttpCapability>> = Some(Arc::new(
            PassthroughHttpCapability::new(http_client)
        ));

        CapabilityRegistry {
            kernel,
            embed,
            llm: None,
            storage: None,
            http,
            proof: None,
            scheduler: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_effect::capability::CapabilityRegistry;
    use valori_effect::NoOpKernelCapability;

    #[test]
    fn builder_no_embed_yields_none() {
        let reg = CapabilityRegistry {
            kernel: Arc::new(NoOpKernelCapability { shard_count: 1 }),
            embed: None, llm: None, storage: None, http: None, proof: None, scheduler: None,
        };
        assert!(reg.embed().is_err());
        assert!(reg.llm().is_err());
    }
}
