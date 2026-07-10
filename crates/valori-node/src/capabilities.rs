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

    async fn save_snapshot(&self, _shard_id: u8, path: Option<&str>) -> Result<String, EffectError> {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        let eng = self.engine.write().await;
        eng.save_snapshot(path.map(std::path::Path::new))
            .map_err(|e| EffectError::Dispatch(format!("snapshot: {e}")))?;
        let hash = hash_state_blake3(&eng.state).iter().map(|b| format!("{:02x}", b)).collect();
        Ok(hash)
    }

    async fn graph_rag(
        &self,
        _shard_id: u8,
        namespace_id: u16,
        vector: Vec<f32>,
        k: u32,
        depth: u32,
    ) -> Result<serde_json::Value, EffectError> {
        let eng = self.engine.read().await;
        let hits = eng.search_l2_ns(&vector, k as usize, namespace_id)
            .map_err(|e| EffectError::Dispatch(format!("graph_rag search: {e}")))?;

        let mut seeds: Vec<u32> = Vec::new();
        let mut hits_out: Vec<serde_json::Value> = Vec::new();
        for (record_id, score) in &hits {
            let node_id = eng.record_to_node.get(record_id).copied();
            if let Some(nid) = node_id { seeds.push(nid); }
            let memory_id = format!("rec:{record_id}");
            let metadata = eng.metadata.get(&memory_id);
            hits_out.push(serde_json::json!({
                "memory_id": memory_id,
                "record_id": record_id,
                "score": score,
                "node_id": node_id,
                "metadata": metadata,
            }));
        }

        let (nodes, edges) = crate::graph_rag::expand_subgraph(&eng.state, &seeds, depth);
        Ok(serde_json::json!({
            "hits": hits_out,
            "seed_nodes": seeds,
            "subgraph": { "nodes": nodes, "edges": edges },
        }))
    }

    async fn memory_search(
        &self,
        _shard_id: u8,
        namespace_id: u16,
        vector: Vec<f32>,
        k: u32,
        decay_half_life_secs: Option<f64>,
        rerank: bool,
        query_text: Option<String>,
        metadata_filter: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, EffectError> {
        let use_rerank = rerank && query_text.is_some();
        let over_k = if use_rerank || metadata_filter.is_some() { k as usize * 10 } else { k as usize };
        let eng = self.engine.read().await;
        let hits = eng.search_l2_ns(&vector, over_k, namespace_id)
            .map_err(|e| EffectError::Dispatch(format!("memory_search: {e}")))?;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

        let mut results: Vec<serde_json::Value> = hits.iter().map(|(record_id, score)| {
            let memory_id = format!("rec:{record_id}");
            let metadata = eng.metadata.get(&memory_id);
            let (decay_factor, age_secs) = match decay_half_life_secs {
                Some(hl) if hl > 0.0 => {
                    let created = eng.created_at.get(record_id).copied().unwrap_or(now_secs);
                    let age_s = now_secs.saturating_sub(created);
                    (Some(crate::decay::decay_factor(age_s, hl as u64)), Some(age_s))
                }
                _ => (None, None),
            };
            serde_json::json!({
                "memory_id": memory_id,
                "record_id": record_id,
                "score": score,
                "metadata": metadata,
                "decay_factor": decay_factor,
                "age_secs": age_secs,
            })
        }).collect();

        // Metadata filter
        if let Some(ref mf) = metadata_filter {
            let mf_map = mf.as_object().cloned().unwrap_or_default();
            results.retain(|r| {
                r.get("metadata")
                    .map(|m| crate::api::matches_metadata_filter(m, &mf_map))
                    .unwrap_or(false)
            });
        }

        // Rerank
        if use_rerank {
            let qt = query_text.as_deref().unwrap_or("");
            let candidates: Vec<(u32, f32)> = results.iter()
                .filter_map(|r| Some((r["record_id"].as_u64()? as u32, r["score"].as_f64()? as f32)))
                .collect();
            let reranked = eng.reranker_rerank(qt, &vector, &candidates);
            let order: Vec<u32> = reranked.iter().map(|(id, _)| *id).collect();
            results.sort_by_key(|r| {
                let id = r["record_id"].as_u64().unwrap_or(u64::MAX) as u32;
                order.iter().position(|&rid| rid == id).unwrap_or(usize::MAX)
            });
        }

        results.truncate(k as usize);
        Ok(serde_json::Value::Array(results))
    }

    async fn community_detect(
        &self,
        _shard_id: u8,
        namespace_id: u16,
        max_iter: u32,
    ) -> Result<serde_json::Value, EffectError> {
        let mut eng = self.engine.write().await;
        let ns_id = if namespace_id == 0 { None } else { Some(namespace_id) };
        let raw = crate::community::label_propagation(&eng.state, ns_id, max_iter);
        let store = crate::community::build_community_store(&eng.state, raw);
        let community_count = store.community_count;
        let node_count = store.node_count;
        let receipt = store.receipt.clone();
        let communities: Vec<serde_json::Value> = store.members.iter()
            .map(|(&cid, members)| serde_json::json!({
                "community_id": cid,
                "member_count": members.len(),
            }))
            .collect();
        eng.resources.community_store = Some(store);
        Ok(serde_json::json!({
            "community_count": community_count,
            "node_count": node_count,
            "receipt": receipt,
            "communities": communities,
        }))
    }

    async fn community_search(
        &self,
        _shard_id: u8,
        _namespace_id: u16,
        vector: Vec<f32>,
        k: u32,
        _depth: u32,
        _drill_in: bool,
    ) -> Result<serde_json::Value, EffectError> {
        let eng = self.engine.read().await;
        let store = eng.resources.community_store.as_ref()
            .ok_or_else(|| EffectError::Dispatch(
                "community index not built — call community_detect first".into()
            ))?;
        let ranked = crate::community::rank_communities(store, &vector, k as usize);
        let total = store.centroids.len();
        let communities: Vec<serde_json::Value> = ranked.into_iter()
            .map(|(cid, score)| {
                let members = store.members.get(&cid).map(|v| v.as_slice()).unwrap_or(&[]);
                let sample: Vec<u32> = members.iter().copied().take(20).collect();
                serde_json::json!({
                    "community_id": cid,
                    "score": score,
                    "member_count": members.len(),
                    "sample_node_ids": sample,
                })
            })
            .collect();
        Ok(serde_json::json!({
            "communities": communities,
            "total_communities_searched": total,
        }))
    }

    async fn tree_build(&self, text: String, doc_name: String) -> Result<serde_json::Value, EffectError> {
        let tree = crate::tree_rag::TreeIndex::from_markdown(&text, &doc_name);
        let cache_key = self.engine.write().await.cache_tree(&text, tree.clone());
        Ok(serde_json::json!({
            "cache_key": cache_key,
            "doc_name": tree.doc_name,
            "node_count": tree.nodes.len(),
            "tree": serde_json::to_value(&tree).unwrap_or(serde_json::Value::Null),
        }))
    }

    async fn tree_query(
        &self,
        tree_json: serde_json::Value,
        query: String,
        k: u32,
        prev_hash: Option<String>,
    ) -> Result<serde_json::Value, EffectError> {
        let tree: crate::tree_rag::TreeIndex = serde_json::from_value(tree_json)
            .map_err(|e| EffectError::Dispatch(format!("tree_query: bad tree JSON: {e}")))?;
        let prev = prev_hash.as_deref().unwrap_or(crate::tree_rag::GENESIS);
        let result = tree.answer(&query, k.max(1) as usize, prev);
        serde_json::to_value(result).map_err(EffectError::Serde)
    }

    async fn tree_hybrid(
        &self,
        _shard_id: u8,
        namespace_id: u16,
        query: String,
        k: u32,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, EffectError> {
        use crate::tree_rag::{HybridHit, HybridResponse, GENESIS};

        let tree_json = params.get("tree").cloned().unwrap_or(serde_json::Value::Null);
        let cache_key = params.get("cache_key").and_then(|v| v.as_str()).map(|s| s.to_string());
        let tree_weight = params.get("tree_weight").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
        let prev = params.get("prev_hash").and_then(|v| v.as_str())
            .unwrap_or(GENESIS).to_string();

        let tree: crate::tree_rag::TreeIndex = if !tree_json.is_null() {
            serde_json::from_value(tree_json)
                .map_err(|e| EffectError::Dispatch(format!("tree_hybrid: bad tree: {e}")))?
        } else if let Some(ref key) = cache_key {
            self.engine.read().await.get_cached_tree(key).cloned()
                .ok_or_else(|| EffectError::Dispatch(format!("tree not in cache: {key}")))?
        } else {
            return Err(EffectError::Dispatch("tree_hybrid: provide 'tree' or 'cache_key' in params".into()));
        };

        let tw = tree_weight.clamp(0.0, 1.0);
        let vw = 1.0 - tw;
        let k_usize = k as usize;
        let tree_ranked = tree.rank_nodes_normalized(&query, k_usize * 2);
        let tree_hit_count = tree_ranked.len();

        let mut hits: Vec<HybridHit> = tree_ranked.iter().map(|(nid, norm_score)| {
            let n = &tree.nodes[nid];
            HybridHit {
                source: "tree".into(),
                score: tw as f64 * norm_score,
                node_id: Some(nid.clone()),
                title: Some(n.title.clone()),
                breadcrumb: Some(tree.breadcrumb(nid)),
                text: Some(n.own_text.clone()),
                lines: Some([n.start_line, n.end_line]),
                record_id: None,
                distance: None,
            }
        }).collect();

        let mut vector_hit_count = 0usize;
        let mut reasoning_extra = String::new();

        if vw > 0.0 {
            let eng = self.engine.read().await;
            let query_vec: Vec<f32> = params.get("vector")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_f64()).map(|f| f as f32).collect())
                .unwrap_or_default();
            if !query_vec.is_empty() {
                if let Ok(vec_hits) = eng.search_l2_ns(&query_vec, k_usize * 2, namespace_id) {
                    let max_dist = vec_hits.iter().map(|(_, d)| *d).fold(0f32, f32::max).max(1e-9);
                    for (rid, dist) in &vec_hits {
                        let sim = ((1.0 - dist / max_dist) as f64).clamp(0.0, 1.0);
                        hits.push(HybridHit {
                            source: "vector".into(),
                            score: vw as f64 * sim,
                            node_id: None, title: None, breadcrumb: None, text: None, lines: None,
                            record_id: Some(*rid),
                            distance: Some(*dist),
                        });
                        vector_hit_count += 1;
                    }
                }
            } else {
                reasoning_extra = " (no vector provided — vector path skipped)".into();
            }
        }

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(k_usize);

        let tree_answer = if tree_hit_count > 0 {
            Some(tree.answer(&query, k_usize.min(tree_hit_count), &prev))
        } else {
            None
        };

        let reasoning = format!(
            "{} tree hits, {} vector hits{}",
            tree_hit_count, vector_hit_count, reasoning_extra
        );

        let resp = HybridResponse { query, hits, tree_hit_count, vector_hit_count, tree_answer, reasoning };
        serde_json::to_value(&resp).map_err(|e| EffectError::Dispatch(format!("tree_hybrid serialize: {e}")))
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
