// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Concrete capability implementations backed by the live valori-node subsystems.
//!
//! - `EngineKernelCapability` wraps `SharedEngine` (standalone path).
//! - `RaftKernelCapability` wraps the cluster Raft + shards (cluster path).
//! - `HttpEmbedCapability` wraps `EmbedConfig` + `reqwest::Client`.
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;

use valori_effect::capability::{Capability, EmbedCapability, HttpCapability, KernelCapability};
use valori_effect::effect::KernelCommandBody;
use valori_effect::error::EffectError;

use crate::server::SharedEngine;
use valori_ingest::{embed_batch, EmbedConfig};

// ── EngineKernelCapability ────────────────────────────────────────────────────

/// `KernelCapability` backed by the standalone `SharedEngine`.
pub struct EngineKernelCapability {
    engine: SharedEngine,
    shard_count: u8,
}

impl EngineKernelCapability {
    pub fn new(engine: SharedEngine, shard_count: u8) -> Self {
        EngineKernelCapability {
            engine,
            shard_count,
        }
    }
}

impl Capability for EngineKernelCapability {
    fn name(&self) -> &'static str {
        "kernel_engine"
    }
    fn is_available(&self) -> bool {
        true
    }
}

#[async_trait]
impl KernelCapability for EngineKernelCapability {
    fn shard_count(&self) -> u8 {
        self.shard_count
    }

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
                let record_id = eng
                    .insert_record_from_f32_ns(values, namespace_id)
                    .map_err(|e| {
                        if let crate::errors::EngineError::Kernel(
                            valori_kernel::error::KernelError::CapacityExceeded,
                        ) = &e
                        {
                            EffectError::Capacity("record pool full".into())
                        } else {
                            EffectError::Dispatch(format!("kernel insert: {e}"))
                        }
                    })?;
                if let Some(t) = text {
                    eng.reranker_insert(record_id, t);
                }
                let hash = hash_state_blake3(&eng.state)
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(serde_json::json!({ "record_id": record_id, "state_hash": hash }))
            }
            KernelCommandBody::SoftDeleteRecord { record_id } => {
                let mut eng = self.engine.write().await;
                eng.soft_delete_record(*record_id)
                    .map_err(|e| EffectError::Dispatch(format!("kernel soft_delete: {e}")))?;
                let hash = hash_state_blake3(&eng.state)
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(serde_json::json!({ "state_hash": hash }))
            }
            KernelCommandBody::HardDeleteRecord { record_id } => {
                let mut eng = self.engine.write().await;
                eng.delete_record(*record_id)
                    .map_err(|e| EffectError::Dispatch(format!("kernel delete: {e}")))?;
                let hash = hash_state_blake3(&eng.state)
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(serde_json::json!({ "state_hash": hash }))
            }
            KernelCommandBody::CreateNode { kind, record_id } => {
                let mut eng = self.engine.write().await;
                let node_id = eng
                    .create_node_for_record(*record_id, *kind, namespace_id)
                    .map_err(|e| EffectError::Dispatch(format!("kernel create_node: {e}")))?;
                let hash = hash_state_blake3(&eng.state)
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(serde_json::json!({ "node_id": node_id, "state_hash": hash }))
            }
            KernelCommandBody::CreateEdge { from, to, kind } => {
                let mut eng = self.engine.write().await;
                let edge_id = eng
                    .create_edge(*from, *to, *kind)
                    .map_err(|e| EffectError::Dispatch(format!("kernel create_edge: {e}")))?;
                let hash = hash_state_blake3(&eng.state)
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(serde_json::json!({ "edge_id": edge_id, "state_hash": hash }))
            }
        }
    }

    fn state_hash(&self, _shard_id: u8) -> String {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        if let Ok(eng) = self.engine.try_read() {
            hash_state_blake3(&eng.state)
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect()
        } else {
            "0".repeat(64)
        }
    }

    async fn save_snapshot(
        &self,
        _shard_id: u8,
        path: Option<&str>,
    ) -> Result<String, EffectError> {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        // Resolve path and snapshot bytes under a READ lock, then release before I/O.
        let (target, data, hash) = {
            let eng = self.engine.read().await;
            let target = path
                .map(std::path::PathBuf::from)
                .or_else(|| eng.snapshot_path.clone())
                .ok_or_else(|| {
                    EffectError::Dispatch("snapshot: No snapshot path configured".into())
                })?;
            let data = eng
                .snapshot()
                .map_err(|e| EffectError::Dispatch(format!("snapshot encode: {e}")))?;
            let hash: String = hash_state_blake3(&eng.state)
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            (target, data, hash)
        }; // read lock released here
        std::fs::write(&target, data)
            .map_err(|e| EffectError::Dispatch(format!("snapshot write: {e}")))?;
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
        let hits = eng
            .search_l2_ns(&vector, k as usize, namespace_id)
            .map_err(|e| EffectError::Dispatch(format!("graph_rag search: {e}")))?;

        let mut seeds: Vec<u32> = Vec::new();
        let mut hits_out: Vec<serde_json::Value> = Vec::new();
        for (record_id, score) in &hits {
            let node_id = eng.record_to_node.get(record_id).copied();
            if let Some(nid) = node_id {
                seeds.push(nid);
            }
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

        let (nodes, edges) = valori_rag::graph::expand_subgraph(&eng.state, &seeds, depth);
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
        let over_k = if use_rerank || metadata_filter.is_some() {
            k as usize * 10
        } else {
            k as usize
        };
        let eng = self.engine.read().await;
        let hits = eng
            .search_l2_ns(&vector, over_k, namespace_id)
            .map_err(|e| EffectError::Dispatch(format!("memory_search: {e}")))?;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut results: Vec<serde_json::Value> = hits
            .iter()
            .map(|(record_id, score)| {
                let memory_id = format!("rec:{record_id}");
                let metadata = eng.metadata.get(&memory_id);
                let (decay_factor, age_secs) = match decay_half_life_secs {
                    Some(hl) if hl > 0.0 => {
                        let created = eng.created_at.get(record_id).copied().unwrap_or(now_secs);
                        let age_s = now_secs.saturating_sub(created);
                        (
                            Some(valori_search::decay_factor(age_s, hl as u64)),
                            Some(age_s),
                        )
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
            })
            .collect();

        // Metadata filter
        if let Some(ref mf) = metadata_filter {
            let mf_map = mf.as_object().cloned().unwrap_or_default();
            results.retain(|r| {
                r.get("metadata")
                    .map(|m| valori_search::matches_metadata_filter(m, &mf_map))
                    .unwrap_or(false)
            });
        }

        // Rerank
        if use_rerank {
            let qt = query_text.as_deref().unwrap_or("");
            let candidates: Vec<(u32, f32)> = results
                .iter()
                .filter_map(|r| {
                    Some((r["record_id"].as_u64()? as u32, r["score"].as_f64()? as f32))
                })
                .collect();
            let reranked = eng.reranker_rerank(qt, &vector, &candidates);
            let order: Vec<u32> = reranked.iter().map(|(id, _)| *id).collect();
            results.sort_by_key(|r| {
                let id = r["record_id"].as_u64().unwrap_or(u64::MAX) as u32;
                order
                    .iter()
                    .position(|&rid| rid == id)
                    .unwrap_or(usize::MAX)
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
        let ns_id = if namespace_id == 0 {
            None
        } else {
            Some(namespace_id)
        };
        let raw = valori_rag::community::label_propagation(&eng.state, ns_id, max_iter);
        let store = valori_rag::community::build_community_store(&eng.state, raw);
        let community_count = store.community_count;
        let node_count = store.node_count;
        let receipt = store.receipt.clone();
        let communities: Vec<serde_json::Value> = store
            .members
            .iter()
            .map(|(&cid, members)| {
                serde_json::json!({
                    "community_id": cid,
                    "member_count": members.len(),
                })
            })
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
        let store = eng.resources.community_store.as_ref().ok_or_else(|| {
            EffectError::Dispatch("community index not built — call community_detect first".into())
        })?;
        let ranked = valori_rag::community::rank_communities(store, &vector, k as usize);
        let total = store.centroids.len();
        let communities: Vec<serde_json::Value> = ranked
            .into_iter()
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

    async fn tree_build(
        &self,
        text: String,
        doc_name: String,
    ) -> Result<serde_json::Value, EffectError> {
        let tree = valori_rag::tree::TreeIndex::from_markdown(&text, &doc_name);
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
        let tree: valori_rag::tree::TreeIndex = serde_json::from_value(tree_json)
            .map_err(|e| EffectError::Dispatch(format!("tree_query: bad tree JSON: {e}")))?;
        let prev = prev_hash.as_deref().unwrap_or(valori_rag::tree::GENESIS);
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
        use valori_rag::tree::{HybridHit, HybridResponse, GENESIS};

        let tree_json = params
            .get("tree")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let cache_key = params
            .get("cache_key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let tree_weight = params
            .get("tree_weight")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5) as f32;
        let prev = params
            .get("prev_hash")
            .and_then(|v| v.as_str())
            .unwrap_or(GENESIS)
            .to_string();

        let tree: valori_rag::tree::TreeIndex = if !tree_json.is_null() {
            serde_json::from_value(tree_json)
                .map_err(|e| EffectError::Dispatch(format!("tree_hybrid: bad tree: {e}")))?
        } else if let Some(ref key) = cache_key {
            self.engine
                .read()
                .await
                .get_cached_tree(key)
                .cloned()
                .ok_or_else(|| EffectError::Dispatch(format!("tree not in cache: {key}")))?
        } else {
            return Err(EffectError::Dispatch(
                "tree_hybrid: provide 'tree' or 'cache_key' in params".into(),
            ));
        };

        let tw = tree_weight.clamp(0.0, 1.0);
        let vw = 1.0 - tw;
        let k_usize = k as usize;
        let tree_ranked = tree.rank_nodes_normalized(&query, k_usize * 2);
        let tree_hit_count = tree_ranked.len();

        let mut hits: Vec<HybridHit> = tree_ranked
            .iter()
            .map(|(nid, norm_score)| {
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
            })
            .collect();

        let mut vector_hit_count = 0usize;
        let mut reasoning_extra = String::new();

        if vw > 0.0 {
            let eng = self.engine.read().await;
            let query_vec: Vec<f32> = params
                .get("vector")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_f64())
                        .map(|f| f as f32)
                        .collect()
                })
                .unwrap_or_default();
            if !query_vec.is_empty() {
                if let Ok(vec_hits) = eng.search_l2_ns(&query_vec, k_usize * 2, namespace_id) {
                    let max_dist = vec_hits
                        .iter()
                        .map(|(_, d)| *d)
                        .fold(0f32, f32::max)
                        .max(1e-9);
                    for (rid, dist) in &vec_hits {
                        let sim = ((1.0 - dist / max_dist) as f64).clamp(0.0, 1.0);
                        hits.push(HybridHit {
                            source: "vector".into(),
                            score: vw as f64 * sim,
                            node_id: None,
                            title: None,
                            breadcrumb: None,
                            text: None,
                            lines: None,
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

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
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

        let resp = HybridResponse {
            query,
            hits,
            tree_hit_count,
            vector_hit_count,
            tree_answer,
            reasoning,
        };
        serde_json::to_value(&resp)
            .map_err(|e| EffectError::Dispatch(format!("tree_hybrid serialize: {e}")))
    }
}

// ── RaftKernelCapability ──────────────────────────────────────────────────────

/// `KernelCapability` backed by the cluster Raft path.
///
/// Holds all shard handles keyed by `ShardId` so it can route writes to the
/// correct shard based on the `shard_id` parameter. Also carries the node-local
/// tree cache, community store, and embed config so read-only capability methods
/// can fulfil their contract without going through Raft.
pub struct RaftKernelCapability {
    shards: Arc<
        std::collections::BTreeMap<valori_consensus::types::ShardId, crate::cluster::ShardHandle>,
    >,
    sm: valori_consensus::ValoriStateMachine,
    pub shard_count: u8,
    tree_cache:
        Arc<tokio::sync::RwLock<std::collections::HashMap<String, valori_rag::tree::TreeIndex>>>,
    community_store: Arc<tokio::sync::RwLock<Option<valori_rag::community::CommunityStore>>>,
    embed_config: Option<EmbedConfig>,
    http: reqwest::Client,
}

impl RaftKernelCapability {
    pub fn new(
        shards: Arc<
            std::collections::BTreeMap<
                valori_consensus::types::ShardId,
                crate::cluster::ShardHandle,
            >,
        >,
        sm: valori_consensus::ValoriStateMachine,
        shard_count: u8,
        tree_cache: Arc<
            tokio::sync::RwLock<std::collections::HashMap<String, valori_rag::tree::TreeIndex>>,
        >,
        community_store: Arc<tokio::sync::RwLock<Option<valori_rag::community::CommunityStore>>>,
        embed_config: Option<EmbedConfig>,
        http: reqwest::Client,
    ) -> Self {
        RaftKernelCapability {
            shards,
            sm,
            shard_count,
            tree_cache,
            community_store,
            embed_config,
            http,
        }
    }
}

impl Capability for RaftKernelCapability {
    fn name(&self) -> &'static str {
        "kernel_raft"
    }
    fn is_available(&self) -> bool {
        true
    }
}

#[async_trait]
impl KernelCapability for RaftKernelCapability {
    fn shard_count(&self) -> u8 {
        self.shard_count
    }

    async fn apply_command(
        &self,
        shard_id: u8,
        namespace_id: u16,
        body: &KernelCommandBody,
        request_id: &str,
    ) -> Result<serde_json::Value, EffectError> {
        use valori_consensus::types::{ClientRequest, ShardId, CURRENT_SCHEMA_VERSION};
        use valori_kernel::config::SCALE;
        use valori_kernel::event::KernelEvent;
        use valori_kernel::types::scalar::FxpScalar;
        use valori_kernel::types::vector::FxpVector;

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
        let shard = self
            .shards
            .get(&sid)
            .ok_or_else(|| EffectError::Dispatch(format!("shard {shard_id} not found")))?;

        match body {
            KernelCommandBody::InsertRecord {
                values,
                metadata: _,
                tag,
                ..
            } => {
                let fxp: Result<Vec<_>, _> = values
                    .iter()
                    .map(|&v| {
                        if v > 32767.99 || v < -32768.0 {
                            Err(EffectError::TaskFailed("value out of Q16.16 range".into()))
                        } else {
                            Ok(FxpScalar((v * SCALE as f32) as i32))
                        }
                    })
                    .collect();
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

                let resp = shard
                    .raft
                    .client_write(cr)
                    .await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let record_id = resp.data.allocated_record_id.unwrap_or(0);
                let hash = resp
                    .data
                    .state_hash
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(
                    serde_json::json!({ "record_id": record_id, "log_index": resp.data.log_index, "state_hash": hash }),
                )
            }
            KernelCommandBody::SoftDeleteRecord { record_id } => {
                let cr = ClientRequest {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id,
                    event: KernelEvent::SoftDeleteRecord {
                        id: valori_kernel::types::id::RecordId(*record_id),
                    },
                    request_id: req_id_bytes,
                };
                let resp = shard
                    .raft
                    .client_write(cr)
                    .await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let hash = resp
                    .data
                    .state_hash
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(serde_json::json!({ "state_hash": hash }))
            }
            KernelCommandBody::HardDeleteRecord { record_id } => {
                let cr = ClientRequest {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id,
                    event: KernelEvent::DeleteRecord {
                        id: valori_kernel::types::id::RecordId(*record_id),
                    },
                    request_id: req_id_bytes,
                };
                let resp = shard
                    .raft
                    .client_write(cr)
                    .await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let hash = resp
                    .data
                    .state_hash
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(serde_json::json!({ "state_hash": hash }))
            }
            KernelCommandBody::CreateNode { kind, record_id } => {
                let cr = ClientRequest {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id,
                    event: KernelEvent::AutoCreateNode {
                        kind: valori_kernel::types::enums::NodeKind::from_u8(*kind)
                            .unwrap_or(valori_kernel::types::enums::NodeKind::Document),
                        record: record_id.map(valori_kernel::types::id::RecordId),
                    },
                    request_id: req_id_bytes,
                };
                let resp = shard
                    .raft
                    .client_write(cr)
                    .await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let node_id = resp.data.allocated_node_id.unwrap_or(0);
                let hash = resp
                    .data
                    .state_hash
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(
                    serde_json::json!({ "node_id": node_id, "log_index": resp.data.log_index, "state_hash": hash }),
                )
            }
            KernelCommandBody::CreateEdge { from, to, kind } => {
                let cr = ClientRequest {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    namespace_id,
                    event: KernelEvent::AutoCreateEdge {
                        from: valori_kernel::types::id::NodeId(*from),
                        to: valori_kernel::types::id::NodeId(*to),
                        kind: valori_kernel::types::enums::EdgeKind::from_u8(*kind)
                            .unwrap_or(valori_kernel::types::enums::EdgeKind::RefersTo),
                    },
                    request_id: req_id_bytes,
                };
                let resp = shard
                    .raft
                    .client_write(cr)
                    .await
                    .map_err(|e| EffectError::Dispatch(format!("raft.client_write: {e}")))?;
                let edge_id = resp.data.allocated_edge_id.unwrap_or(0);
                let hash = resp
                    .data
                    .state_hash
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                Ok(
                    serde_json::json!({ "edge_id": edge_id, "log_index": resp.data.log_index, "state_hash": hash }),
                )
            }
        }
    }

    fn state_hash(&self, shard_id: u8) -> String {
        use valori_consensus::types::ShardId;
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        let sid = ShardId(shard_id as u32);
        if let Some(shard) = self.shards.get(&sid) {
            let sm = shard.state_machine.clone();
            // block_in_place: moves this thread out of the async scheduler so we can
            // block_on an async call from sync context.  Safe here because:
            //   1. The ValoriStateMachine mutex is never held by *this* task at the
            //      point state_hash is called (the preceding graph_rag / save_snapshot
            //      await already released it).
            //   2. Valori uses a multi-threaded Tokio runtime, which supports block_in_place.
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async move {
                    sm.with_state(|kernel| {
                        hash_state_blake3(kernel)
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    })
                    .await
                })
            })
        } else {
            "0".repeat(64)
        }
    }

    async fn save_snapshot(
        &self,
        shard_id: u8,
        _path: Option<&str>,
    ) -> Result<String, EffectError> {
        use valori_consensus::types::ShardId;
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        let sid = ShardId(shard_id as u32);
        let shard = self
            .shards
            .get(&sid)
            .ok_or_else(|| EffectError::Dispatch(format!("shard {shard_id} not found")))?;
        let kernel_state = shard.state_machine.clone_state().await;
        let hash: String = hash_state_blake3(&kernel_state)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        Ok(hash)
    }

    async fn graph_rag(
        &self,
        shard_id: u8,
        namespace_id: u16,
        vector: Vec<f32>,
        k: u32,
        depth: u32,
    ) -> Result<serde_json::Value, EffectError> {
        use valori_consensus::types::ShardId;
        use valori_kernel::fxp::qformat::SCALE;
        use valori_kernel::index::SearchResult;
        use valori_kernel::types::scalar::FxpScalar;
        use valori_kernel::types::vector::FxpVector;

        let sid = ShardId(shard_id as u32);
        let shard = self
            .shards
            .get(&sid)
            .ok_or_else(|| EffectError::Dispatch(format!("shard {shard_id} not found")))?;

        let fxp_data: Result<Vec<FxpScalar>, EffectError> = vector
            .iter()
            .map(|&v| {
                if v > 32767.99 || v < -32768.0 {
                    Err(EffectError::TaskFailed(
                        "query vector value out of Q16.16 range".into(),
                    ))
                } else {
                    Ok(FxpScalar((v * SCALE as f32) as i32))
                }
            })
            .collect();
        let fxp_q = FxpVector { data: fxp_data? };
        let k_usize = k as usize;

        // Pass 1: search + seed/subgraph (sync closure, no metadata yet).
        let (raw_hits, seeds, nodes, edges): (Vec<(u32, f32, Option<u32>)>, Vec<u32>, _, _) = shard
            .state_machine
            .with_state(move |s| {
                let mut buf = vec![SearchResult::default(); k_usize];
                let n = s.search_l2_ns(&fxp_q, &mut buf, namespace_id);
                let hits: Vec<(u32, f32)> = buf[..n]
                    .iter()
                    .map(|r| {
                        let dist = r.score as f32 / (SCALE as f32 * SCALE as f32);
                        (r.id.0, dist)
                    })
                    .collect();
                let record_ids: Vec<u32> = hits.iter().map(|(id, _)| *id).collect();
                let seed_map = valori_rag::graph::resolve_seed_nodes(s, &record_ids);
                let mut seeds: Vec<u32> = Vec::new();
                let raw: Vec<(u32, f32, Option<u32>)> = hits
                    .iter()
                    .map(|(record_id, score)| {
                        let node_id = seed_map.get(record_id).copied();
                        if let Some(nid) = node_id {
                            seeds.push(nid);
                        }
                        (*record_id, *score, node_id)
                    })
                    .collect();
                let (nodes, edges) = valori_rag::graph::expand_subgraph(s, &seeds, depth);
                (raw, seeds, nodes, edges)
            })
            .await;

        // Pass 2: fetch metadata async.
        let mut hits_out: Vec<serde_json::Value> = Vec::with_capacity(raw_hits.len());
        for (record_id, score, node_id) in &raw_hits {
            let memory_id = format!("rec:{record_id}");
            let metadata = shard.state_machine.get_meta_json(&memory_id).await;
            hits_out.push(serde_json::json!({
                "memory_id": memory_id,
                "record_id": record_id,
                "score": score,
                "node_id": node_id,
                "metadata": metadata,
            }));
        }

        Ok(
            serde_json::json!({ "hits": hits_out, "seed_nodes": seeds, "subgraph": { "nodes": nodes, "edges": edges } }),
        )
    }

    async fn memory_search(
        &self,
        shard_id: u8,
        namespace_id: u16,
        vector: Vec<f32>,
        k: u32,
        decay_half_life_secs: Option<f64>,
        rerank: bool,
        query_text: Option<String>,
        metadata_filter: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, EffectError> {
        use valori_consensus::types::ShardId;
        use valori_kernel::fxp::qformat::SCALE;
        use valori_kernel::index::SearchResult;
        use valori_kernel::types::scalar::FxpScalar;
        use valori_kernel::types::vector::FxpVector;

        let sid = ShardId(shard_id as u32);
        let shard = self
            .shards
            .get(&sid)
            .ok_or_else(|| EffectError::Dispatch(format!("shard {shard_id} not found")))?;

        let over_k = if rerank || metadata_filter.is_some() {
            (k as usize) * 10
        } else {
            k as usize
        };
        let fxp_data: Result<Vec<FxpScalar>, EffectError> = vector
            .iter()
            .map(|&v| {
                if v > 32767.99 || v < -32768.0 {
                    Err(EffectError::TaskFailed(
                        "query vector value out of Q16.16 range".into(),
                    ))
                } else {
                    Ok(FxpScalar((v * SCALE as f32) as i32))
                }
            })
            .collect();
        let fxp_q = FxpVector { data: fxp_data? };

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Pass 1: search + decay inside a sync closure (no async allowed there).
        let raw: Vec<(u32, f32, Option<u64>)> = shard
            .state_machine
            .with_state_and_timestamps(move |s, ts| {
                let mut buf = vec![SearchResult::default(); over_k];
                let n = s.search_l2_ns(&fxp_q, &mut buf, namespace_id);
                buf[..n]
                    .iter()
                    .map(|r| {
                        let rid = r.id.0;
                        let dist = r.score as f32 / (SCALE as f32 * SCALE as f32);
                        let age_secs = decay_half_life_secs.map(|_| {
                            let created = ts.get(&rid).copied().unwrap_or(0);
                            now_secs.saturating_sub(created)
                        });
                        (rid, dist, age_secs)
                    })
                    .collect::<Vec<_>>()
            })
            .await;

        // Pass 2: fetch metadata async (KernelState.meta is accessed via get_meta_json).
        let mf_map: Option<serde_json::Map<String, serde_json::Value>> = metadata_filter
            .as_ref()
            .and_then(|v| v.as_object())
            .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect());

        let mut candidates: Vec<crate::api::MemorySearchHit> = Vec::with_capacity(raw.len());
        for (rid, dist, age_secs) in raw {
            let memory_id = format!("rec:{rid}");
            let meta = shard.state_machine.get_meta_json(&memory_id).await;
            if let Some(ref mf) = mf_map {
                let meta_val = meta.as_ref().cloned().unwrap_or(serde_json::Value::Null);
                if !valori_search::matches_metadata_filter(&meta_val, mf) {
                    continue;
                }
            }
            let (decay_factor, age_secs) = if let Some(hl) = decay_half_life_secs {
                let age_s = age_secs.unwrap_or(0);
                let df = valori_search::decay_factor(age_s, hl as u64);
                (Some(df as f32), Some(age_s))
            } else {
                (None, None)
            };
            candidates.push(crate::api::MemorySearchHit {
                memory_id,
                record_id: rid,
                score: dist,
                metadata: meta,
                decay_factor,
                age_secs,
            });
        }

        if decay_half_life_secs.is_some() {
            // score is L2 distance (lower = better); dividing by decay_factor
            // penalizes older records by raising their effective distance.
            candidates.sort_by(|a, b| {
                let score_a = a.score as f64 / a.decay_factor.unwrap_or(1.0) as f64;
                let score_b = b.score as f64 / b.decay_factor.unwrap_or(1.0) as f64;
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // BM25 reranking: build a ValoriReranker from the replicated text corpus.
        if rerank {
            if let Some(qt) = query_text.as_deref() {
                let raw_candidates: Vec<(u64, f32)> = candidates
                    .iter()
                    .map(|c| (c.record_id as u64, c.score))
                    .collect();
                let reranked = shard
                    .state_machine
                    .with_text_corpus(|corpus| {
                        let mut r = valori_search::ValoriReranker::new();
                        for (&id, text) in corpus {
                            r.insert(id, text);
                        }
                        r.rerank(qt, raw_candidates)
                    })
                    .await;
                let order: Vec<u64> = reranked.iter().map(|(id, _)| *id).collect();
                candidates.sort_by_key(|c| {
                    order
                        .iter()
                        .position(|&id| id == c.record_id as u64)
                        .unwrap_or(usize::MAX)
                });
            }
        }

        candidates.truncate(k as usize);
        Ok(serde_json::to_value(&candidates).unwrap_or(serde_json::Value::Array(vec![])))
    }

    async fn community_detect(
        &self,
        shard_id: u8,
        namespace_id: u16,
        max_iter: u32,
    ) -> Result<serde_json::Value, EffectError> {
        use valori_consensus::types::ShardId;

        let sid = ShardId(shard_id as u32);
        let shard = self
            .shards
            .get(&sid)
            .ok_or_else(|| EffectError::Dispatch(format!("shard {shard_id} not found")))?;

        let ns_filter = if namespace_id == 0 {
            None
        } else {
            Some(namespace_id)
        };

        let store = shard
            .state_machine
            .with_state(move |kernel| {
                let raw = valori_rag::community::label_propagation(kernel, ns_filter, max_iter);
                valori_rag::community::build_community_store(kernel, raw)
            })
            .await;

        let communities: Vec<valori_rag::community::CommunitySummary> = store
            .members
            .iter()
            .map(|(&cid, members)| valori_rag::community::CommunitySummary {
                community_id: cid,
                member_count: members.len(),
                centroid_record_id: None,
            })
            .collect();

        let resp = valori_rag::community::DetectResponse {
            community_count: store.community_count,
            node_count: store.node_count,
            receipt: store.receipt.clone(),
            communities,
        };
        *self.community_store.write().await = Some(store);

        serde_json::to_value(&resp)
            .map_err(|e| EffectError::Dispatch(format!("community_detect serialize: {e}")))
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
        let store_guard = self.community_store.read().await;
        let store = store_guard.as_ref().ok_or_else(|| {
            EffectError::Dispatch(
                "community index not built — call POST /v1/community/detect first".into(),
            )
        })?;

        let ranked = valori_rag::community::rank_communities(store, &vector, k as usize);
        let total = store.centroids.len();
        let communities: Vec<valori_rag::community::CommunityHit> = ranked
            .into_iter()
            .map(|(cid, score)| {
                let members = store.members.get(&cid).map(|v| v.as_slice()).unwrap_or(&[]);
                valori_rag::community::CommunityHit {
                    community_id: cid,
                    score,
                    member_count: members.len(),
                    sample_node_ids: members.iter().copied().take(20).collect(),
                }
            })
            .collect();

        let resp = valori_rag::community::SearchResponse {
            communities,
            total_communities_searched: total,
        };
        serde_json::to_value(&resp)
            .map_err(|e| EffectError::Dispatch(format!("community_search serialize: {e}")))
    }

    async fn tree_build(
        &self,
        text: String,
        doc_name: String,
    ) -> Result<serde_json::Value, EffectError> {
        let tree = valori_rag::tree::TreeIndex::from_markdown(&text, &doc_name);
        let cache_key = valori_rag::tree::hash_text(&text);
        self.tree_cache
            .write()
            .await
            .insert(cache_key.clone(), tree.clone());
        serde_json::to_value(serde_json::json!({
            "cache_key": cache_key,
            "doc_name": tree.doc_name,
            "node_count": tree.nodes.len(),
            "tree": tree,
        }))
        .map_err(|e| EffectError::Dispatch(format!("tree_build serialize: {e}")))
    }

    async fn tree_query(
        &self,
        tree_json: serde_json::Value,
        query: String,
        k: u32,
        prev_hash: Option<String>,
    ) -> Result<serde_json::Value, EffectError> {
        let tree: valori_rag::tree::TreeIndex = if let Some(key) = tree_json.as_str() {
            self.tree_cache
                .read()
                .await
                .get(key)
                .cloned()
                .ok_or_else(|| EffectError::Dispatch(format!("tree not in cache: {key}")))?
        } else {
            serde_json::from_value(tree_json)
                .map_err(|e| EffectError::Dispatch(format!("tree_query: bad tree: {e}")))?
        };
        let prev = prev_hash.as_deref().unwrap_or(valori_rag::tree::GENESIS);
        let result = tree.answer(&query, k.max(1) as usize, prev);
        serde_json::to_value(&result)
            .map_err(|e| EffectError::Dispatch(format!("tree_query serialize: {e}")))
    }

    async fn tree_hybrid(
        &self,
        shard_id: u8,
        namespace_id: u16,
        query: String,
        k: u32,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, EffectError> {
        use valori_consensus::types::ShardId;
        use valori_kernel::fxp::qformat::SCALE;
        use valori_kernel::index::SearchResult;
        use valori_kernel::types::scalar::FxpScalar;
        use valori_kernel::types::vector::FxpVector;
        use valori_rag::tree::{HybridHit, HybridResponse, GENESIS};

        let tree_json = params
            .get("tree")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let cache_key = params
            .get("cache_key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let tree_weight = params
            .get("tree_weight")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5) as f32;
        let prev = params
            .get("prev_hash")
            .and_then(|v| v.as_str())
            .unwrap_or(GENESIS)
            .to_string();

        let tree: valori_rag::tree::TreeIndex = if !tree_json.is_null() {
            serde_json::from_value(tree_json)
                .map_err(|e| EffectError::Dispatch(format!("tree_hybrid: bad tree: {e}")))?
        } else if let Some(ref key) = cache_key {
            self.tree_cache
                .read()
                .await
                .get(key)
                .cloned()
                .ok_or_else(|| EffectError::Dispatch(format!("tree not in cache: {key}")))?
        } else {
            return Err(EffectError::Dispatch(
                "tree_hybrid: provide 'tree' or 'cache_key' in params".into(),
            ));
        };

        let tw = tree_weight.clamp(0.0, 1.0);
        let vw = 1.0 - tw;
        let k_usize = k as usize;
        let tree_ranked = tree.rank_nodes_normalized(&query, k_usize * 2);
        let tree_hit_count = tree_ranked.len();

        let mut hits: Vec<HybridHit> = tree_ranked
            .iter()
            .map(|(nid, norm_score)| {
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
            })
            .collect();

        let mut vector_hit_count = 0usize;
        let mut reasoning_extra = String::new();

        if vw > 0.0 {
            let query_vec: Vec<f32> = params
                .get("vector")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_f64())
                        .map(|f| f as f32)
                        .collect()
                })
                .unwrap_or_default();

            if !query_vec.is_empty() {
                let sid = ShardId(shard_id as u32);
                if let Some(shard) = self.shards.get(&sid) {
                    let fxp_data: Vec<FxpScalar> = query_vec
                        .iter()
                        .map(|&v| FxpScalar((v * SCALE as f32) as i32))
                        .collect();
                    let fxp_q = FxpVector { data: fxp_data };
                    let fetch = k_usize * 2;
                    let raw_hits: Vec<(u32, f32)> = shard
                        .state_machine
                        .with_state(move |kernel| {
                            let mut results = vec![SearchResult::default(); fetch];
                            let found = kernel.search_l2_ns(&fxp_q, &mut results, namespace_id);
                            results[..found]
                                .iter()
                                .map(|r| {
                                    let dist = r.score as f32 / (SCALE as f32 * SCALE as f32);
                                    (r.id.0, dist)
                                })
                                .collect::<Vec<_>>()
                        })
                        .await;
                    let max_dist = raw_hits
                        .iter()
                        .map(|(_, d)| *d)
                        .fold(f32::NEG_INFINITY, f32::max)
                        .max(1e-6);
                    for (rid, dist) in &raw_hits {
                        let norm_sim = ((1.0 - dist / max_dist) as f64).clamp(0.0, 1.0);
                        hits.push(HybridHit {
                            source: "vector".into(),
                            score: vw as f64 * norm_sim,
                            node_id: None,
                            title: None,
                            breadcrumb: None,
                            text: None,
                            lines: None,
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

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
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
        let resp = HybridResponse {
            query,
            hits,
            tree_hit_count,
            vector_hit_count,
            tree_answer,
            reasoning,
        };
        serde_json::to_value(&resp)
            .map_err(|e| EffectError::Dispatch(format!("tree_hybrid serialize: {e}")))
    }
}

// ── NoRaftKernelCapability (placeholder for tests) ────────────────────────────

pub struct NoRaftKernelCapability {
    pub shard_count: u8,
}

impl Capability for NoRaftKernelCapability {
    fn name(&self) -> &'static str {
        "kernel_raft_stub"
    }
    fn is_available(&self) -> bool {
        false
    }
}

#[async_trait]
impl KernelCapability for NoRaftKernelCapability {
    fn shard_count(&self) -> u8 {
        self.shard_count
    }
    async fn apply_command(
        &self,
        _: u8,
        _: u16,
        _: &KernelCommandBody,
        _: &str,
    ) -> Result<serde_json::Value, EffectError> {
        Err(EffectError::CapabilityUnavailable("kernel_raft_stub"))
    }
    fn state_hash(&self, _shard_id: u8) -> String {
        "0".repeat(64)
    }
}

// ── HttpEmbedCapability ───────────────────────────────────────────────────────

pub struct HttpEmbedCapability {
    config: Arc<EmbedConfig>,
    client: reqwest::Client,
}

impl HttpEmbedCapability {
    pub fn new(config: EmbedConfig, client: reqwest::Client) -> Self {
        HttpEmbedCapability {
            config: Arc::new(config),
            client,
        }
    }
}

impl Capability for HttpEmbedCapability {
    fn name(&self) -> &'static str {
        "embed_http"
    }
    fn is_available(&self) -> bool {
        true
    }
}

#[async_trait]
impl EmbedCapability for HttpEmbedCapability {
    fn model_name(&self) -> &str {
        &self.config.model
    }
    fn dim(&self) -> usize {
        0
    }

    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, EffectError> {
        embed_batch(&texts, &self.config, &self.client)
            .await
            .map_err(|e| EffectError::Dispatch(e.to_string()))
    }
}

// ── PassthroughHttpCapability ─────────────────────────────────────────────────

pub struct PassthroughHttpCapability {
    client: reqwest::Client,
}

impl PassthroughHttpCapability {
    pub fn new(client: reqwest::Client) -> Self {
        PassthroughHttpCapability { client }
    }
}

impl Capability for PassthroughHttpCapability {
    fn name(&self) -> &'static str {
        "http_passthrough"
    }
    fn is_available(&self) -> bool {
        true
    }
}

#[async_trait]
impl HttpCapability for PassthroughHttpCapability {
    async fn get(&self, url: &str) -> Result<Bytes, EffectError> {
        self.client
            .get(url)
            .send()
            .await
            .map_err(|e| EffectError::Dispatch(e.to_string()))?
            .bytes()
            .await
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
        CapabilityRegistryBuilder {
            engine,
            shard_count,
            embed_config: None,
            http_client,
        }
    }

    pub fn with_embed(mut self, cfg: EmbedConfig) -> Self {
        self.embed_config = Some(cfg);
        self
    }

    pub fn build(self) -> valori_effect::capability::CapabilityRegistry {
        use valori_effect::capability::CapabilityRegistry;

        let kernel: Arc<dyn KernelCapability> =
            Arc::new(EngineKernelCapability::new(self.engine, self.shard_count));

        let embed = self.embed_config.map(|cfg| {
            let cap: Arc<dyn EmbedCapability> =
                Arc::new(HttpEmbedCapability::new(cfg, self.http_client.clone()));
            cap
        });

        let http: Option<Arc<dyn HttpCapability>> =
            Some(Arc::new(PassthroughHttpCapability::new(self.http_client)));

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
        shards: Arc<
            std::collections::BTreeMap<
                valori_consensus::types::ShardId,
                crate::cluster::ShardHandle,
            >,
        >,
        sm: valori_consensus::ValoriStateMachine,
        shard_count: u8,
        embed_config: Option<EmbedConfig>,
        http_client: reqwest::Client,
        tree_cache: Arc<
            tokio::sync::RwLock<std::collections::HashMap<String, valori_rag::tree::TreeIndex>>,
        >,
        community_store: Arc<tokio::sync::RwLock<Option<valori_rag::community::CommunityStore>>>,
    ) -> valori_effect::capability::CapabilityRegistry {
        use valori_effect::capability::CapabilityRegistry;

        let kernel: Arc<dyn KernelCapability> = Arc::new(RaftKernelCapability::new(
            shards,
            sm,
            shard_count,
            tree_cache,
            community_store,
            embed_config.clone(),
            http_client.clone(),
        ));

        let embed = embed_config.map(|cfg| {
            let cap: Arc<dyn EmbedCapability> =
                Arc::new(HttpEmbedCapability::new(cfg, http_client.clone()));
            cap
        });

        let http: Option<Arc<dyn HttpCapability>> =
            Some(Arc::new(PassthroughHttpCapability::new(http_client)));

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
            embed: None,
            llm: None,
            storage: None,
            http: None,
            proof: None,
            scheduler: None,
        };
        assert!(reg.embed().is_err());
        assert!(reg.llm().is_err());
    }
}
