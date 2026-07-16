// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Node Engine — the stateful orchestrator.
//!
//! `Engine` coordinates `KernelState` with persistence, indexing, and
//! application-level caching. It is the single write path for standalone mode:
//! every mutation flows through `commit_and_apply_ns`.
//!
//! # Construction
//!
//! Use [`Engine::with_config`] with an [`EngineConfig`]. `valori-node` provides
//! the `EngineFromNodeConfig` extension trait so that tests and `main.rs` can
//! still call `Engine::new(&node_config)` after importing the trait.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use valori_kernel::error::KernelError;
use valori_kernel::fxp::qformat::SCALE;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::encode_state;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

use valori_index::{BruteForceIndex, Quantizer, VectorIndex, NoQuantizer, ScalarQuantizer};
use valori_metadata::CollectionRegistry;
use valori_storage::events::event_commit::EventCommitter;
use valori_storage::events::event_journal::EventJournal;
use valori_storage::events::event_log::EventLogWriter;

use crate::config::{EngineConfig, IndexKind, QuantizationKind};
use crate::error::EngineError;
use crate::metadata::MetadataStore;
use crate::persistence::Persistence;

/// Auto-tier thresholds for `IndexKind::Auto`.
const AUTO_TIER_BQ_MIN: usize = 10_000;
const AUTO_TIER_HNSW_MIN: usize = 2_000_000;

// ── Support types ─────────────────────────────────────────────────────────────

/// Utilisation stats for a single bounded pool (records, nodes, or edges).
#[derive(Debug, serde::Serialize)]
pub struct PoolStats {
    pub live: usize,
    pub slots_used: usize,
    pub capacity: usize,
    pub fill_pct: f64,
}

/// Structured response for `GET /health`.
///
/// `status` drives load-balancer routing:
/// * `"ok"`       → 200, route freely
/// * `"degraded"` → 200, any pool ≥ 90 % full; still serves all operations
/// * `"full"`     → 503, at least one pool at 100 %
#[derive(Debug, serde::Serialize)]
pub struct EngineHealth {
    pub status: &'static str,
    pub version: &'static str,
    pub dim: usize,
    pub index: String,
    pub persistence: String,
    pub records: PoolStats,
    pub nodes: PoolStats,
    pub edges: PoolStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_log_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_path: Option<String>,
    pub embed_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_provider: Option<String>,
    pub shard_count: usize,
}

/// Result of [`Engine::try_recover`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryMode {
    EventLog(u64),
    Snapshot,
    /// Recovered by replaying `n` commands from the legacy WAL backend
    /// (`Persistence::Wal`). Only attempted when no snapshot was
    /// recovered — `save_snapshot()` never truncates the WAL, so it can
    /// contain the full history relative to the snapshot; snapshot and
    /// WAL are an either/or fallback, not layered.
    Wal(usize),
    Fresh,
}

/// Application-layer caches that sit above the database layer.
pub struct ExecutionResources {
    pub tree_cache: HashMap<String, valori_rag::tree::TreeIndex>,
    pub community_store: Option<valori_rag::community::CommunityStore>,
}

impl ExecutionResources {
    fn new() -> Self {
        Self { tree_cache: HashMap::new(), community_store: None }
    }
}

// ── Engine ────────────────────────────────────────────────────────────────────

/// The Node Engine orchestrates state, persistence, and indexing.
pub struct Engine {
    pub state: KernelState,
    pub metadata: MetadataStore,
    pub index: Box<dyn VectorIndex + Send + Sync>,
    pub quant: Box<dyn Quantizer + Send + Sync>,

    pub index_kind: IndexKind,
    pub current_effective_kind: IndexKind,
    pub quantization_kind: QuantizationKind,
    pub wal_path: Option<PathBuf>,
    pub snapshot_path: Option<PathBuf>,

    pub max_records: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub dim: usize,

    pub persistence: Persistence,
    pub metadata_path: Option<PathBuf>,

    pub record_to_node: HashMap<u32, u32>,
    pub created_at: HashMap<u32, u64>,

    pub namespaces: CollectionRegistry,
    pub namespaces_path: Option<PathBuf>,

    pub object_store: Option<Arc<valori_storage::object_store::ObjectStoreBackend>>,
    pub object_store_keep: u32,

    pub vault: Arc<dyn valori_kernel::crypto::KeyVault>,

    pub batch_seen: rustc_hash::FxHashMap<[u8; 16], u32>,

    pub hnsw_config: valori_index::HnswConfig,
    pub ivf_config: valori_index::IvfConfig,

    pub decay_half_life_secs: Option<u64>,
    pub reranker: valori_search::ValoriReranker,
    pub embed_config: Option<valori_ingest::EmbedConfig>,
    pub resources: ExecutionResources,
    pub shard_count: usize,
}

impl Engine {
    fn make_index(kind: IndexKind, cfg: &EngineConfig) -> Box<dyn VectorIndex + Send + Sync> {
        match kind {
            IndexKind::BruteForce | IndexKind::Auto => Box::new(BruteForceIndex::new()),
            IndexKind::Hnsw => {
                use valori_index::{HnswIndex, HnswConfig};
                let mut hnsw_cfg = HnswConfig::default();
                if let Some(m) = cfg.hnsw_m {
                    hnsw_cfg.m = m;
                    hnsw_cfg.m_max0 = m * 2;
                    hnsw_cfg.lambda = 1.0 / (m as f64).ln();
                }
                if let Some(ef) = cfg.hnsw_ef_construction { hnsw_cfg.ef_construction = ef; }
                if let Some(ef) = cfg.hnsw_ef_search       { hnsw_cfg.ef_search = ef; }
                Box::new(HnswIndex::new_with_config(hnsw_cfg))
            }
            IndexKind::Ivf => {
                use valori_index::{IvfIndex, IvfConfig};
                let auto_scale = cfg.ivf_n_list.is_none() && cfg.ivf_n_probe.is_none();
                Box::new(IvfIndex::new(IvfConfig {
                    n_list:  cfg.ivf_n_list.unwrap_or(100),
                    n_probe: cfg.ivf_n_probe.unwrap_or(10),
                    auto_scale,
                }, cfg.dim))
            }
            IndexKind::Bq => {
                use valori_index::BqIndex;
                Box::new(BqIndex::new())
            }
        }
    }

    /// Primary constructor. `valori-node` wraps this via the `EngineFromNodeConfig`
    /// extension trait so existing `Engine::new(&node_config)` call sites compile
    /// unchanged after importing that trait.
    pub fn with_config(cfg: EngineConfig) -> Self {
        let initial_kind = match cfg.index_kind {
            IndexKind::Auto => IndexKind::BruteForce,
            other => other,
        };
        let index = Self::make_index(initial_kind, &cfg);
        let current_effective_kind = initial_kind;

        let quant: Box<dyn Quantizer + Send + Sync> = match cfg.quantization_kind {
            QuantizationKind::None => Box::new(NoQuantizer),
            QuantizationKind::Scalar => Box::new(ScalarQuantizer {}),
            QuantizationKind::Product => {
                use valori_index::{ProductQuantizer, PqConfig};
                Box::new(ProductQuantizer::new(PqConfig::default(), cfg.dim))
            }
        };

        let persistence = if let Some(ref path) = cfg.event_log_path {
            match EventLogWriter::open(path, Some(cfg.dim as u32)) {
                Ok(log_writer) => {
                    let journal = EventJournal::new();
                    let live_state = KernelState::with_dim(cfg.dim);
                    let mut committer = EventCommitter::new(log_writer, journal, live_state);
                    if let Some(limit) = cfg.event_log_rotation_bytes {
                        committer = committer.with_rotation_bytes(if limit == 0 { None } else { Some(limit) });
                    }
                    Persistence::EventLog(committer)
                }
                Err(e) => {
                    tracing::error!("Failed to open Event Log: {}", e);
                    Persistence::Ephemeral
                }
            }
        } else if let Some(ref path) = cfg.wal_path {
            match valori_storage::wal_writer::WalWriter::open(path, cfg.dim as u32) {
                Ok(writer) => {
                    tracing::info!("WAL initialized at {:?}", path);
                    Persistence::Wal(writer)
                }
                Err(e) => {
                    tracing::error!("Failed to open WAL: {}", e);
                    Persistence::Ephemeral
                }
            }
        } else {
            Persistence::Ephemeral
        };

        let metadata_path = cfg.event_log_path.as_ref()
            .map(|p| p.with_extension("metadata.json"));
        let namespaces_path = cfg.event_log_path.as_ref()
            .or(cfg.snapshot_path.as_ref())
            .map(|p| p.with_extension("namespaces.json"));

        let mut kernel_state = KernelState::with_dim(cfg.dim);
        match initial_kind {
            IndexKind::Bq => {
                use valori_kernel::index::IndexVariant;
                kernel_state.set_index_kind(IndexVariant::BinaryQuantization);
            }
            IndexKind::Hnsw | IndexKind::Ivf => {
                tracing::warn!(
                    "VALORI_INDEX={:?}: kernel replay/proof path uses BruteForce \
                     (HNSW and IVF are not yet kernel-native).",
                    initial_kind
                );
            }
            _ => {}
        }

        let hnsw_config = {
            use valori_index::HnswConfig;
            let mut c = HnswConfig::default();
            if let Some(m) = cfg.hnsw_m {
                c.m = m; c.m_max0 = m * 2; c.lambda = 1.0 / (m as f64).ln();
            }
            if let Some(ef) = cfg.hnsw_ef_construction { c.ef_construction = ef; }
            if let Some(ef) = cfg.hnsw_ef_search       { c.ef_search = ef; }
            c
        };
        let ivf_config = {
            use valori_index::IvfConfig;
            let auto_scale = cfg.ivf_n_list.is_none() && cfg.ivf_n_probe.is_none();
            IvfConfig {
                n_list:  cfg.ivf_n_list.unwrap_or(100),
                n_probe: cfg.ivf_n_probe.unwrap_or(10),
                auto_scale,
            }
        };

        Self {
            state: kernel_state,
            metadata: MetadataStore::new(),
            index,
            quant,
            index_kind: cfg.index_kind,
            current_effective_kind,
            quantization_kind: cfg.quantization_kind,
            wal_path: cfg.wal_path,
            snapshot_path: cfg.snapshot_path,
            max_records: cfg.max_records,
            max_nodes: cfg.max_nodes,
            max_edges: cfg.max_edges,
            dim: cfg.dim,
            persistence,
            record_to_node: HashMap::new(),
            created_at: HashMap::new(),
            metadata_path,
            namespaces: CollectionRegistry::new(),
            namespaces_path,
            object_store: cfg.object_store,
            object_store_keep: cfg.object_store_keep,
            vault: cfg.vault,
            batch_seen: rustc_hash::FxHashMap::default(),
            hnsw_config,
            ivf_config,
            decay_half_life_secs: cfg.decay_half_life_secs,
            reranker: valori_search::ValoriReranker::new(),
            embed_config: cfg.embed_config,
            resources: ExecutionResources::new(),
            shard_count: cfg.shard_count,
        }
    }

    #[inline]
    pub fn shard_for_ns(&self, namespace_id: u16) -> usize {
        if self.shard_count <= 1 { 0 } else { namespace_id as usize % self.shard_count }
    }

    fn commit_and_apply_ns(&mut self, event: &valori_kernel::event::KernelEvent, namespace_id: u16) -> Result<(), EngineError> {
        self.persistence.log_event_ns(event, namespace_id)?;
        self.apply_committed_event_ns(event, namespace_id)
    }

    pub fn event_committer(&self) -> Option<&EventCommitter> {
        self.persistence.event_committer()
    }

    pub fn event_committer_mut(&mut self) -> Option<&mut EventCommitter> {
        self.persistence.event_committer_mut()
    }

    fn now_unix() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    pub fn record_created_at(&self, id: u32) -> Option<u64> {
        self.created_at.get(&id).copied()
    }

    fn rebuild_record_to_node(&mut self) {
        self.record_to_node.clear();
        for node in self.state.iter_nodes() {
            if let Some(rid) = node.record {
                self.record_to_node.insert(rid.0, node.id.0);
            }
        }
    }

    // ── Metadata sidecar ─────────────────────────────────────────────────────

    pub fn flush_metadata(&self) -> Result<(), EngineError> {
        if let Some(ref path) = self.metadata_path {
            self.metadata
                .flush_to(path)
                .map_err(|e| EngineError::InvalidInput(
                    format!("Failed to flush metadata sidecar: {}", e),
                ))?;
        }
        Ok(())
    }

    pub fn load_metadata(&mut self) -> Result<(), EngineError> {
        if let Some(ref path) = self.metadata_path {
            self.metadata
                .load_from(path)
                .map_err(|e| EngineError::InvalidInput(
                    format!("Failed to load metadata sidecar: {}", e),
                ))?;
        }
        Ok(())
    }

    fn sync_metadata_from_state(&mut self) {
        for (key, value) in self.state.meta.iter() {
            if let Ok(parsed) = serde_json::from_str(value) {
                self.metadata.set(key.clone(), parsed);
            }
        }
    }

    pub fn set_meta_audited(&mut self, key: String, value: serde_json::Value) -> Result<(), EngineError> {
        let event = valori_kernel::event::KernelEvent::SetMeta {
            key: key.clone(),
            value: value.to_string(),
        };
        self.commit_and_apply_ns(&event, 0)?;
        self.metadata.set(key, value);
        self.flush_metadata()
    }

    pub fn flush_namespaces(&self) -> Result<(), EngineError> {
        if let Some(ref path) = self.namespaces_path {
            let json = serde_json::to_vec(&self.namespaces)
                .map_err(|e| EngineError::InvalidInput(
                    format!("Failed to serialize namespace registry: {}", e),
                ))?;
            let tmp = {
                let mut s = path.clone().into_os_string();
                s.push(".tmp");
                PathBuf::from(s)
            };
            std::fs::write(&tmp, &json)
                .map_err(|e| EngineError::InvalidInput(
                    format!("Failed to write namespace sidecar: {}", e),
                ))?;
            std::fs::rename(&tmp, path)
                .map_err(|e| EngineError::InvalidInput(
                    format!("Failed to commit namespace sidecar: {}", e),
                ))?;
        }
        Ok(())
    }

    pub fn load_namespaces(&mut self) -> Result<(), EngineError> {
        if let Some(ref path) = self.namespaces_path {
            match std::fs::read(path) {
                Ok(bytes) => {
                    let reg: CollectionRegistry = serde_json::from_slice(&bytes)
                        .map_err(|e| EngineError::InvalidInput(
                            format!("Failed to parse namespace sidecar: {}", e),
                        ))?;
                    self.namespaces = reg;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(EngineError::InvalidInput(
                    format!("Failed to read namespace sidecar: {}", e),
                )),
            }
        }
        Ok(())
    }

    // ── Observability ─────────────────────────────────────────────────────────

    pub fn health(&self) -> EngineHealth {
        let live_records = self.state.record_count();
        let slot_records = self.state.total_record_slots();
        let live_nodes   = self.state.node_count();
        let live_edges   = self.state.edge_count();

        let rec_fill  = pct(live_records, self.max_records);
        let node_fill = pct(live_nodes,   self.max_nodes);
        let edge_fill = pct(live_edges,   self.max_edges);

        let status = if rec_fill >= 100.0 || node_fill >= 100.0 || edge_fill >= 100.0 {
            "full"
        } else if rec_fill >= 90.0 || node_fill >= 90.0 || edge_fill >= 90.0 {
            "degraded"
        } else {
            "ok"
        };

        let persistence = match self.persistence {
            Persistence::EventLog(_) => "event_log",
            Persistence::Wal(_) => "wal",
            Persistence::Ephemeral if self.snapshot_path.is_some() => "snapshot",
            Persistence::Ephemeral => "none",
        };

        EngineHealth {
            status,
            version: env!("CARGO_PKG_VERSION"),
            dim: self.state.dim.unwrap_or(self.dim),
            index: if self.index_kind == IndexKind::Auto {
                format!("auto({})", format!("{:?}", self.current_effective_kind).to_lowercase())
            } else {
                format!("{:?}", self.index_kind)
            },
            persistence: persistence.to_string(),
            records: PoolStats {
                live: live_records,
                slots_used: slot_records,
                capacity: self.max_records,
                fill_pct: round1(rec_fill),
            },
            nodes: PoolStats {
                live: live_nodes,
                slots_used: live_nodes,
                capacity: self.max_nodes,
                fill_pct: round1(node_fill),
            },
            edges: PoolStats {
                live: live_edges,
                slots_used: live_edges,
                capacity: self.max_edges,
                fill_pct: round1(edge_fill),
            },
            event_log_height: self.event_committer().map(|c| c.journal().committed_height()),
            event_log_path: self.event_committer()
                .map(|c| c.event_log().path().to_string_lossy().into_owned()),
            snapshot_path: self.snapshot_path.as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            embed_enabled: self.embed_config.is_some(),
            embed_provider: self.embed_config.as_ref().map(|c| c.provider.clone()),
            shard_count: self.shard_count,
        }
    }

    pub fn update_prometheus_metrics(&self) {
        let live_records = self.state.record_count() as f64;
        let live_nodes   = self.state.node_count()   as f64;
        let live_edges   = self.state.edge_count()   as f64;

        metrics::gauge!("valori_records_live",     live_records);
        metrics::gauge!("valori_records_capacity", self.max_records as f64);
        metrics::gauge!("valori_record_fill_ratio",
            if self.max_records > 0 { live_records / self.max_records as f64 } else { 0.0 });

        metrics::gauge!("valori_nodes_live",     live_nodes);
        metrics::gauge!("valori_nodes_capacity", self.max_nodes as f64);
        metrics::gauge!("valori_node_fill_ratio",
            if self.max_nodes > 0 { live_nodes / self.max_nodes as f64 } else { 0.0 });

        metrics::gauge!("valori_edges_live",     live_edges);
        metrics::gauge!("valori_edges_capacity", self.max_edges as f64);
        metrics::gauge!("valori_edge_fill_ratio",
            if self.max_edges > 0 { live_edges / self.max_edges as f64 } else { 0.0 });

        metrics::gauge!("valori_dim", self.dim as f64);

        if let Some(c) = self.event_committer() {
            metrics::gauge!("valori_event_log_height", c.journal().committed_height() as f64);
        }
    }

    // ── Inserts ───────────────────────────────────────────────────────────────

    pub fn insert_record_from_f32(&mut self, values: &[f32]) -> Result<u32, EngineError> {
        self.insert_record_from_f32_ns(values, valori_kernel::types::id::DEFAULT_NS.0)
    }

    pub fn insert_record_from_f32_ns(&mut self, values: &[f32], namespace_id: u16) -> Result<u32, EngineError> {
        if self.state.record_count() >= self.max_records {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }
        let mut fxp_data = Vec::with_capacity(values.len());
        for &v in values {
            if v > 32767.99 || v < -32768.0 {
                return Err(EngineError::InvalidInput(
                    "Vector values must be between -32768.0 and 32767.99".to_string()
                ));
            }
            fxp_data.push(FxpScalar((v * SCALE as f32) as i32));
        }
        let vector = FxpVector { data: fxp_data };
        let rid = self.state.next_record_id();
        let event = valori_kernel::event::KernelEvent::InsertRecord {
            id: rid, vector, metadata: None, tag: 0,
        };
        self.commit_and_apply_ns(&event, namespace_id)?;
        self.auto_tier_check();
        self.created_at.insert(rid.0, Self::now_unix());
        Ok(rid.0)
    }

    pub fn reranker_insert(&mut self, record_id: u32, text: &str) {
        self.reranker.insert(record_id as u64, text);
    }

    pub fn reranker_corpus_len(&self) -> usize {
        self.reranker.len()
    }

    pub fn reranker_rerank(&self, query_text: &str, _query_vec: &[f32], candidates: &[(u32, f32)]) -> Vec<(u32, f32)> {
        let u64_candidates: Vec<(u64, f32)> = candidates.iter().map(|&(id, s)| (id as u64, s)).collect();
        self.reranker
            .rerank(query_text, u64_candidates)
            .into_iter()
            .map(|(id, s)| (id as u32, s))
            .collect()
    }

    // ── Single-record insert (canonical path for FFI and embedded SDK) ────────

    pub fn next_record_id(&self) -> RecordId {
        self.state.next_record_id()
    }

    /// Insert a pre-converted FxpVector record. Returns the new record ID.
    /// Routes through `commit_and_apply_ns`, so engine.state, the audit log,
    /// and the search index are all updated atomically.
    pub fn insert_record_fxp(
        &mut self,
        fxp_vec: FxpVector,
        metadata: Option<Vec<u8>>,
        tag: u64,
        namespace_id: u16,
    ) -> Result<u32, EngineError> {
        if self.state.record_count() >= self.max_records {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }
        let rid = self.state.next_record_id();
        let event = valori_kernel::event::KernelEvent::InsertRecord {
            id: rid,
            vector: fxp_vec,
            metadata,
            tag,
        };
        self.commit_and_apply_ns(&event, namespace_id)?;
        let now = Self::now_unix();
        self.created_at.insert(rid.0, now);
        Ok(rid.0)
    }

    /// Commit a SetMeta key-value event into the default namespace.
    pub fn apply_meta_event(&mut self, key: String, value: String) -> Result<(), EngineError> {
        let event = valori_kernel::event::KernelEvent::SetMeta { key, value };
        self.commit_and_apply_ns(&event, valori_kernel::types::id::DEFAULT_NS.0)
    }

    // ── Crypto-shredding ──────────────────────────────────────────────────────

    pub fn insert_encrypted_ns(&mut self, plaintext: &[u8], tag: u64, namespace_id: u16, key_id: [u8; 16]) -> Result<u32, EngineError> {
        if self.state.record_count() >= self.max_records {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }
        if self.state.dim.is_none() {
            return Err(EngineError::InvalidInput("VALORI_DIM must be set before encrypted insert".into()));
        }
        let ciphertext = self.vault.encrypt(key_id, plaintext)
            .map_err(|e| EngineError::InvalidInput(format!("Vault encrypt: {e:?}")))?;
        let rid = self.state.next_record_id();
        let event = valori_kernel::event::KernelEvent::InsertRecordEncrypted {
            id: rid, key_id, ciphertext, metadata_ciphertext: None, tag,
        };
        self.commit_and_apply_ns(&event, namespace_id)?;
        Ok(rid.0)
    }

    pub fn shred_key(&mut self, key_id: [u8; 16]) -> Result<(), EngineError> {
        self.vault.shred(key_id)
            .map_err(|e| EngineError::InvalidInput(format!("Vault shred: {e:?}")))?;
        let event = valori_kernel::event::KernelEvent::ShredKey { key_id };
        self.commit_and_apply_ns(&event, valori_kernel::types::id::DEFAULT_NS.0)?;
        Ok(())
    }

    // ── Batch insert ──────────────────────────────────────────────────────────

    pub fn insert_batch(&mut self, batch: &[Vec<f32>]) -> Result<Vec<u32>, EngineError> {
        self.insert_batch_ns(batch, None, valori_kernel::types::id::DEFAULT_NS.0, None)
    }

    pub fn insert_batch_ns(
        &mut self,
        batch: &[Vec<f32>],
        metadata: Option<&[Option<Vec<u8>>]>,
        namespace_id: u16,
        request_ids: Option<&[Option<[u8; 16]>]>,
    ) -> Result<Vec<u32>, EngineError> {
        let mut deduped: Vec<(usize, u32)> = Vec::new();
        let mut insert_indices: Vec<usize> = Vec::new();

        for (i, _) in batch.iter().enumerate() {
            if let Some(Some(rid)) = request_ids.and_then(|r| r.get(i)) {
                if let Some(&existing_id) = self.batch_seen.get(rid) {
                    deduped.push((i, existing_id));
                    continue;
                }
            }
            insert_indices.push(i);
        }

        if self.state.record_count() + insert_indices.len() > self.max_records {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }

        let mut id_map: Vec<u32> = vec![0u32; batch.len()];
        for (i, id) in &deduped { id_map[*i] = *id; }

        let mut events = Vec::with_capacity(insert_indices.len());
        let start_id = self.state.next_record_id().0;

        for (slot, &i) in insert_indices.iter().enumerate() {
            let values = &batch[i];
            let mut fxp_data = Vec::with_capacity(values.len());
            for &v in values {
                if v > 32767.99 || v < -32768.0 {
                    return Err(EngineError::InvalidInput(
                        "Vector values must be between -32768.0 and 32767.99".to_string()
                    ));
                }
                fxp_data.push(FxpScalar((v * SCALE as f32) as i32));
            }
            let id = start_id + slot as u32;
            let meta = metadata.and_then(|m| m.get(i)).cloned().flatten();
            events.push(valori_kernel::event::KernelEvent::InsertRecord {
                id: RecordId(id),
                vector: FxpVector { data: fxp_data },
                metadata: meta,
                tag: 0,
            });
            id_map[i] = id;
        }

        self.persistence
            .log_batch_ns(&events, namespace_id)?;
        for event in &events {
            self.apply_committed_event_ns(event, namespace_id)?;
        }
        self.auto_tier_check();

        for &i in &insert_indices {
            if let Some(Some(rid)) = request_ids.and_then(|r| r.get(i)) {
                if self.batch_seen.len() >= 65536 { self.batch_seen.clear(); }
                self.batch_seen.insert(*rid, id_map[i]);
            }
        }

        let now = Self::now_unix();
        for &i in &insert_indices {
            self.created_at.insert(id_map[i], now);
        }

        Ok(id_map)
    }

    // ── Search ────────────────────────────────────────────────────────────────

    pub fn search_l2(&self, query: &[f32], k: usize) -> Result<Vec<(u32, f32)>, EngineError> {
        self.search_l2_ns(query, k, valori_kernel::types::id::DEFAULT_NS.0)
    }

    pub fn search_l2_ns(&self, query: &[f32], k: usize, namespace_id: u16) -> Result<Vec<(u32, f32)>, EngineError> {
        use valori_kernel::index::SearchResult;

        if let Some(dim) = self.state.dim {
            if query.len() != dim {
                return Err(EngineError::Kernel(KernelError::DimensionMismatch {
                    expected: dim,
                    found: query.len(),
                }));
            }
        }
        for &v in query {
            if v > 32767.99 || v < -32768.0 {
                return Err(EngineError::InvalidInput(
                    "Query vector values must be between -32768.0 and 32767.99".to_string()
                ));
            }
        }

        if self.effective_index_kind() != IndexKind::BruteForce {
            let candidates = self.index.search(query, k);
            let hits: Vec<(u32, f32)> = candidates
                .into_iter()
                .filter(|(id, _)| {
                    self.state
                        .get_record(RecordId(*id))
                        .map_or(false, |r| r.namespace_id == namespace_id)
                })
                .take(k)
                .collect();
            return Ok(hits);
        }

        let fxp_data: Vec<FxpScalar> = query.iter()
            .map(|&v| FxpScalar((v * SCALE as f32) as i32))
            .collect();
        let fxp_query = FxpVector { data: fxp_data };
        let mut results = vec![SearchResult::default(); k];
        let found = self.state.search_l2_ns(&fxp_query, &mut results, namespace_id);
        Ok(results[..found].iter().map(|r| {
            (r.id.0, r.score as f32 / (SCALE as f32 * SCALE as f32))
        }).collect())
    }

    // ── Collections ───────────────────────────────────────────────────────────

    /// Tag-filtered brute-force L2 search across all records.
    ///
    /// When `tag` is `Some(t)`, only records whose stored `tag` field equals `t` are scored.
    /// `None` scores every active record (no tag restriction).
    ///
    /// Returns `(record_id, l2_distance_f32)` pairs in ascending distance order,
    /// using the same f32 scale as `search_l2_ns`.
    pub fn search_l2_filtered(&self, query: &[f32], k: usize, tag: Option<u64>) -> Result<Vec<(u32, f32)>, EngineError> {
        use valori_kernel::index::SearchResult;

        if let Some(dim) = self.state.dim {
            if query.len() != dim {
                return Err(EngineError::Kernel(KernelError::DimensionMismatch {
                    expected: dim,
                    found: query.len(),
                }));
            }
        }
        for &v in query {
            if v > 32767.99 || v < -32768.0 {
                return Err(EngineError::InvalidInput(
                    "Query vector values must be between -32768.0 and 32767.99".to_string(),
                ));
            }
        }

        let fxp_data: Vec<FxpScalar> = query.iter()
            .map(|&v| FxpScalar((v * SCALE as f32) as i32))
            .collect();
        let fxp_query = FxpVector { data: fxp_data };
        let mut results = vec![SearchResult::default(); k];
        let found = self.state.search_l2(&fxp_query, &mut results, tag);
        Ok(results[..found].iter().map(|r| {
            (r.id.0, r.score as f32 / (SCALE as f32 * SCALE as f32))
        }).collect())
    }

    /// BLAKE3 hash of the current kernel state, as a lowercase hex string.
    pub fn state_hash_hex(&self) -> String {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        hash_state_blake3(&self.state).iter().map(|b| format!("{:02x}", b)).collect()
    }

    pub fn resolve_collection(&self, name: Option<&str>) -> Result<u16, EngineError> {
        self.namespaces.resolve(name).ok_or_else(|| {
            EngineError::InvalidInput(format!(
                "unknown collection '{}' — create it first with POST /v1/namespaces",
                name.unwrap_or("default")
            ))
        })
    }

    pub fn create_collection(&mut self, name: &str) -> Result<u16, EngineError> {
        let id = self.namespaces.create(name).ok_or_else(|| {
            EngineError::InvalidInput(format!(
                "namespace limit reached ({} max)", valori_kernel::types::id::MAX_NAMESPACES
            ))
        })?;
        self.state.apply_event_ns(
            &valori_kernel::event::KernelEvent::AutoCreateNamespace { name: String::new() },
            id,
        )?;
        self.flush_namespaces()?;
        Ok(id)
    }

    pub fn drop_collection(&mut self, name: &str) -> Result<(), EngineError> {
        if name == "default" {
            return Err(EngineError::InvalidInput(
                "the 'default' collection cannot be dropped".into(),
            ));
        }
        let id = self.namespaces.drop(name).ok_or_else(|| {
            EngineError::InvalidInput(format!("collection '{name}' not found"))
        })?;
        let ns_record_ids: Vec<u64> = self.state.iter_records_in_ns(id)
            .map(|r| r.id.0 as u64)
            .collect();
        self.state.apply_event_ns(
            &valori_kernel::event::KernelEvent::DropNamespace { name: String::new() },
            id,
        )?;
        for rid in &ns_record_ids { self.index.delete(*rid as u32); }
        self.reranker.remove_batch(&ns_record_ids);
        self.flush_namespaces()?;
        Ok(())
    }

    pub fn list_collections(&self) -> Vec<(String, u16)> {
        self.namespaces.list()
    }

    // ── Snapshot ──────────────────────────────────────────────────────────────

    pub fn snapshot(&self) -> Result<Vec<u8>, EngineError> {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"VAL1");

        let hint = valori_kernel::snapshot::encode::encode_capacity_hint(&self.state);
        let mut k_buf = Vec::with_capacity(hint);
        encode_state(&self.state, &mut k_buf)?;
        buffer.extend_from_slice(&(k_buf.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&k_buf);

        let m_buf = self.metadata.snapshot();
        buffer.extend_from_slice(&(m_buf.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&m_buf);

        let i_buf = self.index.snapshot().map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        buffer.extend_from_slice(&(i_buf.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&i_buf);

        let ns_json = serde_json::to_vec(&self.namespaces)
            .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        buffer.extend_from_slice(b"NSRG");
        buffer.extend_from_slice(&(ns_json.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&ns_json);

        let crts_buf = bincode::serde::encode_to_vec(&self.created_at, bincode::config::standard())
            .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        buffer.extend_from_slice(b"CRTS");
        buffer.extend_from_slice(&(crts_buf.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&crts_buf);

        let (corpus, total_tokens) = self.reranker.snapshot_corpus();
        let bcrp_buf = bincode::serde::encode_to_vec(&(corpus, total_tokens), bincode::config::standard())
            .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        buffer.extend_from_slice(b"BCRP");
        buffer.extend_from_slice(&(bcrp_buf.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&bcrp_buf);

        Ok(buffer)
    }

    pub fn save_snapshot(&self, path: Option<&Path>) -> Result<PathBuf, EngineError> {
        let target = path.or(self.snapshot_path.as_deref())
            .ok_or(EngineError::InvalidInput("No snapshot path configured".into()))?;
        let data = self.snapshot()?;
        std::fs::write(target, data).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        tracing::info!("Snapshot saved to {:?}", target);
        Ok(target.to_path_buf())
    }

    pub fn restore(&mut self, data: &[u8]) -> Result<(), EngineError> {
        if data.len() < 16 {
            return Err(EngineError::InvalidInput("Buffer too small".into()));
        }
        if &data[0..4] != b"VAL1" {
            return Err(EngineError::InvalidInput("Invalid magic bytes".into()));
        }
        let mut offset = 4;

        let k_len = read_u32(data, &mut offset, "k_len")? as usize;
        let k_data = slice_at(data, &mut offset, k_len, "k_data")?;

        let m_len = read_u32(data, &mut offset, "m_len")? as usize;
        let m_data = slice_at(data, &mut offset, m_len, "m_data")?;

        let i_len = read_u32(data, &mut offset, "i_len")? as usize;
        let i_data = if offset + i_len <= data.len() {
            Some(&data[offset..offset + i_len])
        } else {
            None
        };
        offset += i_len;

        let ns_registry: Option<CollectionRegistry> = if offset + 4 <= data.len()
            && &data[offset..offset + 4] == b"NSRG"
        {
            offset += 4;
            let ns_len = read_u32(data, &mut offset, "ns_len")? as usize;
            let ns_json = slice_at(data, &mut offset, ns_len, "ns_data")?;
            Some(serde_json::from_slice(ns_json)
                .map_err(|e| EngineError::InvalidInput(format!("ns registry decode: {e}")))?)
        } else {
            None
        };

        self.restore_from_components(k_data, m_data, i_data, ns_registry)?;
        self.restore_trailing_sections(data, offset);
        Ok(())
    }

    // ── Mutations ─────────────────────────────────────────────────────────────

    pub fn soft_delete_record(&mut self, id: u32) -> Result<(), EngineError> {
        if let Some(node_id) = self.record_to_node.get(&id).copied() {
            self.delete_node(node_id)?;
        }
        let rid = RecordId(id);
        let event = valori_kernel::event::KernelEvent::SoftDeleteRecord { id: rid };
        self.commit_and_apply_ns(&event, valori_kernel::types::id::DEFAULT_NS.0)?;
        self.reranker.remove(id as u64);
        self.created_at.remove(&id);
        Ok(())
    }

    pub fn update_record_metadata(&mut self, id: u32, metadata: Option<Vec<u8>>, namespace_id: u16) -> Result<(), EngineError> {
        let rid = RecordId(id);
        let event = valori_kernel::event::KernelEvent::UpdateRecordMetadata { id: rid, metadata };
        self.commit_and_apply_ns(&event, namespace_id)
    }

    pub fn delete_record(&mut self, id: u32) -> Result<(), EngineError> {
        if let Some(node_id) = self.record_to_node.get(&id).copied() {
            self.delete_node(node_id)?;
        }
        let rid = RecordId(id);
        let event = valori_kernel::event::KernelEvent::DeleteRecord { id: rid };
        self.commit_and_apply_ns(&event, valori_kernel::types::id::DEFAULT_NS.0)?;
        self.created_at.remove(&id);
        Ok(())
    }

    pub fn delete_node(&mut self, id: u32) -> Result<(), EngineError> {
        use valori_kernel::types::id::NodeId;
        let event = valori_kernel::event::KernelEvent::DeleteNode { id: NodeId(id) };
        self.commit_and_apply_ns(&event, valori_kernel::types::id::DEFAULT_NS.0)?;
        Ok(())
    }

    pub fn delete_edge(&mut self, id: u32) -> Result<(), EngineError> {
        use valori_kernel::types::id::EdgeId;
        let event = valori_kernel::event::KernelEvent::DeleteEdge { id: EdgeId(id) };
        self.commit_and_apply_ns(&event, valori_kernel::types::id::DEFAULT_NS.0)?;
        Ok(())
    }

    pub fn create_node_for_record(&mut self, record_id: Option<u32>, kind: u8, namespace_id: u16) -> Result<u32, EngineError> {
        if self.state.node_count() >= self.max_nodes {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }
        let node_id = self.state.next_node_id();
        let kind = NodeKind::from_u8(kind).unwrap_or_default();
        let record = record_id.map(RecordId);
        let event = valori_kernel::event::KernelEvent::CreateNode { id: node_id, kind, record };
        self.commit_and_apply_ns(&event, namespace_id)?;
        Ok(node_id.0)
    }

    pub fn nodes_in_ns(&self, namespace_id: u16) -> Vec<(u32, u8, Option<u32>)> {
        self.state.iter_nodes()
            .filter(|n| n.namespace_id == namespace_id)
            .map(|n| (n.id.0, n.kind as u8, n.record.map(|r| r.0)))
            .collect()
    }

    pub fn create_edge(&mut self, from: u32, to: u32, kind: u8) -> Result<u32, EngineError> {
        if self.state.edge_count() >= self.max_edges {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }
        use valori_kernel::types::id::{NodeId, EdgeId};
        let kind = EdgeKind::from_u8(kind).unwrap_or_default();
        let edge_id = EdgeId(self.state.edge_count() as u32);
        let event = valori_kernel::event::KernelEvent::CreateEdge {
            id: edge_id, kind, from: NodeId(from), to: NodeId(to),
        };
        self.commit_and_apply_ns(&event, valori_kernel::types::id::DEFAULT_NS.0)?;
        Ok(edge_id.0)
    }

    pub fn get_proof(&self) -> valori_kernel::proof::DeterministicProof {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        let final_state_hash = hash_state_blake3(&self.state);
        valori_kernel::proof::DeterministicProof {
            kernel_version: 1,
            snapshot_hash: [0u8; 32],
            wal_hash: [0u8; 32],
            final_state_hash,
        }
    }

    // ── Event application ─────────────────────────────────────────────────────

    pub fn apply_committed_event(&mut self, event: &valori_kernel::event::KernelEvent) -> Result<(), EngineError> {
        use valori_kernel::event::KernelEvent;
        if let KernelEvent::DeleteNode { id } = event {
            if let Some(node) = self.state.get_node(*id) {
                if let Some(rid) = node.record { self.record_to_node.remove(&rid.0); }
            }
        }
        self.state.apply_event(event)?;
        self.post_apply_derived(event);
        Ok(())
    }

    pub fn apply_committed_event_ns(&mut self, event: &valori_kernel::event::KernelEvent, namespace_id: u16) -> Result<(), EngineError> {
        use valori_kernel::event::KernelEvent;
        if let KernelEvent::DeleteNode { id } = event {
            if let Some(node) = self.state.get_node(*id) {
                if let Some(rid) = node.record { self.record_to_node.remove(&rid.0); }
            }
        }
        self.state.apply_event_ns(event, namespace_id)?;
        self.post_apply_derived(event);
        Ok(())
    }

    fn post_apply_derived(&mut self, event: &valori_kernel::event::KernelEvent) {
        use valori_kernel::event::KernelEvent;
        match event {
            KernelEvent::InsertRecord { id, vector, .. } => {
                let vals: Vec<f32> = vector.data.iter().map(|fxp| fxp.0 as f32 / SCALE as f32).collect();
                self.index.insert(id.0, &vals);
            }
            KernelEvent::DeleteRecord { id } | KernelEvent::SoftDeleteRecord { id } => {
                self.index.delete(id.0);
            }
            KernelEvent::CreateNode { id, record, .. } => {
                if let Some(rid) = record {
                    self.record_to_node.insert(rid.0, id.0);
                }
            }
            _ => {}
        }
    }

    // ── Tree cache ────────────────────────────────────────────────────────────

    pub fn cache_tree(&mut self, text: &str, tree: valori_rag::tree::TreeIndex) -> String {
        let key = valori_rag::tree::hash_text(text);
        self.resources.tree_cache.insert(key.clone(), tree);
        key
    }

    pub fn get_cached_tree(&self, key: &str) -> Option<&valori_rag::tree::TreeIndex> {
        self.resources.tree_cache.get(key)
    }

    // ── KernelState read accessors ────────────────────────────────────────────

    pub fn record_count(&self) -> usize { self.state.record_count() }

    pub fn apply_event_for_test(&mut self, evt: &valori_kernel::event::KernelEvent) -> Result<(), valori_kernel::error::KernelError> {
        self.state.apply_event(evt)
    }

    pub fn clone_kernel_state(&self) -> KernelState { self.state.clone() }

    pub fn kernel_state(&self) -> &KernelState { &self.state }

    pub fn node_count(&self) -> usize { self.state.node_count() }

    pub fn edge_count(&self) -> usize { self.state.edge_count() }

    pub fn kernel_dim(&self) -> Option<usize> { self.state.dim }

    pub fn get_node(&self, id: valori_kernel::types::id::NodeId) -> Option<&valori_kernel::graph::node::GraphNode> {
        self.state.get_node(id)
    }

    pub fn outgoing_edges(&self, id: valori_kernel::types::id::NodeId) -> Option<impl Iterator<Item = &valori_kernel::graph::edge::GraphEdge>> {
        self.state.outgoing_edges(id)
    }

    pub fn get_record(&self, id: valori_kernel::types::id::RecordId) -> Option<&valori_kernel::storage::record::Record> {
        self.state.get_record(id)
    }

    pub fn get_edge(&self, id: valori_kernel::types::id::EdgeId) -> Option<&valori_kernel::graph::edge::GraphEdge> {
        self.state.get_edge(id)
    }

    pub fn cosine_similarity(&self, id_a: u32, id_b: u32) -> Option<f32> {
        use valori_kernel::math::dot::dot_i32 as dot_product;
        use valori_kernel::types::id::RecordId;
        let rec_a = self.state.get_record(RecordId(id_a))?;
        let rec_b = self.state.get_record(RecordId(id_b))?;
        if !rec_a.is_searchable() || !rec_b.is_searchable() { return None; }
        let va: Vec<i32> = rec_a.vector.data.iter().map(|s| s.0).collect();
        let vb: Vec<i32> = rec_b.vector.data.iter().map(|s| s.0).collect();
        let dot = dot_product(&va, &vb) as f64;
        let mag_a = (dot_product(&va, &va) as f64).sqrt();
        let mag_b = (dot_product(&vb, &vb) as f64).sqrt();
        if mag_a == 0.0 || mag_b == 0.0 { return None; }
        Some((dot / (mag_a * mag_b)) as f32)
    }

    // ── Index management ──────────────────────────────────────────────────────

    pub fn build_index(&mut self) {
        let total_slots = self.state.total_record_slots();
        let mut records: Vec<(u32, Vec<f32>)> = Vec::with_capacity(total_slots);
        for i in 0..total_slots {
            if let Some(record) = self.state.get_record(RecordId(i as u32)) {
                if !record.is_searchable() { continue; }
                let vals: Vec<f32> = record.vector.data.iter()
                    .map(|fxp| fxp.0 as f32 / SCALE as f32)
                    .collect();
                records.push((i as u32, vals));
            }
        }
        self.index.build(&records);
    }

    pub fn rebuild_index(&mut self) {
        let target = self.effective_index_kind();
        let blank: Box<dyn VectorIndex + Send + Sync> = match target {
            IndexKind::BruteForce | IndexKind::Auto => Box::new(BruteForceIndex::new()),
            IndexKind::Hnsw => {
                use valori_index::HnswIndex;
                Box::new(HnswIndex::new_with_config(self.hnsw_config.clone()))
            }
            IndexKind::Ivf => {
                use valori_index::IvfIndex;
                let dim = self.state.dim.unwrap_or(0);
                Box::new(IvfIndex::new(self.ivf_config.clone(), dim))
            }
            IndexKind::Bq => {
                use valori_index::BqIndex;
                Box::new(BqIndex::new())
            }
        };
        self.index = blank;
        self.build_index();
    }

    pub fn effective_index_kind(&self) -> IndexKind {
        match self.index_kind {
            IndexKind::Auto => {
                let n = self.state.record_count();
                if n >= AUTO_TIER_HNSW_MIN       { IndexKind::Hnsw }
                else if n >= AUTO_TIER_BQ_MIN     { IndexKind::Bq }
                else                              { IndexKind::BruteForce }
            }
            other => other,
        }
    }

    pub fn auto_tier_check(&mut self) {
        if self.index_kind != IndexKind::Auto { return; }
        let target  = self.effective_index_kind();
        let current = self.current_effective_kind;
        if target != current {
            tracing::info!(from = ?current, to = ?target,
                records = self.state.record_count(), "auto-tier: switching index");
            self.current_effective_kind = target;
            self.rebuild_index();
        }
    }

    // ── Crash recovery ────────────────────────────────────────────────────────

    pub fn try_recover(&mut self) -> RecoveryMode {
        let log_info = self.event_committer().map(|c| {
            (c.event_log().path().to_path_buf(), c.event_log().dim())
        });

        if let Some((log_path, dim)) = log_info {
            if log_path.exists() {
                match valori_state::bootstrap::recover_from_events(&log_path) {
                    Ok((recovered_state, recovered_journal, count)) => {
                        if count == 0 {
                            tracing::info!("Event log exists but is empty; trying snapshot");
                        } else {
                            tracing::info!("Event-log recovery: replaying {} events from {:?}", count, log_path);
                            self.persistence = Persistence::Ephemeral;
                            match EventLogWriter::open(&log_path, Some(dim)) {
                                Ok(log_writer) => {
                                    let state_for_committer = recovered_state.clone();
                                    self.state = recovered_state;
                                    self.persistence = Persistence::EventLog(EventCommitter::new(
                                        log_writer, recovered_journal, state_for_committer,
                                    ));
                                    self.rebuild_index();
                                    self.auto_tier_check();
                                    self.rebuild_record_to_node();
                                    self.load_metadata().ok();
                                    self.sync_metadata_from_state();
                                    self.load_namespaces().ok();
                                    return RecoveryMode::EventLog(count);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to reopen event log after recovery: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Event-log recovery failed ({:?}); trying snapshot", e);
                    }
                }
            }
        }

        let mut snapshot_recovered = false;
        if let Some(path) = self.snapshot_path.clone() {
            if path.exists() {
                match std::fs::read(&path) {
                    Ok(data) => match self.restore(&data) {
                        Ok(()) => {
                            tracing::info!("Snapshot recovery succeeded from {:?}", path);
                            snapshot_recovered = true;
                        }
                        Err(e) => tracing::error!("Snapshot restore failed ({:?}); starting fresh", e),
                    },
                    Err(e) => tracing::error!("Failed to read snapshot file {:?}: {}", path, e),
                }
            }
        }

        // Legacy WAL fallback — only attempted when the snapshot step above
        // did NOT already recover a state. `save_snapshot()` never truncates
        // or rotates the WAL (unlike `EventLogWriter::rotate`, which splices
        // the chain at a checkpoint), so a WAL file can contain the FULL
        // history including everything the snapshot already covers; replaying
        // all of it on top of a snapshot-restored state would immediately hit
        // a duplicate-id rejection on the first pre-snapshot record. Treating
        // snapshot and WAL as either/or (not layered) avoids that, and still
        // fixes the actual reported gap: before this, `try_recover` never
        // looked at the WAL at all, so a restart under `Persistence::Wal`
        // (no snapshot configured, or snapshot never taken) silently lost
        // every command ever written — fell straight through to
        // `RecoveryMode::Fresh`.
        if !snapshot_recovered {
            if let Some(wal_path) = self.wal_path.clone() {
                if wal_path.exists() {
                    match valori_state::bootstrap::replay_wal(&mut self.state, &wal_path) {
                        Ok((count, _hasher)) if count > 0 => {
                            tracing::info!("WAL recovery: replayed {} commands from {:?}", count, wal_path);
                            self.rebuild_index();
                            self.auto_tier_check();
                            self.rebuild_record_to_node();
                            self.load_metadata().ok();
                            self.sync_metadata_from_state();
                            self.load_namespaces().ok();
                            return RecoveryMode::Wal(count);
                        }
                        Ok(_) => {} // WAL exists but is empty — nothing to replay.
                        Err(e) => tracing::error!("WAL replay failed ({:?})", e),
                    }
                }
            }
        }

        if snapshot_recovered {
            self.load_metadata().ok();
            self.sync_metadata_from_state();
            self.load_namespaces().ok();
            return RecoveryMode::Snapshot;
        }

        self.load_namespaces().ok();
        tracing::info!("No durable state found; starting from an empty store");
        RecoveryMode::Fresh
    }

    fn restore_from_components(
        &mut self,
        k_data: &[u8],
        m_data: &[u8],
        i_data: Option<&[u8]>,
        ns_registry: Option<CollectionRegistry>,
    ) -> Result<(), EngineError> {
        self.state = decode_state(k_data)?;
        if !m_data.is_empty() { self.metadata.restore(m_data); }
        match i_data {
            Some(blob) if !blob.is_empty() => {
                self.index.restore(blob).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            }
            _ => self.rebuild_index(),
        }
        self.auto_tier_check();
        self.rebuild_record_to_node();
        if let Some(reg) = ns_registry { self.namespaces = reg; }
        Ok(())
    }

    fn restore_trailing_sections(&mut self, data: &[u8], mut offset: usize) {
        while offset + 8 <= data.len() {
            let tag = &data[offset..offset + 4];
            let section_len = u32::from_le_bytes(
                data[offset + 4..offset + 8].try_into().unwrap_or([0; 4])
            ) as usize;
            offset += 8;
            if offset + section_len > data.len() { break; }
            let section = &data[offset..offset + section_len];
            offset += section_len;

            if tag == b"CRTS" {
                if let Ok((map, _)) = bincode::serde::decode_from_slice::<HashMap<u32, u64>, _>(
                    section, bincode::config::standard()
                ) {
                    self.created_at = map;
                }
            } else if tag == b"BCRP" {
                use std::collections::HashMap as StdMap;
                if let Ok(((corpus, total_tokens), _)) = bincode::serde::decode_from_slice::<(StdMap<u64, Vec<String>>, usize), _>(
                    section, bincode::config::standard()
                ) {
                    self.reranker.restore_corpus(corpus, total_tokens);
                }
            }
        }
    }
}

// ── Drop ─────────────────────────────────────────────────────────────────────

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(committer) = self.persistence.event_committer_mut() {
            let _ = committer.flush_pending();
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_u32(data: &[u8], offset: &mut usize, field: &'static str) -> Result<u32, EngineError> {
    if *offset + 4 > data.len() {
        return Err(EngineError::InvalidInput(format!("Truncated snapshot: missing {field}")));
    }
    let val = u32::from_le_bytes(data[*offset..*offset + 4].try_into()
        .map_err(|_| EngineError::InvalidInput(format!("Failed to read {field}")))?);
    *offset += 4;
    Ok(val)
}

fn slice_at<'a>(data: &'a [u8], offset: &mut usize, len: usize, field: &'static str) -> Result<&'a [u8], EngineError> {
    if *offset + len > data.len() {
        return Err(EngineError::InvalidInput(format!("Truncated snapshot: {field} out of bounds")));
    }
    let s = &data[*offset..*offset + len];
    *offset += len;
    Ok(s)
}

fn pct(used: usize, capacity: usize) -> f64 {
    if capacity == 0 { 0.0 } else { used as f64 / capacity as f64 * 100.0 }
}

fn round1(v: f64) -> f64 { (v * 10.0).round() / 10.0 }

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EngineConfig, IndexKind, QuantizationKind};
    use valori_kernel::crypto::{KeyVault, CryptoError};

    struct NoopVault;
    impl KeyVault for NoopVault {
        fn encrypt(&self, _key_id: [u8; 16], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
            Ok(plaintext.to_vec())
        }
        fn decrypt(&self, _key_id: [u8; 16], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
            Ok(ciphertext.to_vec())
        }
        fn shred(&self, _key_id: [u8; 16]) -> Result<(), CryptoError> { Ok(()) }
        fn key_exists(&self, _key_id: &[u8; 16]) -> bool { true }
    }

    fn tiny_cfg() -> EngineConfig {
        EngineConfig {
            dim: 4,
            max_records: 100,
            max_nodes: 32,
            max_edges: 64,
            index_kind: IndexKind::BruteForce,
            quantization_kind: QuantizationKind::None,
            hnsw_m: None,
            hnsw_ef_construction: None,
            hnsw_ef_search: None,
            ivf_n_list: None,
            ivf_n_probe: None,
            snapshot_path: None,
            wal_path: None,
            event_log_path: None,
            event_log_rotation_bytes: None,
            decay_half_life_secs: None,
            shard_count: 1,
            object_store_keep: 7,
            object_store: None,
            vault: Arc::new(NoopVault),
            embed_config: None,
        }
    }

    #[test]
    fn insert_and_search() {
        let mut e = Engine::with_config(tiny_cfg());
        e.create_collection("default").unwrap();
        let id = e.insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0]).unwrap();
        let results = e.search_l2(&[1.0, 0.0, 0.0, 0.0], 1).unwrap();
        assert_eq!(results[0].0, id);
    }

    #[test]
    fn health_reports_ok() {
        let e = Engine::with_config(tiny_cfg());
        assert_eq!(e.health().status, "ok");
    }

    #[test]
    fn soft_delete_removes_from_index() {
        let mut e = Engine::with_config(tiny_cfg());
        e.create_collection("default").unwrap();
        let id = e.insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0]).unwrap();
        e.soft_delete_record(id).unwrap();
        let results = e.search_l2(&[1.0, 0.0, 0.0, 0.0], 1).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn snapshot_roundtrip() {
        let mut e = Engine::with_config(tiny_cfg());
        e.create_collection("default").unwrap();
        e.insert_record_from_f32(&[0.5, 0.5, 0.5, 0.5]).unwrap();
        let snap = e.snapshot().unwrap();

        let mut e2 = Engine::with_config(tiny_cfg());
        e2.restore(&snap).unwrap();
        assert_eq!(e2.record_count(), 1);
    }

    #[test]
    fn collection_create_and_drop() {
        let mut e = Engine::with_config(tiny_cfg());
        e.create_collection("test").unwrap();
        assert!(e.list_collections().iter().any(|(n, _)| n == "test"));
        e.drop_collection("test").unwrap();
        assert!(!e.list_collections().iter().any(|(n, _)| n == "test"));
    }
}
