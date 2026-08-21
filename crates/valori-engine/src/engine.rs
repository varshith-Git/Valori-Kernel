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

use valori_index::{BruteForceIndex, NoQuantizer, Quantizer, ScalarQuantizer, VectorIndex};
use valori_metadata::CollectionRegistry;
use valori_storage::events::event_commit::EventCommitter;
use valori_storage::events::event_journal::EventJournal;
use valori_storage::events::event_log::EventLogWriter;
use valori_storage::provider::StorageProvider;

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
    pub collections: usize,
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
    /// Phase 2.1: recovered via the manifest-driven, `StorageProvider`-backed
    /// snapshot + WAL-tail path (`valori_state::collection_bootstrap::recover_project_with_wal_tail`).
    /// The `u64` is the highest LSN actually applied. Only attempted when
    /// `Engine.storage_provider`/`project_id` are configured AND at least
    /// one collection manifest was discovered — see `try_recover`'s doc
    /// comment for the exact fallback order.
    StorageProvider(u64),
}

/// Application-layer caches that sit above the database layer.
pub struct ExecutionResources {
    pub tree_cache: HashMap<String, valori_rag::tree::TreeIndex>,
    pub community_store: Option<valori_rag::community::CommunityStore>,
}

impl ExecutionResources {
    fn new() -> Self {
        Self {
            tree_cache: HashMap::new(),
            community_store: None,
        }
    }
}

// ── Engine ────────────────────────────────────────────────────────────────────

/// The Node Engine orchestrates state, persistence, and indexing.
pub struct Engine {
    pub state: KernelState,
    pub metadata: MetadataStore,
    pub quant: Box<dyn Quantizer + Send + Sync>,

    pub quantization_kind: QuantizationKind,
    pub wal_path: Option<PathBuf>,
    pub snapshot_path: Option<PathBuf>,

    pub max_records: usize,
    pub max_nodes: usize,
    pub max_edges: usize,

    pub persistence: Persistence,
    pub metadata_path: Option<PathBuf>,

    pub created_at: HashMap<u32, u64>,

    pub namespaces: CollectionRegistry,
    pub namespaces_path: Option<PathBuf>,

    /// Dedicated `dyn VectorIndex` per explicitly-configured collection —
    /// this is what makes collections independently dimensioned/indexed
    /// instead of sharing one project-wide index post-filtered by namespace.
    /// A namespace absent here has no explicit config and continues to use
    /// the single legacy `self.index` (exactly today's behavior). Not
    /// serialized: rebuilt from `self.state`'s records after snapshot
    /// restore / WAL replay / sidecar load, the same way `self.index` is.
    pub collection_indexes: HashMap<u16, Box<dyn VectorIndex + Send + Sync>>,

    /// Phase 4: per-collection index lifecycle state.
    /// Tracks desired/active/building generations for each namespace.
    /// Not replicated — derived from the collection's records; survives restarts
    /// by re-building the active generation from records (same as before Phase 4).
    pub index_states: HashMap<u16, crate::index_manager::CollectionIndexState>,

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

    /// Phase 2.1: when set, collection creation publishes a
    /// `CollectionManifest`, snapshot creation can durably materialize a
    /// per-collection artifact, and `try_recover()` attempts the
    /// manifest-driven snapshot+WAL-tail recovery path FIRST — see
    /// `configure_storage_provider`'s doc comment. `None` (the default for
    /// every existing caller/test) preserves this phase's exact
    /// pre-existing behavior: no manifest publication, no change to
    /// `try_recover()`'s legacy whole-process path.
    pub storage_provider: Option<Arc<dyn StorageProvider>>,
    pub project_id: Option<valori_domain::ProjectId>,
}

impl Engine {
    fn make_collection_index(
        &self,
        kind: IndexKind,
        dim: usize,
    ) -> Box<dyn VectorIndex + Send + Sync> {
        match kind {
            IndexKind::BruteForce | IndexKind::Auto => Box::new(BruteForceIndex::new()),
            IndexKind::Hnsw => {
                use valori_index::HnswIndex;
                Box::new(HnswIndex::new_with_config(self.hnsw_config.clone()))
            }
            IndexKind::Ivf => {
                use valori_index::IvfIndex;
                Box::new(IvfIndex::new(self.ivf_config.clone(), dim))
            }
            IndexKind::Bq => {
                use valori_index::BqIndex;
                Box::new(BqIndex::new())
            }
        }
    }

    /// Ensure `namespace_id` has a `collection_indexes` entry matching
    /// `index_kind`, creating (and, if records already exist for this
    /// namespace, rebuilding) it if absent. Idempotent. Also mirrors the
    /// config into `self.namespaces.configs` so `Engine::resolve_collection`
    /// / the collection-listing API see it even when it arrived via Raft
    /// replay rather than `create_collection_with_config`.
    pub fn ensure_collection_index(&mut self, namespace_id: u16, dim: usize, index_kind_wire: u8) {
        let kind = IndexKind::from_u8(index_kind_wire).unwrap_or(IndexKind::BruteForce);
        if !self.namespaces.configs.contains_key(&namespace_id) {
            self.namespaces.configs.insert(
                namespace_id,
                valori_metadata::collection::CollectionVectorConfig {
                    dim: dim as u32,
                    metric: valori_domain::Metric::SquaredL2,
                },
            );
        }
        self.namespaces.set_desired_index(
            namespace_id,
            match kind {
                IndexKind::BruteForce => valori_domain::IndexKind::Brute,
                IndexKind::Hnsw => valori_domain::IndexKind::Hnsw,
                IndexKind::Ivf => valori_domain::IndexKind::Ivf,
                IndexKind::Bq => valori_domain::IndexKind::Bq,
                IndexKind::Auto => valori_domain::IndexKind::Auto,
            },
        );
        // BruteForce collections deliberately have NO dedicated index object:
        // `KernelState::search_l2_ns`'s existing per-namespace linked-list
        // scan is already exact, O(N_tenant), and namespace-isolated.
        if kind == IndexKind::BruteForce {
            self.collection_indexes.remove(&namespace_id);
            return;
        }
        if self.collection_indexes.contains_key(&namespace_id) {
            return;
        }
        let mut idx = self.make_collection_index(kind, dim);
        let records: Vec<(u32, Vec<f32>)> = self
            .state
            .iter_records_in_ns(namespace_id)
            .filter(|r| r.is_searchable())
            .map(|r| {
                let vals: Vec<f32> = r
                    .vector
                    .data
                    .iter()
                    .map(|fxp| fxp.0 as f32 / SCALE as f32)
                    .collect();
                (r.id.0, vals)
            })
            .collect();
        idx.build(&records);
        self.collection_indexes.insert(namespace_id, idx);
        // Phase 4: sync the index_states map to reflect this synchronously-built
        // index (collection creation, restart rebuild). Idempotent — if a
        // lifecycle-managed build already set up the state, don't overwrite it.
        let type_str = match kind {
            IndexKind::Hnsw => "hnsw",
            IndexKind::Ivf => "ivf",
            IndexKind::Bq => "bq",
            _ => return,
        };
        let state = self
            .index_states
            .entry(namespace_id)
            .or_insert_with(crate::index_manager::CollectionIndexState::new);
        // Only set up the default state if no lifecycle-managed generation exists
        if state.active_generation.is_none() && state.building_generation.is_none() {
            let lsn = self
                .persistence
                .event_committer()
                .map(|c| c.journal().committed_height())
                .unwrap_or(0);
            let spec = crate::index_manager::IndexSpec {
                index_type: type_str.to_string(),
                parameters: serde_json::json!({}),
            };
            let gen = state.start_build(spec.clone(), lsn);
            state.mark_ready(gen);
            state.activate(gen);
            state.desired = Some(spec);
        }
    }

    /// The effective dimension for `namespace_id`: resolved from its
    /// explicit collection config or kernel namespace config.
    pub fn namespace_effective_dim(&self, namespace_id: u16) -> Option<usize> {
        self.namespaces
            .config(namespace_id)
            .map(|c| c.dim as usize)
            .or_else(|| self.state.namespace_dim(namespace_id))
    }

    /// Reconstruct `self.collection_indexes` and `self.namespaces.configs`
    /// from `self.state.namespace_configs` — the source of truth replicated
    /// via Raft / persisted in the snapshot. Call after snapshot restore,
    /// WAL replay, or any path that doesn't go through
    /// `create_collection_with_config` (which already keeps both in sync).
    pub fn sync_collection_indexes_from_state(&mut self) {
        let entries: Vec<(u16, u32, u8)> = self
            .state
            .namespace_configs
            .iter()
            .map(|(&ns, cfg)| (ns, cfg.dim, cfg.index_kind))
            .collect();
        for (ns, dim, index_kind_wire) in entries {
            self.ensure_collection_index(ns, dim as usize, index_kind_wire);
        }
    }

    /// Primary constructor. `valori-node` wraps this via the `EngineFromNodeConfig`
    /// extension trait so existing `Engine::new(&node_config)` call sites compile
    /// unchanged after importing that trait.
    pub fn with_config(cfg: EngineConfig) -> Self {
        let quant: Box<dyn Quantizer + Send + Sync> = match cfg.quantization_kind {
            QuantizationKind::None => Box::new(NoQuantizer),
            QuantizationKind::Scalar => Box::new(ScalarQuantizer {}),
            QuantizationKind::Product => Box::new(NoQuantizer),
        };

        let persistence = if let Some(ref path) = cfg.event_log_path {
            match EventLogWriter::open(path, None) {
                Ok(log_writer) => {
                    let journal = EventJournal::new();
                    let live_state = KernelState::new();
                    let mut committer = EventCommitter::new(log_writer, journal, live_state);
                    if let Some(limit) = cfg.event_log_rotation_bytes {
                        committer = committer.with_rotation_bytes(if limit == 0 {
                            None
                        } else {
                            Some(limit)
                        });
                    }
                    Persistence::EventLog(committer)
                }
                Err(e) => {
                    tracing::error!("Failed to open Event Log: {}", e);
                    Persistence::Ephemeral
                }
            }
        } else if let Some(ref path) = cfg.wal_path {
            match valori_storage::wal_writer::WalWriter::open(path, 0) {
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

        let metadata_path = cfg
            .event_log_path
            .as_ref()
            .map(|p| p.with_extension("metadata.json"));
        let namespaces_path = cfg
            .event_log_path
            .as_ref()
            .or(cfg.snapshot_path.as_ref())
            .map(|p| p.with_extension("namespaces.json"));

        let kernel_state = KernelState::new();

        let hnsw_config = {
            use valori_index::HnswConfig;
            let mut c = HnswConfig::default();
            if let Some(m) = cfg.hnsw_m {
                c.m = m;
                c.m_max0 = m * 2;
                c.lambda = 1.0 / (m as f64).ln();
            }
            if let Some(ef) = cfg.hnsw_ef_construction {
                c.ef_construction = ef;
            }
            if let Some(ef) = cfg.hnsw_ef_search {
                c.ef_search = ef;
            }
            c
        };
        let ivf_config = {
            use valori_index::IvfConfig;
            let auto_scale = cfg.ivf_n_list.is_none() && cfg.ivf_n_probe.is_none();
            IvfConfig {
                n_list: cfg.ivf_n_list.unwrap_or(100),
                n_probe: cfg.ivf_n_probe.unwrap_or(10),
                auto_scale,
            }
        };

        Self {
            state: kernel_state,
            metadata: MetadataStore::new(),
            quant,
            quantization_kind: cfg.quantization_kind,
            wal_path: cfg.wal_path,
            snapshot_path: cfg.snapshot_path,
            max_records: cfg.max_records,
            max_nodes: cfg.max_nodes,
            max_edges: cfg.max_edges,
            persistence,
            created_at: HashMap::new(),
            metadata_path,
            namespaces: CollectionRegistry::new(),
            namespaces_path,
            collection_indexes: HashMap::new(),
            index_states: HashMap::new(),
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
            storage_provider: None,
            project_id: None,
        }
    }

    #[inline]
    pub fn shard_for_ns(&self, namespace_id: u16) -> usize {
        if self.shard_count <= 1 {
            0
        } else {
            namespace_id as usize % self.shard_count
        }
    }

    fn commit_and_apply_ns(
        &mut self,
        event: &valori_kernel::event::KernelEvent,
        namespace_id: u16,
    ) -> Result<(), EngineError> {
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

    // ── Metadata sidecar ─────────────────────────────────────────────────────

    pub fn flush_metadata(&self) -> Result<(), EngineError> {
        if let Some(ref path) = self.metadata_path {
            self.metadata.flush_to(path).map_err(|e| {
                EngineError::InvalidInput(format!("Failed to flush metadata sidecar: {}", e))
            })?;
        }
        Ok(())
    }

    pub fn load_metadata(&mut self) -> Result<(), EngineError> {
        if let Some(ref path) = self.metadata_path {
            self.metadata.load_from(path).map_err(|e| {
                EngineError::InvalidInput(format!("Failed to load metadata sidecar: {}", e))
            })?;
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

    pub fn set_meta_audited(
        &mut self,
        key: String,
        value: serde_json::Value,
    ) -> Result<(), EngineError> {
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
            let json = serde_json::to_vec(&self.namespaces).map_err(|e| {
                EngineError::InvalidInput(format!("Failed to serialize namespace registry: {}", e))
            })?;
            let tmp = {
                let mut s = path.clone().into_os_string();
                s.push(".tmp");
                PathBuf::from(s)
            };
            std::fs::write(&tmp, &json).map_err(|e| {
                EngineError::InvalidInput(format!("Failed to write namespace sidecar: {}", e))
            })?;
            std::fs::rename(&tmp, path).map_err(|e| {
                EngineError::InvalidInput(format!("Failed to commit namespace sidecar: {}", e))
            })?;
        }
        Ok(())
    }

    pub fn load_namespaces(&mut self) -> Result<(), EngineError> {
        if let Some(ref path) = self.namespaces_path {
            match std::fs::read(path) {
                Ok(bytes) => {
                    let reg: CollectionRegistry = serde_json::from_slice(&bytes).map_err(|e| {
                        EngineError::InvalidInput(format!(
                            "Failed to parse namespace sidecar: {}",
                            e
                        ))
                    })?;
                    self.namespaces = reg;
                    self.sync_collection_indexes_from_state();
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(EngineError::InvalidInput(format!(
                        "Failed to read namespace sidecar: {}",
                        e
                    )))
                }
            }
        }
        Ok(())
    }

    // ── Observability ─────────────────────────────────────────────────────────

    pub fn health(&self) -> EngineHealth {
        let live_records = self.state.record_count();
        let slot_records = self.state.total_record_slots();
        let live_nodes = self.state.node_count();
        let live_edges = self.state.edge_count();

        let rec_fill = pct(live_records, self.max_records);
        let node_fill = pct(live_nodes, self.max_nodes);
        let edge_fill = pct(live_edges, self.max_edges);

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
            collections: self.namespaces.len(),
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
            event_log_height: self
                .event_committer()
                .map(|c| c.journal().committed_height()),
            event_log_path: self
                .event_committer()
                .map(|c| c.event_log().path().to_string_lossy().into_owned()),
            snapshot_path: self
                .snapshot_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            embed_enabled: self.embed_config.is_some(),
            embed_provider: self.embed_config.as_ref().map(|c| c.provider.clone()),
            shard_count: self.shard_count,
        }
    }

    pub fn update_prometheus_metrics(&self) {
        let live_records = self.state.record_count() as f64;
        let live_nodes = self.state.node_count() as f64;
        let live_edges = self.state.edge_count() as f64;

        metrics::gauge!("valori_records_live", live_records);
        metrics::gauge!("valori_records_capacity", self.max_records as f64);
        metrics::gauge!(
            "valori_record_fill_ratio",
            if self.max_records > 0 {
                live_records / self.max_records as f64
            } else {
                0.0
            }
        );

        metrics::gauge!("valori_nodes_live", live_nodes);
        metrics::gauge!("valori_nodes_capacity", self.max_nodes as f64);
        metrics::gauge!(
            "valori_node_fill_ratio",
            if self.max_nodes > 0 {
                live_nodes / self.max_nodes as f64
            } else {
                0.0
            }
        );

        metrics::gauge!("valori_edges_live", live_edges);
        metrics::gauge!("valori_edges_capacity", self.max_edges as f64);
        metrics::gauge!(
            "valori_edge_fill_ratio",
            if self.max_edges > 0 {
                live_edges / self.max_edges as f64
            } else {
                0.0
            }
        );

        // +1 for the implicit `default` namespace, which never gets an
        // entry in the registry map (id 0 is reserved for it — see
        // CollectionRegistry::new).
        metrics::gauge!(
            "valori_collections_total",
            self.namespaces.map.len() as f64 + 1.0
        );

        if let Some(c) = self.event_committer() {
            metrics::gauge!(
                "valori_event_log_height",
                c.journal().committed_height() as f64
            );
        }
    }

    /// Serialized size of all per-collection vector indexes in bytes.
    pub fn index_size_bytes(&self) -> Option<usize> {
        let total: usize = self
            .collection_indexes
            .values()
            .filter_map(|idx| idx.snapshot().ok().map(|b| b.len()))
            .sum();
        Some(total)
    }

    // ── Inserts ───────────────────────────────────────────────────────────────

    pub fn insert_record_from_f32(&mut self, values: &[f32]) -> Result<u32, EngineError> {
        self.insert_record_from_f32_ns(values, valori_kernel::types::id::DEFAULT_NS.0)
    }

    pub fn insert_record_from_f32_ns(
        &mut self,
        values: &[f32],
        namespace_id: u16,
    ) -> Result<u32, EngineError> {
        self.insert_record_from_f32_ns_full(values, None, 0, namespace_id)
    }

    /// Single-record insert carrying the full public field set.
    ///
    /// Phase API-2: the standalone path used to drop `metadata` and `tag` on
    /// the floor while the cluster path committed them into the audited
    /// `InsertRecord` event. Both now commit the same event.
    pub fn insert_record_from_f32_ns_full(
        &mut self,
        values: &[f32],
        metadata: Option<Vec<u8>>,
        tag: u64,
        namespace_id: u16,
    ) -> Result<u32, EngineError> {
        if self.state.record_count() >= self.max_records {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }
        let mut fxp_data = Vec::with_capacity(values.len());
        for &v in values {
            if v > 32767.99 || v < -32768.0 {
                return Err(EngineError::InvalidInput(
                    "Vector values must be between -32768.0 and 32767.99".to_string(),
                ));
            }
            fxp_data.push(FxpScalar((v * SCALE as f32) as i32));
        }
        let vector = FxpVector { data: fxp_data };
        let rid = self.state.next_record_id();
        let event = valori_kernel::event::KernelEvent::InsertRecord {
            id: rid,
            vector,
            metadata,
            tag,
        };
        self.commit_and_apply_ns(&event, namespace_id)?;
        self.auto_tier_check();
        self.created_at.insert(rid.0, Self::now_unix());
        Ok(rid.0)
    }

    // ── Idempotency (Phase API-2) ────────────────────────────────────────────
    //
    // `batch_seen` was introduced for `insert_batch_ns`'s per-item
    // `request_ids`. It is exactly the standalone analogue of the cluster
    // state machine's replicated `dedup` table (`valori-consensus`
    // `StateMachineInner::dedup_map`), so single-record inserts reuse it
    // rather than introducing a second, differently-behaved table:
    //
    //   * same key type — the 16-byte client token,
    //   * same value — the record id the first request allocated,
    //   * same eviction discipline — bounded, oldest-first, so a retry that
    //     arrives after the window is no longer recognised.
    //
    // It is deliberately **not** part of the BLAKE3 state hash and is not
    // persisted in standalone mode: idempotency is a request-level
    // convenience, not replicated state, and a standalone restart legitimately
    // forgets in-flight tokens.

    /// The record id a previous request carrying this token allocated, if the
    /// token is still inside the dedup window.
    pub fn dedup_lookup(&self, request_id: &[u8; 16]) -> Option<u32> {
        self.batch_seen.get(request_id).copied()
    }

    /// Remember that `request_id` produced `record_id`. Bounded — the table is
    /// cleared wholesale once it exceeds its cap, mirroring `insert_batch_ns`.
    pub fn dedup_record(&mut self, request_id: [u8; 16], record_id: u32) {
        if self.batch_seen.len() >= 65536 {
            self.batch_seen.clear();
        }
        self.batch_seen.insert(request_id, record_id);
    }

    pub fn reranker_insert(&mut self, record_id: u32, text: &str) {
        self.reranker.insert(record_id as u64, text);
    }

    pub fn reranker_corpus_len(&self) -> usize {
        self.reranker.len()
    }

    pub fn reranker_rerank(
        &self,
        query_text: &str,
        _query_vec: &[f32],
        candidates: &[(u32, f32)],
    ) -> Vec<(u32, f32)> {
        let u64_candidates: Vec<(u64, f32)> =
            candidates.iter().map(|&(id, s)| (id as u64, s)).collect();
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

    pub fn insert_encrypted_ns(
        &mut self,
        plaintext: &[u8],
        tag: u64,
        namespace_id: u16,
        key_id: [u8; 16],
    ) -> Result<u32, EngineError> {
        if self.state.record_count() >= self.max_records {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }
        if self.namespace_effective_dim(namespace_id).is_none() {
            return Err(EngineError::InvalidInput(
                "Collection dimension must be configured before encrypted insert".into(),
            ));
        }
        let ciphertext = self
            .vault
            .encrypt(key_id, plaintext)
            .map_err(|e| EngineError::InvalidInput(format!("Vault encrypt: {e:?}")))?;
        let rid = self.state.next_record_id();
        let event = valori_kernel::event::KernelEvent::InsertRecordEncrypted {
            id: rid,
            key_id,
            ciphertext,
            metadata_ciphertext: None,
            tag,
        };
        self.commit_and_apply_ns(&event, namespace_id)?;
        Ok(rid.0)
    }

    pub fn shred_key(&mut self, key_id: [u8; 16]) -> Result<(), EngineError> {
        self.vault
            .shred(key_id)
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
        for (i, id) in &deduped {
            id_map[*i] = *id;
        }

        let mut events = Vec::with_capacity(insert_indices.len());
        let start_id = self.state.next_record_id().0;

        for (slot, &i) in insert_indices.iter().enumerate() {
            let values = &batch[i];
            let mut fxp_data = Vec::with_capacity(values.len());
            for &v in values {
                if v > 32767.99 || v < -32768.0 {
                    return Err(EngineError::InvalidInput(
                        "Vector values must be between -32768.0 and 32767.99".to_string(),
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

        self.persistence.log_batch_ns(&events, namespace_id)?;
        for event in &events {
            self.apply_committed_event_ns(event, namespace_id)?;
        }
        self.auto_tier_check();

        for &i in &insert_indices {
            if let Some(Some(rid)) = request_ids.and_then(|r| r.get(i)) {
                if self.batch_seen.len() >= 65536 {
                    self.batch_seen.clear();
                }
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

    pub fn search_l2_ns(
        &self,
        query: &[f32],
        k: usize,
        namespace_id: u16,
    ) -> Result<Vec<(u32, f32)>, EngineError> {
        use valori_kernel::index::SearchResult;

        // Resolve THIS collection's own dimension — its explicit config if
        // it has one, else the legacy process-wide dim. Never validates
        // against a different collection's dimension (the audit's own
        // "search(A) must never use B's config" isolation requirement).
        if let Some(dim) = self.namespace_effective_dim(namespace_id) {
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

        // A collection with its own dedicated index (Hnsw/Ivf/Bq) never
        // needs the post-hoc namespace filter the legacy global-index path
        // below still uses — by construction, only this namespace's records
        // were ever inserted into it (`post_apply_derived` routes there).
        if let Some(idx) = self.collection_indexes.get(&namespace_id) {
            let candidates = idx.search(query, k);
            let hits: Vec<(u32, f32)> = candidates.into_iter().take(k).collect();
            return Ok(hits);
        }

        let fxp_data: Vec<FxpScalar> = query
            .iter()
            .map(|&v| FxpScalar((v * SCALE as f32) as i32))
            .collect();
        let fxp_query = FxpVector { data: fxp_data };
        let mut results = vec![SearchResult::default(); k];
        let found = self
            .state
            .search_l2_ns(&fxp_query, &mut results, namespace_id);
        Ok(results[..found]
            .iter()
            .map(|r| (r.id.0, r.score as f32 / (SCALE as f32 * SCALE as f32)))
            .collect())
    }

    // ── Collections ───────────────────────────────────────────────────────────

    /// Tag-filtered brute-force L2 search across all records.
    ///
    /// When `tag` is `Some(t)`, only records whose stored `tag` field equals `t` are scored.
    /// `None` scores every active record (no tag restriction).
    ///
    /// Returns `(record_id, l2_distance_f32)` pairs in ascending distance order,
    /// using the same f32 scale as `search_l2_ns`.
    pub fn search_l2_filtered(
        &self,
        query: &[f32],
        k: usize,
        tag: Option<u64>,
    ) -> Result<Vec<(u32, f32)>, EngineError> {
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

        let fxp_data: Vec<FxpScalar> = query
            .iter()
            .map(|&v| FxpScalar((v * SCALE as f32) as i32))
            .collect();
        let fxp_query = FxpVector { data: fxp_data };
        let mut results = vec![SearchResult::default(); k];
        let found = self.state.search_l2(&fxp_query, &mut results, tag);
        Ok(results[..found]
            .iter()
            .map(|r| (r.id.0, r.score as f32 / (SCALE as f32 * SCALE as f32)))
            .collect())
    }

    /// BLAKE3 hash of the current kernel state, as a lowercase hex string.
    pub fn state_hash_hex(&self) -> String {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        hash_state_blake3(&self.state)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }

    pub fn resolve_collection(&self, name: Option<&str>) -> Result<u16, EngineError> {
        self.namespaces.resolve(name).ok_or_else(|| {
            // Phase API-2: `collection` is required on every data-path
            // request. `resolve(None)` has never fallen back to a name — a
            // Collection called "default" is an ordinary Collection, not an
            // implicit target — so saying "unknown collection 'default'" for
            // an omitted field sent people off to create something that
            // would not have helped. Say what is actually wrong.
            match name {
                Some(n) => EngineError::CollectionNotFound(format!(
                    "unknown collection '{n}' — create it first with POST /v1/namespaces"
                )),
                None => EngineError::CollectionNotFound(
                    "no collection specified — `collection` is required on this request; \
                     there is no implicit default collection (create one with \
                     POST /v1/namespaces and name it explicitly)"
                        .to_string(),
                ),
            }
        })
    }

    /// Create a collection with its own explicit dimension/metric/index.
    ///
    /// # The only way to create a collection (Phase 3.3)
    ///
    /// There is no config-free `create_collection(name)` any more — it was
    /// removed. Every collection, `"default"` included, must supply
    /// `dim`/`metric` here; `index` alone is optional (`IndexKind::Brute`
    /// means no dedicated ANN structure, not a request for a
    /// `BruteForceIndex` object). This is a correctness boundary enforced
    /// by the `Engine`'s own type signature, not by the HTTP router —
    /// there is no way to call into this module and produce an
    /// unconfigured modern collection; see
    /// `unconfigured_collection_creation_is_not_possible` below. A brand
    /// new project has zero collections until a caller explicitly invokes
    /// this method.
    ///
    /// Commits TWO events atomically in sequence — `AutoCreateNamespace`
    /// (existing, unchanged) then `ConfigureNamespace` (new) — rather than
    /// adding fields to `AutoCreateNamespace` itself, which would corrupt
    /// every already-persisted WAL entry of that variant. Both go through
    /// `commit_and_apply_ns`, so both are durable and, in cluster mode, both
    /// are Raft-committed and applied identically on every replica.
    pub fn create_collection_with_config(
        &mut self,
        name: &str,
        dim: u32,
        metric: valori_domain::Metric,
        index: valori_domain::IndexKind,
    ) -> Result<u16, EngineError> {
        if dim == 0 || dim as usize > valori_kernel::config::MAX_DIM {
            return Err(EngineError::InvalidInput(format!(
                "dimension must be between 1 and {}",
                valori_kernel::config::MAX_DIM
            )));
        }
        let id = self.namespaces.create(name).ok_or_else(|| {
            EngineError::InvalidInput(format!(
                "namespace limit reached ({} max)",
                valori_kernel::types::id::MAX_NAMESPACES
            ))
        })?;
        self.commit_and_apply_ns(
            &valori_kernel::event::KernelEvent::AutoCreateNamespace {
                name: String::new(),
            },
            id,
        )?;
        let engine_kind = IndexKind::from_domain(index);
        self.commit_and_apply_ns(
            &valori_kernel::event::KernelEvent::ConfigureNamespace {
                namespace_id: id,
                dim,
                metric: metric.as_u8(),
                index_kind: engine_kind.as_u8(),
            },
            id,
        )?;
        self.flush_namespaces()?;

        // Phase 2.1 §4: make CollectionManifest live. Only when a storage
        // provider is configured — see `configure_storage_provider`'s doc
        // comment for why this is additive, not a hard requirement, in
        // this phase. A publish failure here does NOT roll back the
        // already-committed collection (the collection genuinely exists
        // and is usable either way); it's logged so an operator can see a
        // storage-layer problem without the collection-creation request
        // itself failing for a reason unrelated to collection creation.
        if let (Some(provider), Some(project_id)) = (&self.storage_provider, self.project_id) {
            if let Err(e) = valori_state::collection_bootstrap::publish_collection_manifest(
                provider.as_ref(),
                project_id,
                valori_kernel::types::id::NamespaceId(id),
                dim,
                metric,
            ) {
                tracing::error!("Failed to publish collection manifest for namespace {id}: {e}");
            }
        }

        Ok(id)
    }

    /// Wire a durable `StorageProvider` into this engine (Phase 2.1). Once
    /// set:
    /// - `create_collection_with_config` publishes a `CollectionManifest`
    ///   for every new collection.
    /// - `snapshot_collection_to_storage` becomes usable to durably
    ///   materialize one collection's state, republishing its manifest.
    /// - `try_recover()` attempts `valori_state::collection_bootstrap::recover_project_with_wal_tail`
    ///   FIRST, before falling back to the legacy whole-process path.
    ///
    /// As of Phase 2.3, `valori-node`'s `main.rs` calls this automatically
    /// whenever the node was started with both `VALORI_STORAGE_ROOT` and
    /// `VALORI_PROJECT_ID` set — which is what makes the StorageProvider
    /// path the actual default for a normally-daemon-spawned project,
    /// rather than something only tests exercise. A node without either
    /// env var keeps the pre-existing legacy path, unchanged.
    ///
    /// Also ensures a `ProjectManifest` exists (publishing one if this is
    /// the first time this project has been configured with a provider) —
    /// idempotent, never overwrites an existing manifest, since this method
    /// runs on every startup, not only project creation.
    pub fn configure_storage_provider(
        &mut self,
        provider: Arc<dyn StorageProvider>,
        project_id: valori_domain::ProjectId,
        project_name: Option<&str>,
    ) {
        self.storage_provider = Some(provider.clone());
        self.project_id = Some(project_id);

        if let Some(committer) = self.event_committer_mut() {
            committer.set_storage_provider(provider.clone(), project_id, valori_core::ShardId(0));
        }

        if valori_state::collection_bootstrap::discover_project(provider.as_ref(), project_id)
            .ok()
            .flatten()
            .is_none()
        {
            let name = project_name
                .and_then(|n| n.parse::<valori_domain::ProjectName>().ok())
                .unwrap_or_else(|| {
                    format!("project-{}", project_id)
                        .parse::<valori_domain::ProjectName>()
                        .expect("a project_id-derived fallback name is always valid")
                });
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Err(e) = valori_state::collection_bootstrap::publish_project_manifest(
                provider.as_ref(),
                project_id,
                name,
                valori_domain::ProjectTopology::STANDALONE,
                now,
            ) {
                tracing::warn!("Failed to publish initial ProjectManifest: {e}");
            }
        }
    }

    /// The current authoritative LSN — the shard-wide committed-event
    /// count. Equal to `EventJournal.committed_height()` when the event
    /// log is the active persistence backend (the canonical, WAL-durable
    /// path); falls back to the kernel's own `state.version()` apply
    /// counter otherwise (e.g. `Persistence::Ephemeral`, where there is no
    /// journal to ask) — both increment exactly once per successful apply,
    /// so they agree whenever both exist.
    pub fn current_lsn(&self) -> valori_storage::collection_manifest::Lsn {
        let height = self
            .event_committer()
            .map(|c| c.journal().committed_height())
            .unwrap_or_else(|| self.state.version());
        valori_storage::collection_manifest::Lsn(height)
    }

    /// Durably snapshot one collection's current state to the configured
    /// `StorageProvider`, as the next generation, then republish its
    /// manifest to point at it (Phase 2.1 §14/§15's mandatory ordering —
    /// see `valori_state::collection_bootstrap::snapshot_collection`'s doc
    /// comment for the crash-safety argument).
    pub fn snapshot_collection_to_storage(
        &self,
        collection_id: valori_kernel::types::id::NamespaceId,
        generation: u32,
    ) -> Result<(), EngineError> {
        let (provider, project_id) = self
            .storage_provider
            .as_ref()
            .zip(self.project_id)
            .ok_or_else(|| {
                EngineError::InvalidInput("no StorageProvider configured on this engine".into())
            })?;
        let metric = self
            .namespaces
            .config(collection_id.0)
            .map(|c| c.metric)
            .unwrap_or_default();
        valori_state::collection_bootstrap::snapshot_collection(
            provider.as_ref(),
            project_id,
            &self.state,
            collection_id,
            generation,
            self.current_lsn(),
            metric,
        )
        .map_err(|e| EngineError::InvalidInput(e.to_string()))
    }

    pub fn drop_collection(&mut self, name: &str) -> Result<(), EngineError> {
        // Phase 3.3: "default" has no special meaning — droppable like any
        // other explicitly-created collection. (Namespace 0 itself is still
        // permanently undroppable at the kernel level — see
        // `CollectionRegistry::new`'s doc comment — but that's an id-based
        // restriction nothing here ever allocates a real collection into,
        // not a name-based one.)
        let id = self
            .namespaces
            .drop(name)
            .ok_or_else(|| EngineError::InvalidInput(format!("collection '{name}' not found")))?;
        let ns_record_ids: Vec<u64> = self
            .state
            .iter_records_in_ns(id)
            .map(|r| r.id.0 as u64)
            .collect();
        // S8 fix — same bug and same fix as create_collection() above:
        // route through the durable "log then apply" helper instead of
        // mutating state (and bumping its hashed version counter) directly.
        self.commit_and_apply_ns(
            &valori_kernel::event::KernelEvent::DropNamespace {
                name: String::new(),
            },
            id,
        )?;
        self.collection_indexes.remove(&id);
        self.reranker.remove_batch(&ns_record_ids);
        self.flush_namespaces()?;
        Ok(())
    }

    pub fn list_collections(&self) -> Vec<(String, u16)> {
        self.namespaces.list()
    }

    // ── Snapshot ──────────────────────────────────────────────────────────────

    /// Write the complete engine snapshot to `w` without buffering the kernel
    /// section in RAM.  The envelope format is identical to [`snapshot`].
    ///
    /// Requires `W: Write + Seek` so the kernel-section length (written as a
    /// 4-byte prefix before the kernel bytes) can be patched in after the
    /// kernel bytes are written — avoiding any need to know the size upfront.
    ///
    /// For saving to disk, use a `BufWriter<File>` (1 MB buffer recommended).
    /// For in-memory use (e.g. HTTP download), use a `Cursor<Vec<u8>>`.
    pub fn write_snapshot_to_writer<W: std::io::Write + std::io::Seek>(
        &self,
        w: &mut W,
    ) -> Result<(), EngineError> {
        use std::io::{Seek, SeekFrom, Write};
        use valori_kernel::snapshot::encode::encode_state_to_writer;

        let io_err = |e: std::io::Error| EngineError::InvalidInput(e.to_string());

        // Magic
        w.write_all(b"VAL1").map_err(io_err)?;

        // Kernel section — streamed record-by-record; length patched in after.
        let len_pos = w.stream_position().map_err(io_err)?;
        w.write_all(&0u32.to_le_bytes()).map_err(io_err)?; // placeholder
        let data_start = w.stream_position().map_err(io_err)?;
        encode_state_to_writer(&self.state, w)
            .map_err(|e| EngineError::InvalidInput(format!("kernel encode: {e}")))?;
        let data_end = w.stream_position().map_err(io_err)?;
        let k_len = (data_end - data_start) as u32;
        w.seek(SeekFrom::Start(len_pos)).map_err(io_err)?;
        w.write_all(&k_len.to_le_bytes()).map_err(io_err)?;
        w.seek(SeekFrom::Start(data_end)).map_err(io_err)?;

        // Metadata section (small — buffering is fine)
        let m_buf = self.metadata.snapshot();
        w.write_all(&(m_buf.len() as u32).to_le_bytes())
            .map_err(io_err)?;
        w.write_all(&m_buf).map_err(io_err)?;

        // Index section (legacy envelope format — empty index payload)
        w.write_all(&0u32.to_le_bytes()).map_err(io_err)?;

        // NSRG section
        let ns_json = serde_json::to_vec(&self.namespaces)
            .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        w.write_all(b"NSRG").map_err(io_err)?;
        w.write_all(&(ns_json.len() as u32).to_le_bytes())
            .map_err(io_err)?;
        w.write_all(&ns_json).map_err(io_err)?;

        // CRTS section
        let crts_buf = bincode::serde::encode_to_vec(&self.created_at, bincode::config::standard())
            .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        w.write_all(b"CRTS").map_err(io_err)?;
        w.write_all(&(crts_buf.len() as u32).to_le_bytes())
            .map_err(io_err)?;
        w.write_all(&crts_buf).map_err(io_err)?;

        // BCRP section
        let (corpus, total_tokens) = self.reranker.snapshot_corpus();
        let bcrp_buf =
            bincode::serde::encode_to_vec(&(corpus, total_tokens), bincode::config::standard())
                .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        w.write_all(b"BCRP").map_err(io_err)?;
        w.write_all(&(bcrp_buf.len() as u32).to_le_bytes())
            .map_err(io_err)?;
        w.write_all(&bcrp_buf).map_err(io_err)?;

        w.flush().map_err(io_err)?;
        Ok(())
    }

    /// Return the full snapshot as a `Vec<u8>`.
    ///
    /// Uses [`write_snapshot_to_writer`] internally so there is one encoding
    /// path.  Callers that save to disk should prefer [`save_snapshot`] which
    /// streams directly to a file and never materialises the full snapshot.
    pub fn snapshot(&self) -> Result<Vec<u8>, EngineError> {
        let hint = valori_kernel::snapshot::encode::encode_capacity_hint(&self.state) + 4096;
        let mut cursor = std::io::Cursor::new(Vec::with_capacity(hint));
        self.write_snapshot_to_writer(&mut cursor)?;
        Ok(cursor.into_inner())
    }

    /// S8 fix: flush any single-event commits (`commit_and_apply_ns`, used by
    /// `create_collection`/`drop_collection`) still sitting in the
    /// `EventCommitter`'s write buffer (`DEFAULT_WRITE_BUFFER_SIZE = 64` —
    /// batched inserts flush every batch unconditionally, but single events
    /// only flush once 64 of them accumulate or this is called explicitly).
    /// Previously only `Engine::drop()` called this, which does not
    /// reliably fire on graceful shutdown: `SharedEngine` is an
    /// `Arc<RwLock<Engine>>`, and background tasks (auto-snapshot,
    /// process-metrics) can hold clones past the point `with_graceful_shutdown`
    /// returns, so the Engine's strong count may never hit zero before the
    /// process exits. The shutdown path must call this explicitly — see
    /// `main.rs::shutdown_signal`, which now does, under a write lock,
    /// before saving the snapshot.
    pub fn flush_pending_events(&mut self) -> Result<(), EngineError> {
        if let Some(committer) = self.persistence.event_committer_mut() {
            committer
                .flush_pending()
                .map_err(|e| EngineError::InvalidInput(format!("flush pending events: {e}")))?;
        }
        Ok(())
    }

    /// Stream the snapshot directly to `path` via a buffered file writer.
    ///
    /// Peak RAM overhead is ~1 MB (the write buffer) regardless of state size.
    /// The write lock on the engine must already be held by the caller when
    /// invoked via the async capability layer.
    pub fn save_snapshot(&self, path: Option<&Path>) -> Result<PathBuf, EngineError> {
        let target = path
            .or(self.snapshot_path.as_deref())
            .ok_or(EngineError::InvalidInput(
                "No snapshot path configured".into(),
            ))?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(target)
            .map_err(|e| EngineError::InvalidInput(format!("open snapshot: {e}")))?;
        let mut w = std::io::BufWriter::with_capacity(1 << 20, file);
        self.write_snapshot_to_writer(&mut w)
            .map_err(|e| EngineError::InvalidInput(format!("snapshot write: {e}")))?;
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
            Some(
                serde_json::from_slice(ns_json)
                    .map_err(|e| EngineError::InvalidInput(format!("ns registry decode: {e}")))?,
            )
        } else {
            None
        };

        self.restore_from_components(k_data, m_data, i_data, ns_registry)?;
        self.restore_trailing_sections(data, offset);
        Ok(())
    }

    // ── Mutations ─────────────────────────────────────────────────────────────

    /// Soft delete leaves the record's row in place (flagged, not freed), so
    /// the `node.record ⇒ live record` invariant holds without touching the
    /// graph at all: any node referencing this record stays exactly as valid
    /// as it was before. See
    /// docs/reviews/graph-g1.3.1-record-graph-cascade-semantics.md §6.
    pub fn soft_delete_record(&mut self, id: u32) -> Result<(), EngineError> {
        let rid = RecordId(id);
        let event = valori_kernel::event::KernelEvent::SoftDeleteRecord { id: rid };
        self.commit_and_apply_ns(&event, valori_kernel::types::id::DEFAULT_NS.0)?;
        self.reranker.remove(id as u64);
        self.created_at.remove(&id);
        Ok(())
    }

    pub fn update_record_metadata(
        &mut self,
        id: u32,
        metadata: Option<Vec<u8>>,
        namespace_id: u16,
    ) -> Result<(), EngineError> {
        let rid = RecordId(id);
        let event = valori_kernel::event::KernelEvent::UpdateRecordMetadata { id: rid, metadata };
        self.commit_and_apply_ns(&event, namespace_id)
    }

    /// Hard delete frees the record's slot, so `node.record ⇒ live record`
    /// would be violated by any surviving referencing node (this makes the
    /// state's own snapshot undecodable — see G1.3.1 BUG-1). Cascade-delete
    /// every live node referencing this record, in ascending `NodeId` order
    /// (each `delete_node` also frees that node's incident edges), before
    /// freeing the record itself. See
    /// docs/reviews/graph-g1.3.1-record-graph-cascade-semantics.md §7-8.
    pub fn delete_record(&mut self, id: u32) -> Result<(), EngineError> {
        for node_id in valori_rag::graph::nodes_referencing_record(&self.state, id) {
            self.delete_node(node_id)?;
        }
        let rid = RecordId(id);
        let event = valori_kernel::event::KernelEvent::DeleteRecord { id: rid };
        self.commit_and_apply_ns(&event, valori_kernel::types::id::DEFAULT_NS.0)?;
        self.reranker.remove(id as u64);
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

    pub fn create_node_for_record(
        &mut self,
        record_id: Option<u32>,
        kind: u8,
        namespace_id: u16,
    ) -> Result<u32, EngineError> {
        if self.state.node_count() >= self.max_nodes {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }
        let node_id = self.state.next_node_id();
        let kind = NodeKind::from_u8(kind).unwrap_or_default();
        let record = record_id.map(RecordId);
        let event = valori_kernel::event::KernelEvent::CreateNode {
            id: node_id,
            kind,
            record,
        };
        self.commit_and_apply_ns(&event, namespace_id)?;
        Ok(node_id.0)
    }

    pub fn nodes_in_ns(&self, namespace_id: u16) -> Vec<(u32, u8, Option<u32>)> {
        self.state
            .iter_nodes()
            .filter(|n| n.namespace_id == namespace_id)
            .map(|n| (n.id.0, n.kind as u8, n.record.map(|r| r.0)))
            .collect()
    }

    pub fn create_edge(&mut self, from: u32, to: u32, kind: u8) -> Result<u32, EngineError> {
        self.create_edge_ns(from, to, kind, valori_kernel::types::id::DEFAULT_NS.0)
    }

    pub fn create_edge_ns(
        &mut self,
        from: u32,
        to: u32,
        kind: u8,
        namespace_id: u16,
    ) -> Result<u32, EngineError> {
        if self.state.edge_count() >= self.max_edges {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }
        use valori_kernel::types::id::NodeId;
        let kind = EdgeKind::from_u8(kind).unwrap_or_default();
        let edge_id = self.state.next_edge_id();
        let event = valori_kernel::event::KernelEvent::CreateEdge {
            id: edge_id,
            kind,
            from: NodeId(from),
            to: NodeId(to),
        };
        self.commit_and_apply_ns(&event, namespace_id)?;
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

    pub fn apply_committed_event(
        &mut self,
        event: &valori_kernel::event::KernelEvent,
    ) -> Result<(), EngineError> {
        self.apply_committed_event_ns(event, valori_kernel::types::id::DEFAULT_NS.0)
    }

    pub fn apply_committed_event_ns(
        &mut self,
        event: &valori_kernel::event::KernelEvent,
        namespace_id: u16,
    ) -> Result<(), EngineError> {
        // Enforce per-collection dimension BEFORE the kernel applies the
        // event. This is the one call every insert path funnels through —
        // standalone via `commit_and_apply_ns`, cluster via
        // `ValoriStateMachine::apply()` calling this same method on every
        // replica after Raft commit — so it is the single place that
        // guarantees identical accept/reject decisions cluster-wide without
        // touching every individual insert handler in `cluster_server.rs`.
        // Only namespaces with an EXPLICIT collection config are checked
        // here; unconfigured namespaces keep relying on
        // `KernelState::apply_event_ns`'s existing legacy `self.dim` check,
        // completely unchanged.
        self.validate_namespace_dim(event, namespace_id)?;
        // Delete/SoftDelete callers in this crate pass DEFAULT_NS.0
        // regardless of the record's real namespace (the kernel ignores the
        // parameter for those events — it derives the namespace from the
        // record itself). Resolve the record's TRUE namespace before it's
        // gone, so post-apply index routing deletes from the right
        // collection's index instead of silently defaulting to the legacy
        // global one.
        let resolved_ns = self.resolve_event_namespace(event, namespace_id);
        self.state.apply_event_ns(event, namespace_id)?;
        self.post_apply_derived(event, resolved_ns);
        Ok(())
    }

    fn validate_namespace_dim(
        &self,
        event: &valori_kernel::event::KernelEvent,
        namespace_id: u16,
    ) -> Result<(), EngineError> {
        use valori_kernel::event::KernelEvent;
        let vector_len = match event {
            KernelEvent::InsertRecord { vector, .. }
            | KernelEvent::AutoInsertRecord { vector, .. } => vector.len(),
            _ => return Ok(()),
        };
        if let Some(cfg) = self.namespaces.config(namespace_id) {
            if vector_len != cfg.dim as usize {
                return Err(EngineError::Kernel(KernelError::DimensionMismatch {
                    expected: cfg.dim as usize,
                    found: vector_len,
                }));
            }
        }
        Ok(())
    }

    fn resolve_event_namespace(
        &self,
        event: &valori_kernel::event::KernelEvent,
        fallback: u16,
    ) -> u16 {
        use valori_kernel::event::KernelEvent;
        match event {
            KernelEvent::DeleteRecord { id } | KernelEvent::SoftDeleteRecord { id } => self
                .state
                .get_record(*id)
                .map(|r| r.namespace_id)
                .unwrap_or(fallback),
            _ => fallback,
        }
    }

    fn post_apply_derived(&mut self, event: &valori_kernel::event::KernelEvent, namespace_id: u16) {
        use valori_kernel::event::KernelEvent;
        match event {
            KernelEvent::InsertRecord { id, vector, .. } => {
                let vals: Vec<f32> = vector
                    .data
                    .iter()
                    .map(|fxp| fxp.0 as f32 / SCALE as f32)
                    .collect();
                if let Some(idx) = self.collection_indexes.get_mut(&namespace_id) {
                    idx.insert(id.0, &vals);
                }
            }
            KernelEvent::DeleteRecord { id } | KernelEvent::SoftDeleteRecord { id } => {
                if let Some(idx) = self.collection_indexes.get_mut(&namespace_id) {
                    idx.delete(id.0);
                }
            }
            KernelEvent::ConfigureNamespace {
                namespace_id: ns,
                dim,
                index_kind,
                ..
            } => {
                // The kernel already recorded this in `self.state.namespace_configs`
                // (that's the replicated/audited source of truth — see the
                // event's doc comment); this reconciles Engine-side bookkeeping
                // (the dedicated index object, the CollectionRegistry mirror)
                // to match, on every replica that applies this event —
                // standalone, and every cluster follower via Raft.
                self.ensure_collection_index(*ns, *dim as usize, *index_kind);
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

    pub fn record_count(&self) -> usize {
        self.state.record_count()
    }

    pub fn apply_event_for_test(
        &mut self,
        evt: &valori_kernel::event::KernelEvent,
    ) -> Result<(), valori_kernel::error::KernelError> {
        self.state.apply_event(evt)
    }

    pub fn clone_kernel_state(&self) -> KernelState {
        self.state.clone()
    }

    pub fn kernel_state(&self) -> &KernelState {
        &self.state
    }

    pub fn node_count(&self) -> usize {
        self.state.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.state.edge_count()
    }

    pub fn kernel_dim(&self) -> Option<usize> {
        self.state.dim
    }

    pub fn get_node(
        &self,
        id: valori_kernel::types::id::NodeId,
    ) -> Option<&valori_kernel::graph::node::GraphNode> {
        self.state.get_node(id)
    }

    pub fn outgoing_edges(
        &self,
        id: valori_kernel::types::id::NodeId,
    ) -> Option<impl Iterator<Item = &valori_kernel::graph::edge::GraphEdge>> {
        self.state.outgoing_edges(id)
    }

    pub fn get_record(
        &self,
        id: valori_kernel::types::id::RecordId,
    ) -> Option<&valori_kernel::storage::record::Record> {
        self.state.get_record(id)
    }

    pub fn get_edge(
        &self,
        id: valori_kernel::types::id::EdgeId,
    ) -> Option<&valori_kernel::graph::edge::GraphEdge> {
        self.state.get_edge(id)
    }

    pub fn cosine_similarity(&self, id_a: u32, id_b: u32) -> Option<f32> {
        use valori_kernel::math::dot::dot_i32 as dot_product;
        use valori_kernel::types::id::RecordId;
        let rec_a = self.state.get_record(RecordId(id_a))?;
        let rec_b = self.state.get_record(RecordId(id_b))?;
        if !rec_a.is_searchable() || !rec_b.is_searchable() {
            return None;
        }
        let va: Vec<i32> = rec_a.vector.data.iter().map(|s| s.0).collect();
        let vb: Vec<i32> = rec_b.vector.data.iter().map(|s| s.0).collect();
        let dot = dot_product(&va, &vb) as f64;
        let mag_a = (dot_product(&va, &va) as f64).sqrt();
        let mag_b = (dot_product(&vb, &vb) as f64).sqrt();
        if mag_a == 0.0 || mag_b == 0.0 {
            return None;
        }
        Some((dot / (mag_a * mag_b)) as f32)
    }

    // ── Index management ──────────────────────────────────────────────────────

    /// Rebuild the legacy global index AND every explicitly-configured
    /// collection's dedicated index — each from only its own records.
    ///
    /// A record belonging to an explicitly-configured namespace is deliberately
    /// EXCLUDED from the legacy global index build below: it lives only in its
    /// own collection's dedicated index (built separately, in the loop after),
    /// never in both. Mixing the two would silently reintroduce the exact
    /// "one shared index, post-filtered by namespace" problem the dedicated
    /// per-collection indexes exist to fix — a record must always have an
    /// unambiguous collection/namespace relationship.
    /// Build or rebuild all per-collection indexes from the records in `self.state`.
    pub fn build_index(&mut self) {
        let namespaces: Vec<u16> = self.collection_indexes.keys().copied().collect();
        for ns in namespaces {
            let records: Vec<(u32, Vec<f32>)> = self
                .state
                .iter_records_in_ns(ns)
                .filter(|r| r.is_searchable())
                .map(|r| {
                    let vals: Vec<f32> = r
                        .vector
                        .data
                        .iter()
                        .map(|fxp| fxp.0 as f32 / SCALE as f32)
                        .collect();
                    (r.id.0, vals)
                })
                .collect();
            if let Some(idx) = self.collection_indexes.get_mut(&ns) {
                idx.build(&records);
            }
        }
    }

    pub fn rebuild_index(&mut self) {
        self.sync_collection_indexes_from_state();
        self.build_index();
    }

    pub fn effective_index_kind(&self) -> IndexKind {
        IndexKind::BruteForce
    }

    pub fn auto_tier_check(&mut self) {}

    // ── Phase 4: async index lifecycle ────────────────────────────────────────

    /// Returns the index lifecycle state for `namespace_id`, or a fresh
    /// default if the collection has no lifecycle record yet.
    pub fn index_state(&self, namespace_id: u16) -> crate::index_manager::CollectionIndexState {
        self.index_states
            .get(&namespace_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Start an asynchronous background index build for `namespace_id`.
    ///
    /// Returns the new generation number immediately. The actual build runs in
    /// a `spawn_blocking` task; when it completes it calls back through the
    /// provided callback with the built index object and the generation id.
    ///
    /// # Semantics
    /// - Only one build can be in progress per collection at a time.
    ///   Returns an error if a build is already running.
    /// - The active index (if any) continues serving while the new one builds.
    /// - `base_lsn` is the event-log height at the time the build starts — used
    ///   for WAL catch-up once the build completes.
    pub fn start_index_build(
        &mut self,
        namespace_id: u16,
        spec: crate::index_manager::IndexSpec,
    ) -> Result<u32, EngineError> {
        // Read the current height before taking mutable access to index_states.
        let base_lsn = self
            .persistence
            .event_committer()
            .map(|c| c.journal().committed_height())
            .unwrap_or(0);

        let state = self
            .index_states
            .entry(namespace_id)
            .or_insert_with(crate::index_manager::CollectionIndexState::new);

        if state.is_building() {
            return Err(EngineError::InvalidInput(
                "an index build is already in progress for this collection".into(),
            ));
        }

        let gen = state.start_build(spec.clone(), base_lsn);
        state.desired = Some(spec);
        Ok(gen)
    }

    /// Called by the background build task once the index is constructed.
    /// Catches up WAL entries that arrived after `base_lsn`, marks the
    /// generation READY, then atomically activates it.
    ///
    /// Returns the previous active generation (now RETIRING) if any.
    pub fn finish_index_build(
        &mut self,
        namespace_id: u16,
        generation: u32,
        mut new_idx: Box<dyn valori_index::VectorIndex + Send + Sync>,
    ) -> Result<Option<u32>, EngineError> {
        // Step 1: extract the base_lsn without holding a mutable borrow.
        let base_lsn = {
            let state = self
                .index_states
                .get(&namespace_id)
                .ok_or_else(|| EngineError::InvalidInput("no index build in progress".into()))?;
            match state.get_generation(generation) {
                Some(meta) => meta.base_lsn,
                None => return Err(EngineError::InvalidInput("unknown generation".into())),
            }
        };

        // Step 2: collect catch-up mutations from the WAL immutably before
        // mutating anything else.
        let catchup: Vec<(u16, valori_kernel::event::KernelEvent)> =
            if let Some(committer) = self.persistence.event_committer() {
                let journal = committer.journal();
                let current_height = journal.committed_height();
                if current_height > base_lsn {
                    journal
                        .committed_with_namespaces()
                        .skip(base_lsn as usize)
                        .map(|(ev, ns)| (ns, ev.clone()))
                        .collect()
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

        // Step 3: apply catch-up mutations to the new index.
        for (ns, event) in &catchup {
            if *ns != namespace_id {
                continue;
            }
            use valori_kernel::event::KernelEvent;
            match event {
                KernelEvent::InsertRecord { id, vector, .. } => {
                    let vals: Vec<f32> = vector
                        .data
                        .iter()
                        .map(|fxp| fxp.0 as f32 / SCALE as f32)
                        .collect();
                    new_idx.insert(id.0, &vals);
                }
                KernelEvent::DeleteRecord { id } | KernelEvent::SoftDeleteRecord { id } => {
                    new_idx.delete(id.0);
                }
                _ => {}
            }
        }

        // Step 4: atomically swap the index object.
        self.collection_indexes.insert(namespace_id, new_idx);

        // Step 5: advance lifecycle state BUILDING → READY → ACTIVE.
        let state = self
            .index_states
            .entry(namespace_id)
            .or_insert_with(crate::index_manager::CollectionIndexState::new);
        state.mark_ready(generation);
        let retired = state.activate(generation);

        // Step 6 (Phase 4.1): persist the artifact and update the manifest.
        // We do this AFTER activation so the collection is serving immediately;
        // if the write fails we log and continue — the artifact is optional
        // (worst case: restart falls back to rebuild).
        let index_type = state
            .generations
            .iter()
            .find(|g| g.0 == generation)
            .map(|g| g.2.spec.index_type.clone());
        if let Some(ref itype) = index_type {
            if itype != "bq" {
                // Write the artifact bytes (immutable, crash-safe).
                if let (Some(provider), Some(project_id)) =
                    (&self.storage_provider, self.project_id)
                {
                    let ns = valori_kernel::types::id::NamespaceId(namespace_id);
                    // Re-borrow the live index to snapshot its bytes.
                    let artifact_bytes = self
                        .collection_indexes
                        .get(&namespace_id)
                        .and_then(|idx| idx.snapshot().ok());

                    if let Some(bytes) = artifact_bytes {
                        let artifact_key = valori_storage::provider::StorageKey::IndexArtifact {
                            project_id,
                            collection_id: ns,
                            index_type: itype.clone(),
                            generation,
                        };
                        match provider.put_immutable(&artifact_key, &bytes) {
                            Ok(_) => {
                                // Now durably record which generation is active.
                                let current_lsn = self.current_lsn().0;
                                Self::do_write_manifest_index_fields(
                                    provider,
                                    project_id,
                                    ns,
                                    Some(generation),
                                    Some(itype.as_str()),
                                    current_lsn,
                                );
                                // GC: delete retired generation's artifact.
                                if let Some(retiring_gen) = retired {
                                    Self::do_delete_index_artifact(
                                        provider,
                                        project_id,
                                        ns,
                                        itype,
                                        retiring_gen,
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "index artifact write failed for ns={namespace_id} gen={generation}: {e}"
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(retired)
    }

    /// Mark an in-progress build as FAILED (e.g. due to a build error).
    /// The active index (if any) is unaffected.
    pub fn fail_index_build(&mut self, namespace_id: u16, generation: u32, reason: String) {
        let state = self
            .index_states
            .entry(namespace_id)
            .or_insert_with(crate::index_manager::CollectionIndexState::new);
        state.mark_failed(generation, reason);
    }

    /// Drop the active index for `namespace_id`, reverting to exact brute-force search.
    /// If a StorageProvider is configured, clears the index fields in the collection
    /// manifest and deletes the artifact bytes.
    pub fn drop_collection_index(&mut self, namespace_id: u16) {
        // Grab the active generation before mutating state (used for artifact cleanup).
        let retiring_gen_and_type = self.index_states.get(&namespace_id).and_then(|s| {
            let gen = s.active_generation?;
            let idx_type = s
                .generations
                .iter()
                .find(|g| g.0 == gen)
                .map(|g| g.2.spec.index_type.clone())?;
            Some((gen, idx_type))
        });

        self.collection_indexes.remove(&namespace_id);
        let state = self
            .index_states
            .entry(namespace_id)
            .or_insert_with(crate::index_manager::CollectionIndexState::new);
        state.set_none();

        // Best-effort: clear manifest index fields and delete artifact.
        if let Some((retiring_gen, retiring_type)) = retiring_gen_and_type {
            if let (Some(provider), Some(project_id)) = (&self.storage_provider, self.project_id) {
                let ns = valori_kernel::types::id::NamespaceId(namespace_id);
                Self::do_clear_manifest_index_fields(provider, project_id, ns);
                Self::do_delete_index_artifact(
                    provider,
                    project_id,
                    ns,
                    &retiring_type,
                    retiring_gen,
                );
            }
        }
    }

    // ── Index artifact helpers (Phase 4.1) ────────────────────────────────────

    /// Read the `CollectionManifest` for `ns_id` from the configured provider.
    /// Returns `None` if no provider is configured, manifest is missing, or it
    /// fails to decode.
    fn read_collection_manifest(
        &self,
        ns_id: valori_kernel::types::id::NamespaceId,
    ) -> Option<valori_storage::collection_manifest::CollectionManifest> {
        let provider = self.storage_provider.as_ref()?;
        let project_id = self.project_id?;
        let key = valori_storage::provider::StorageKey::CollectionManifest {
            project_id,
            collection_id: ns_id,
        };
        let bytes = provider.get(&key).ok()?;
        valori_storage::collection_manifest::CollectionManifest::decode(&key, &bytes).ok()
    }

    /// Write the active-index fields into the collection manifest for `ns_id`.
    /// Reads first (preserving all other fields), then writes back atomically.
    /// Silent on error — caller logs it.
    fn write_manifest_index_fields(
        &self,
        ns_id: valori_kernel::types::id::NamespaceId,
        active_generation: Option<u32>,
        active_index_type: Option<&str>,
        active_index_base_lsn: u64,
    ) {
        let provider = match &self.storage_provider {
            Some(p) => p,
            None => return,
        };
        let project_id = match self.project_id {
            Some(p) => p,
            None => return,
        };
        Self::do_write_manifest_index_fields(
            provider,
            project_id,
            ns_id,
            active_generation,
            active_index_type,
            active_index_base_lsn,
        );
    }

    fn do_write_manifest_index_fields(
        provider: &Arc<dyn StorageProvider>,
        project_id: valori_domain::ProjectId,
        ns_id: valori_kernel::types::id::NamespaceId,
        active_generation: Option<u32>,
        active_index_type: Option<&str>,
        active_index_base_lsn: u64,
    ) {
        use valori_storage::collection_manifest::Lsn;
        use valori_storage::provider::StorageKey;

        let key = StorageKey::CollectionManifest {
            project_id,
            collection_id: ns_id,
        };
        let bytes = match provider.get(&key) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(
                    "index manifest update: cannot read manifest for ns {}: {e}",
                    ns_id.0
                );
                return;
            }
        };
        let mut manifest =
            match valori_storage::collection_manifest::CollectionManifest::decode(&key, &bytes) {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!(
                        "index manifest update: cannot decode manifest for ns {}: {e}",
                        ns_id.0
                    );
                    return;
                }
            };
        manifest.active_index_generation = active_generation;
        manifest.active_index_type = active_index_type.map(ToOwned::to_owned);
        manifest.active_index_base_lsn = Lsn(active_index_base_lsn);

        if let Err(e) = provider.put_manifest(&key, &manifest.encode()) {
            tracing::error!(
                "index manifest update: cannot write manifest for ns {}: {e}",
                ns_id.0
            );
        }
    }

    fn do_clear_manifest_index_fields(
        provider: &Arc<dyn StorageProvider>,
        project_id: valori_domain::ProjectId,
        ns_id: valori_kernel::types::id::NamespaceId,
    ) {
        Self::do_write_manifest_index_fields(provider, project_id, ns_id, None, None, 0);
    }

    fn do_delete_index_artifact(
        provider: &Arc<dyn StorageProvider>,
        project_id: valori_domain::ProjectId,
        ns_id: valori_kernel::types::id::NamespaceId,
        index_type: &str,
        generation: u32,
    ) {
        use valori_storage::provider::StorageKey;
        let key = StorageKey::IndexArtifact {
            project_id,
            collection_id: ns_id,
            index_type: index_type.to_owned(),
            generation,
        };
        if let Err(e) = provider.delete(&key) {
            tracing::warn!(
                "could not delete retired index artifact ns={} type={index_type} gen={generation}: {e}",
                ns_id.0
            );
        }
    }

    /// Attempt to load persisted index artifacts from the configured StorageProvider
    /// and install them into `self.collection_indexes` + `self.index_states`.
    ///
    /// Called during `try_recover` (StorageProvider path) INSTEAD of the old
    /// `rebuild_index() + sync_collection_indexes_from_state()` sequence.
    /// After this returns, `collection_indexes` is ready to serve.
    ///
    /// # Per-collection strategy
    ///
    /// - Manifest has `active_index_generation` + `base_lsn == recovered_lsn`:
    ///   load artifact bytes → restore → install (fast path, no rebuild).
    /// - Manifest has `active_index_generation` + `base_lsn < recovered_lsn`:
    ///   the artifact is stale (records were inserted after the last build).
    ///   Rebuild from `self.state` records (always correct; avoids WAL segment
    ///   re-reading which is complex post-recovery).
    /// - Manifest has no `active_index_generation`, or artifact load fails:
    ///   call `ensure_collection_index` (existing sync-rebuild path, also used
    ///   for BQ which cannot be artifact-persisted).
    ///
    /// BQ collections (`active_index_type == "bq"`) always fall through to
    /// `ensure_collection_index` — `BqIndex::snapshot()` returns empty bytes
    /// and `restore()` is a no-op, so there is no meaningful artifact.
    pub fn try_restore_index_artifacts(&mut self, recovered_lsn: u64) {
        // Snapshot the namespace configs so we don't hold a borrow while
        // mutating `collection_indexes`.
        //
        // NOTE: after `recover_project_from_storage`, `cfg.index_kind` is
        // always 0 (BruteForce) because that path calls `configure_namespace`
        // with `index_kind=0` from the manifest's dim/metric.  We therefore
        // drive the index-kind decision from the MANIFEST (specifically
        // `active_index_type` / `desired_index`), not from the kernel state.
        let entries: Vec<(u16, u32)> = self
            .state
            .namespace_configs
            .iter()
            .map(|(&ns, cfg)| (ns, cfg.dim))
            .collect();

        for (ns_id, dim) in entries {
            let ns = valori_kernel::types::id::NamespaceId(ns_id);

            // Sync the namespace config mirror (same as ensure_collection_index does).
            if !self.namespaces.configs.contains_key(&ns_id) {
                self.namespaces.configs.insert(
                    ns_id,
                    valori_metadata::collection::CollectionVectorConfig {
                        dim,
                        metric: valori_domain::Metric::SquaredL2,
                    },
                );
            }

            // Read the manifest to know the desired index type.
            let manifest = self.read_collection_manifest(ns);

            // Determine the effective kind from the manifest's desired_index or
            // active_index_type.  Fall back to BruteForce (exact search) if
            // neither is present.
            let effective_kind = manifest
                .as_ref()
                .and_then(|m| {
                    // Prefer the active artifact type over the desired-index label.
                    m.active_index_type
                        .as_deref()
                        .map(|t| match t {
                            "hnsw" => valori_domain::IndexKind::Hnsw,
                            "ivf" => valori_domain::IndexKind::Ivf,
                            "bq" => valori_domain::IndexKind::Bq,
                            _ => valori_domain::IndexKind::Brute,
                        })
                        .or_else(|| m.desired_index)
                })
                .unwrap_or(valori_domain::IndexKind::Brute);

            self.namespaces.set_desired_index(ns_id, effective_kind);

            // BruteForce needs no dedicated index object (namespace-scoped
            // exact scan in KernelState is already correct and isolated).
            if effective_kind == valori_domain::IndexKind::Brute {
                self.collection_indexes.remove(&ns_id);
                continue;
            }

            // Engine IndexKind for fallback rebuild.
            let engine_kind = match effective_kind {
                valori_domain::IndexKind::Hnsw => IndexKind::Hnsw,
                valori_domain::IndexKind::Ivf => IndexKind::Ivf,
                valori_domain::IndexKind::Bq => IndexKind::Bq,
                valori_domain::IndexKind::Auto => IndexKind::Auto,
                _ => IndexKind::BruteForce,
            };
            let index_kind_wire = engine_kind as u8;

            // Try to load artifact from the manifest.
            let loaded =
                self.try_load_index_artifact_for(ns_id, dim as usize, engine_kind, recovered_lsn);

            if !loaded {
                // Fallback: synchronous rebuild from current KernelState records.
                self.ensure_collection_index(ns_id, dim as usize, index_kind_wire);
            }
        }
    }

    /// Internal: try to load the artifact for one collection.
    /// Returns `true` if the artifact was loaded and installed.
    fn try_load_index_artifact_for(
        &mut self,
        ns_id: u16,
        dim: usize,
        kind: IndexKind,
        recovered_lsn: u64,
    ) -> bool {
        use valori_index::{HnswIndex, IvfIndex, VectorIndex};
        use valori_storage::provider::StorageKey;

        let ns = valori_kernel::types::id::NamespaceId(ns_id);

        // Read the manifest — without a provider there's nothing to load.
        let manifest = match self.read_collection_manifest(ns) {
            Some(m) => m,
            None => return false,
        };

        let (gen, index_type) = match (
            &manifest.active_index_generation,
            &manifest.active_index_type,
        ) {
            (Some(g), Some(t)) => (*g, t.clone()),
            _ => return false,
        };

        // BQ: cannot be artifact-persisted (snapshot returns empty bytes).
        if index_type == "bq" {
            return false;
        }

        let base_lsn = manifest.active_index_base_lsn.0;

        // If the artifact is stale, rebuild from records instead of loading.
        if base_lsn < recovered_lsn {
            tracing::info!(
                "ns {} index artifact gen={gen} base_lsn={base_lsn} < current={recovered_lsn}; \
                 rebuilding from kernel state",
                ns_id
            );
            return false;
        }

        // Load the artifact bytes.
        let project_id = match self.project_id {
            Some(p) => p,
            None => return false,
        };
        let provider = match self.storage_provider.clone() {
            Some(p) => p,
            None => return false,
        };
        let artifact_key = StorageKey::IndexArtifact {
            project_id,
            collection_id: ns,
            index_type: index_type.clone(),
            generation: gen,
        };
        let bytes = match provider.get(&artifact_key) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    "ns {} index artifact gen={gen} not found or corrupt ({e}); \
                     falling back to rebuild",
                    ns_id
                );
                return false;
            }
        };

        // Restore the right index type.
        let mut idx: Box<dyn valori_index::VectorIndex + Send + Sync> = match index_type.as_str() {
            "hnsw" => Box::new(HnswIndex::new_with_config(self.hnsw_config.clone())),
            "ivf" => Box::new(IvfIndex::new(self.ivf_config.clone(), dim)),
            _ => {
                tracing::warn!(
                    "ns {} unknown index type '{}'; falling back to rebuild",
                    ns_id,
                    index_type
                );
                return false;
            }
        };

        if let Err(e) = idx.restore(&bytes) {
            tracing::warn!(
                "ns {} index artifact gen={gen} restore failed ({e}); rebuilding",
                ns_id
            );
            return false;
        }

        // Install the restored index.
        self.collection_indexes.insert(ns_id, idx);

        // Reconstruct in-memory lifecycle state.
        let state = self
            .index_states
            .entry(ns_id)
            .or_insert_with(crate::index_manager::CollectionIndexState::new);
        if state.active_generation.is_none() && state.building_generation.is_none() {
            let spec = crate::index_manager::IndexSpec {
                index_type: index_type.clone(),
                parameters: serde_json::json!({}),
            };
            let allocated_gen = state.start_build(spec.clone(), base_lsn);
            state.mark_ready(allocated_gen);
            state.activate(allocated_gen);
            state.desired = Some(spec);
        }

        tracing::info!(
            "ns {} restored {index_type} index gen={gen} from artifact (base_lsn={base_lsn})",
            ns_id
        );
        true
    }

    /// Extract the current record set for `namespace_id` as `(id, f32_vector)` pairs.
    /// Used by the background build task to get a consistent snapshot of records.
    pub fn snapshot_records_for_ns(&self, namespace_id: u16) -> Vec<(u32, Vec<f32>)> {
        self.state
            .iter_records_in_ns(namespace_id)
            .filter(|r| r.is_searchable())
            .map(|r| {
                let vals: Vec<f32> = r
                    .vector
                    .data
                    .iter()
                    .map(|fxp| fxp.0 as f32 / SCALE as f32)
                    .collect();
                (r.id.0, vals)
            })
            .collect()
    }

    // ── Crash recovery ────────────────────────────────────────────────────────

    /// Recover durable state at startup.
    ///
    /// # Priority order (Phase 2.1)
    ///
    /// 1. **StorageProvider-backed recovery** (`recover_project_with_wal_tail`) —
    ///    attempted FIRST, but only when `self.storage_provider`/`self.project_id`
    ///    are both configured (see `configure_storage_provider`) AND at
    ///    least one collection manifest is discovered. On success, this
    ///    entirely replaces the legacy paths below for this call.
    /// 2. Event log (legacy, whole-process) — canonical when no storage
    ///    provider is configured, or the provider path found nothing.
    /// 3. Snapshot (legacy, whole-process) fast-path cache.
    /// 4. WAL (legacy fallback).
    /// 5. Fresh.
    ///
    /// This is the explicit, bounded compatibility path the phase spec
    /// allows: an unconfigured `Engine` (every existing test, and every
    /// live node until `valori-node`'s startup is updated to construct and
    /// inject a provider) behaves byte-for-byte as before this phase.
    pub fn try_recover(&mut self) -> RecoveryMode {
        if let (Some(provider), Some(project_id)) = (self.storage_provider.clone(), self.project_id)
        {
            match valori_state::collection_bootstrap::discover_collections(
                provider.as_ref(),
                project_id,
            ) {
                Ok(manifests) if !manifests.is_empty() => {
                    let wal_path = self
                        .event_committer()
                        .map(|c| c.event_log().path().to_path_buf())
                        .or_else(|| self.wal_path.clone());
                    match valori_state::collection_bootstrap::recover_project_from_storage(
                        provider.as_ref(),
                        project_id,
                        valori_core::ShardId(0),
                        wal_path.as_deref(),
                    ) {
                        Ok((recovered_state, highest_lsn)) => {
                            tracing::info!(
                                "StorageProvider recovery: {} collections, highest LSN {}",
                                manifests.len(),
                                highest_lsn
                            );
                            self.state = recovered_state;
                            // Phase 4.1: try to restore index artifacts from
                            // the provider instead of blindly rebuilding every
                            // index from scratch.  Falls back to rebuild for
                            // collections whose artifact is missing or stale.
                            self.try_restore_index_artifacts(highest_lsn.0);
                            self.auto_tier_check();
                            return RecoveryMode::StorageProvider(highest_lsn.0);
                        }
                        Err(e) => {
                            tracing::error!(
                                "StorageProvider recovery failed ({e}); falling back to the \
                                 legacy whole-process path"
                            );
                        }
                    }
                }
                Ok(_) => {
                    tracing::info!(
                        "StorageProvider configured but no collection manifests discovered; \
                         falling back to the legacy whole-process path"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        "Collection discovery failed ({e}); falling back to the legacy \
                         whole-process path"
                    );
                }
            }
        }

        let log_info = self
            .event_committer()
            .map(|c| (c.event_log().path().to_path_buf(), c.event_log().dim()));

        if let Some((log_path, dim)) = log_info {
            if log_path.exists() {
                match valori_state::bootstrap::recover_from_events(&log_path) {
                    Ok((recovered_state, recovered_journal, count)) => {
                        if count == 0 {
                            tracing::info!("Event log exists but is empty; trying snapshot");
                        } else {
                            tracing::info!(
                                "Event-log recovery: replaying {} events from {:?}",
                                count,
                                log_path
                            );
                            self.persistence = Persistence::Ephemeral;
                            match EventLogWriter::open(&log_path, Some(dim)) {
                                Ok(log_writer) => {
                                    let state_for_committer = recovered_state.clone();
                                    self.state = recovered_state;
                                    self.persistence = Persistence::EventLog(EventCommitter::new(
                                        log_writer,
                                        recovered_journal,
                                        state_for_committer,
                                    ));
                                    self.rebuild_index();
                                    self.auto_tier_check();
                                    self.load_metadata().ok();
                                    self.sync_metadata_from_state();
                                    self.load_namespaces().ok();
                                    return RecoveryMode::EventLog(count);
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to reopen event log after recovery: {}",
                                        e
                                    );
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
                        Err(e) => {
                            tracing::error!("Snapshot restore failed ({:?}); starting fresh", e)
                        }
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
                            tracing::info!(
                                "WAL recovery: replayed {} commands from {:?}",
                                count,
                                wal_path
                            );
                            self.rebuild_index();
                            self.auto_tier_check();
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
        _i_data: Option<&[u8]>,
        ns_registry: Option<CollectionRegistry>,
    ) -> Result<(), EngineError> {
        self.state = decode_state(k_data)?;
        if !m_data.is_empty() {
            self.metadata.restore(m_data);
        }
        if let Some(reg) = ns_registry {
            self.namespaces = reg;
        }
        self.sync_collection_indexes_from_state();
        self.build_index();
        Ok(())
    }

    fn restore_trailing_sections(&mut self, data: &[u8], mut offset: usize) {
        while offset + 8 <= data.len() {
            let tag = &data[offset..offset + 4];
            let section_len =
                u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap_or([0; 4]))
                    as usize;
            offset += 8;
            if offset + section_len > data.len() {
                break;
            }
            let section = &data[offset..offset + section_len];
            offset += section_len;

            if tag == b"CRTS" {
                if let Ok((map, _)) = bincode::serde::decode_from_slice::<HashMap<u32, u64>, _>(
                    section,
                    bincode::config::standard(),
                ) {
                    self.created_at = map;
                }
            } else if tag == b"BCRP" {
                use std::collections::HashMap as StdMap;
                if let Ok(((corpus, total_tokens), _)) =
                    bincode::serde::decode_from_slice::<(StdMap<u64, Vec<String>>, usize), _>(
                        section,
                        bincode::config::standard(),
                    )
                {
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
        return Err(EngineError::InvalidInput(format!(
            "Truncated snapshot: missing {field}"
        )));
    }
    let val = u32::from_le_bytes(
        data[*offset..*offset + 4]
            .try_into()
            .map_err(|_| EngineError::InvalidInput(format!("Failed to read {field}")))?,
    );
    *offset += 4;
    Ok(val)
}

fn slice_at<'a>(
    data: &'a [u8],
    offset: &mut usize,
    len: usize,
    field: &'static str,
) -> Result<&'a [u8], EngineError> {
    if *offset + len > data.len() {
        return Err(EngineError::InvalidInput(format!(
            "Truncated snapshot: {field} out of bounds"
        )));
    }
    let s = &data[*offset..*offset + len];
    *offset += len;
    Ok(s)
}

fn pct(used: usize, capacity: usize) -> f64 {
    if capacity == 0 {
        0.0
    } else {
        used as f64 / capacity as f64 * 100.0
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EngineConfig, IndexKind, QuantizationKind};
    use valori_kernel::crypto::{CryptoError, KeyVault};

    struct NoopVault;
    impl KeyVault for NoopVault {
        fn encrypt(&self, _key_id: [u8; 16], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
            Ok(plaintext.to_vec())
        }
        fn decrypt(&self, _key_id: [u8; 16], ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
            Ok(ciphertext.to_vec())
        }
        fn shred(&self, _key_id: [u8; 16]) -> Result<(), CryptoError> {
            Ok(())
        }
        fn key_exists(&self, _key_id: &[u8; 16]) -> bool {
            true
        }
    }

    fn tiny_cfg() -> EngineConfig {
        EngineConfig {
            max_records: 100,
            max_nodes: 32,
            max_edges: 64,
            quantization_kind: QuantizationKind::None,
            hnsw_m: None,
            hnsw_ef_construction: None,
            hnsw_ef_search: None,
            ivf_n_list: None,
            ivf_n_probe: None,
            bq_pool_factor: None,
            bq_min_candidates: None,
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
        let id = e.insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0]).unwrap();
        e.soft_delete_record(id).unwrap();
        let results = e.search_l2(&[1.0, 0.0, 0.0, 0.0], 1).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn snapshot_roundtrip() {
        let mut e = Engine::with_config(tiny_cfg());
        e.insert_record_from_f32(&[0.5, 0.5, 0.5, 0.5]).unwrap();
        let snap = e.snapshot().unwrap();

        let mut e2 = Engine::with_config(tiny_cfg());
        e2.restore(&snap).unwrap();
        assert_eq!(e2.record_count(), 1);
    }

    #[test]
    fn streaming_encoder_matches_buffered() {
        // write_snapshot_to_writer must produce bit-for-bit identical bytes to
        // snapshot() so callers can switch without a format change.
        let mut e = Engine::with_config(tiny_cfg());
        e.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
        e.insert_record_from_f32(&[0.5, 0.6, 0.7, 0.8]).unwrap();

        let buffered = e.snapshot().unwrap();

        let mut cursor = std::io::Cursor::new(Vec::with_capacity(buffered.len()));
        e.write_snapshot_to_writer(&mut cursor).unwrap();

        assert_eq!(
            buffered,
            cursor.into_inner(),
            "write_snapshot_to_writer must match snapshot()"
        );
    }

    #[test]
    fn streaming_save_restore_roundtrip() {
        // save_snapshot writes directly to a file; restore must reconstruct
        // identical state without an intermediate in-memory Vec<u8>.
        let mut e = Engine::with_config(tiny_cfg());
        e.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
        e.insert_record_from_f32(&[0.5, 0.6, 0.7, 0.8]).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let snap_path = dir.path().join("snap.val");
        e.save_snapshot(Some(snap_path.as_path())).unwrap();

        let data = std::fs::read(&snap_path).unwrap();
        let mut e2 = Engine::with_config(tiny_cfg());
        e2.restore(&data).unwrap();
        assert_eq!(e2.record_count(), 2);

        let results = e2.search_l2(&[0.1, 0.2, 0.3, 0.4], 1).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn collection_create_and_drop() {
        let mut e = Engine::with_config(tiny_cfg());
        e.create_collection_with_config(
            "test",
            4,
            valori_domain::Metric::SquaredL2,
            valori_domain::IndexKind::Brute,
        )
        .unwrap();
        assert!(e.list_collections().iter().any(|(n, _)| n == "test"));
        e.drop_collection("test").unwrap();
        assert!(!e.list_collections().iter().any(|(n, _)| n == "test"));
    }

    /// Phase 3.3 §5/§7 — the exact gap the Phase 3.2 audit found: there must
    /// be no `Engine` method that creates a modern collection without
    /// requiring explicit vector config. This is a compile-time proof, not
    /// a runtime assertion — `create_collection(name)` (the old no-config
    /// signature) does not exist on `Engine` at all any more, so calling it
    /// is a compile error, not a value this test could even construct and
    /// check. `create_collection_with_config` is the only entry point, and
    /// its signature makes `dim`/`metric` mandatory arguments — there is no
    /// way to omit them and still call the function.
    #[test]
    fn direct_unconfigured_collection_creation_is_not_possible() {
        let mut e = Engine::with_config(tiny_cfg());
        // The only way to create "docs" is with explicit config — there is
        // no shorter overload. If a config-free path existed, this call
        // would take fewer arguments and compile without them.
        let id = e
            .create_collection_with_config(
                "docs",
                4,
                valori_domain::Metric::SquaredL2,
                valori_domain::IndexKind::Brute,
            )
            .unwrap();
        assert_eq!(
            e.namespaces.config(id),
            Some(valori_metadata::collection::CollectionVectorConfig {
                dim: 4,
                metric: valori_domain::Metric::SquaredL2,
            }),
            "a collection created through the Engine API always has a NamespaceConfig \
             the moment it exists — there is no intermediate unconfigured state to observe"
        );
    }

    /// A brand-new `Engine` — nothing explicitly created — has zero
    /// collections. Phase 3.3 §1/§4: this must hold with no vector config
    /// of any kind supplied at construction.
    #[test]
    fn new_engine_has_zero_collections() {
        let e = Engine::with_config(tiny_cfg());
        assert!(e.list_collections().is_empty());
        assert!(e.namespaces.is_empty());
    }

    // ── Collection-scoped vector configuration ──────────────────────────────

    fn brute(name: &str) -> valori_domain::IndexKind {
        let _ = name;
        valori_domain::IndexKind::Brute
    }

    /// The core requirement (§19 of the implementation task): two
    /// collections in ONE engine/process, different dimensions, each
    /// accepting only its own dimension.
    #[test]
    fn two_collections_different_dimensions_are_independently_enforced() {
        let mut e = Engine::with_config(tiny_cfg()); // process-wide dim = 4, unused by these
        let docs = e
            .create_collection_with_config(
                "documents",
                3,
                valori_domain::Metric::SquaredL2,
                brute("documents"),
            )
            .unwrap();
        let images = e
            .create_collection_with_config(
                "images",
                5,
                valori_domain::Metric::SquaredL2,
                brute("images"),
            )
            .unwrap();

        // Correct dimension for each collection succeeds.
        e.insert_record_from_f32_ns(&[1.0, 0.0, 0.0], docs).unwrap();
        e.insert_record_from_f32_ns(&[1.0, 0.0, 0.0, 0.0, 0.0], images)
            .unwrap();

        // Wrong dimension for each collection is rejected — the exact
        // scenario the task calls "essential": insert 5-dim into the
        // 3-dim collection, and 3-dim into the 5-dim collection.
        let err = e
            .insert_record_from_f32_ns(&[1.0, 0.0, 0.0, 0.0, 0.0], docs)
            .unwrap_err();
        assert!(matches!(
            err,
            EngineError::Kernel(KernelError::DimensionMismatch {
                expected: 3,
                found: 5
            })
        ));

        let err = e
            .insert_record_from_f32_ns(&[1.0, 0.0, 0.0], images)
            .unwrap_err();
        assert!(matches!(
            err,
            EngineError::Kernel(KernelError::DimensionMismatch {
                expected: 5,
                found: 3
            })
        ));
    }

    /// §18/§20: search(A) must never return B's records, and must never be
    /// validated against B's dimension, regardless of index kind.
    #[test]
    fn cross_collection_isolation_brute_force() {
        let mut e = Engine::with_config(tiny_cfg());
        let a = e
            .create_collection_with_config("a", 3, valori_domain::Metric::SquaredL2, brute("a"))
            .unwrap();
        let b = e
            .create_collection_with_config("b", 3, valori_domain::Metric::SquaredL2, brute("b"))
            .unwrap();

        let id_a = e.insert_record_from_f32_ns(&[1.0, 0.0, 0.0], a).unwrap();
        let id_b = e.insert_record_from_f32_ns(&[1.0, 0.0, 0.0], b).unwrap();
        assert_ne!(id_a, id_b);

        let hits_a = e.search_l2_ns(&[1.0, 0.0, 0.0], 10, a).unwrap();
        let hits_b = e.search_l2_ns(&[1.0, 0.0, 0.0], 10, b).unwrap();
        assert_eq!(
            hits_a.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![id_a]
        );
        assert_eq!(
            hits_b.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![id_b]
        );

        // delete(A) never modifies B.
        e.delete_record(id_a).unwrap();
        let hits_b_after = e.search_l2_ns(&[1.0, 0.0, 0.0], 10, b).unwrap();
        assert_eq!(
            hits_b_after.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![id_b]
        );
        assert!(e.search_l2_ns(&[1.0, 0.0, 0.0], 10, a).unwrap().is_empty());
    }

    /// Mixed index kinds in one engine: A=BruteForce, B=Ivf — the exact
    /// combination from the task's worked example (§25), reduced to what
    /// actually constructs and searches in this runtime today.
    #[test]
    fn mixed_index_kinds_brute_and_ivf_coexist() {
        let mut e = Engine::with_config(tiny_cfg());
        let a = e
            .create_collection_with_config(
                "a",
                3,
                valori_domain::Metric::SquaredL2,
                valori_domain::IndexKind::Brute,
            )
            .unwrap();
        let b = e
            .create_collection_with_config(
                "b",
                3,
                valori_domain::Metric::SquaredL2,
                valori_domain::IndexKind::Ivf,
            )
            .unwrap();

        // B got a real dedicated Ivf index object; A did not (BruteForce
        // collections deliberately reuse the kernel's per-namespace scan —
        // see `ensure_collection_index`'s doc comment).
        assert!(!e.collection_indexes.contains_key(&a));
        assert!(e.collection_indexes.contains_key(&b));

        for i in 0..5u32 {
            let v = [i as f32, 0.0, 0.0];
            e.insert_record_from_f32_ns(&v, a).unwrap();
            e.insert_record_from_f32_ns(&v, b).unwrap();
        }

        let hits_a = e.search_l2_ns(&[0.0, 0.0, 0.0], 5, a).unwrap();
        let hits_b = e.search_l2_ns(&[0.0, 0.0, 0.0], 5, b).unwrap();
        assert_eq!(hits_a.len(), 5);
        assert_eq!(hits_b.len(), 5);
    }

    /// Namespace 0's legacy, config-free fallback (`KernelState.dim`) still
    /// works when reached directly through the legacy, non-namespaced
    /// `insert_record_from_f32`/`search_l2` methods — no `Engine`-level
    /// "create collection" call of any kind. Phase 3.3: this is compat for
    /// old data/replay only, never an active new-Collection creation path
    /// — proven here by NOT calling `create_collection_with_config` (that
    /// method requires explicit config; nothing does for namespace 0 in
    /// this test) and confirming the legacy path still tolerates it.
    #[test]
    fn unconfigured_namespace_zero_keeps_legacy_single_dim_behavior() {
        let mut e = Engine::with_config(tiny_cfg()); // dim = 4
        let id = e.insert_record_from_f32(&[1.0, 0.0, 0.0, 0.0]).unwrap();
        let hits = e.search_l2(&[1.0, 0.0, 0.0, 0.0], 1).unwrap();
        assert_eq!(hits[0].0, id);
        assert!(e.collection_indexes.is_empty());
        assert!(e.namespaces.config(0).is_none());
        assert!(
            e.namespaces.list().is_empty(),
            "no Collection was ever explicitly created"
        );
    }

    /// Configuring the same collection twice with a DIFFERENT dimension is
    /// rejected — dimension is immutable after creation (§3 of the task).
    #[test]
    fn reconfiguring_a_collection_with_a_different_dim_is_rejected() {
        let mut e = Engine::with_config(tiny_cfg());
        e.create_collection_with_config("x", 3, valori_domain::Metric::SquaredL2, brute("x"))
            .unwrap();
        // Re-"creating" the SAME name with the SAME dim is idempotent
        // (`CollectionRegistry::create` returns the existing id) — that is
        // correct, not a bug: it's what makes a retried request safe.
        e.create_collection_with_config("x", 3, valori_domain::Metric::SquaredL2, brute("x"))
            .unwrap();

        // The actual immutability guarantee is enforced one layer down, in
        // `KernelState::configure_namespace` — exercised directly here since
        // `CollectionRegistry::create`'s name-idempotency would mask it above
        // (a second `create_collection_with_config("x", 5, ...)` call would
        // reuse "x"'s existing namespace id and correctly fail, but for the
        // "name already taken" reason, not a distinguishable dim-conflict
        // reason at this layer).
        let mut state = valori_kernel::state::kernel::KernelState::new();
        state
            .configure_namespace(9, 3, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();
        assert!(state
            .configure_namespace(9, 5, valori_kernel::index::Metric::SquaredL2, 0)
            .is_err());
    }

    /// Snapshot round-trip preserves collection configuration (the
    /// `namespace_configs` map itself, and reconstructs the dedicated index)
    /// when every namespace's vectors share one byte length in the snapshot
    /// — the case this phase's V8 extension actually covers safely. See
    /// `snapshot_roundtrip_known_limitation_mixed_dimensions` below for the
    /// disclosed gap this does NOT yet cover.
    #[test]
    fn snapshot_roundtrip_preserves_collection_config_same_dim_as_legacy() {
        let mut e = Engine::with_config(tiny_cfg()); // process dim = 4
        let images = e
            .create_collection_with_config(
                "images",
                4,
                valori_domain::Metric::SquaredL2,
                valori_domain::IndexKind::Ivf,
            )
            .unwrap();
        e.insert_record_from_f32_ns(&[1.0, 2.0, 3.0, 4.0], images)
            .unwrap();

        let snap = e.snapshot().unwrap();
        let mut e2 = Engine::with_config(tiny_cfg());
        e2.restore(&snap).unwrap();

        assert_eq!(e2.namespace_effective_dim(images), Some(4));
        assert!(
            e2.collection_indexes.contains_key(&images),
            "restore must reconstruct the dedicated Ivf index, not just the config"
        );
        assert!(e2.insert_record_from_f32_ns(&[1.0, 2.0], images).is_err());
        let hits = e2.search_l2_ns(&[1.0, 2.0, 3.0, 4.0], 1, images).unwrap();
        assert_eq!(hits.len(), 1);
    }

    /// KNOWN LIMITATION, deliberately tested and documented rather than
    /// silently left broken: `valori-kernel`'s snapshot wire format (V1–V8)
    /// stores exactly ONE vector byte-width for the whole snapshot (the
    /// header `dim` field) and decodes every record slot using it — it does
    /// not yet support records of genuinely different lengths coexisting in
    /// one snapshot. This phase added `namespace_configs` (dim/metric/index
    /// bookkeeping) WITHOUT changing that record layout — doing so safely
    /// requires reordering the snapshot so per-namespace config is readable
    /// BEFORE the records section (today it is appended after, for
    /// backward-compat reasons — see `encode.rs`/`decode.rs`), which is a
    /// real follow-up phase, not a "smallest extension."
    ///
    /// Practical effect: a project whose default namespace never receives a
    /// record AND whose only explicitly-configured collection has a
    /// dimension different from the legacy `dim` cannot currently survive a
    /// snapshot/restore cycle. The in-memory runtime (this same scenario,
    /// no restore) is unaffected — proven by every other test in this
    /// module. Only the durable round-trip is limited, and only when
    /// dimensions actually differ across namespaces in the same snapshot.
    #[test]
    fn snapshot_roundtrip_known_limitation_mixed_dimensions() {
        let mut e = Engine::with_config(tiny_cfg());
        let docs = e
            .create_collection_with_config(
                "docs",
                3,
                valori_domain::Metric::SquaredL2,
                valori_domain::IndexKind::Brute,
            )
            .unwrap();
        e.insert_record_from_f32_ns(&[1.0, 2.0, 3.0], docs).unwrap();

        let images = e
            .create_collection_with_config(
                "images",
                5,
                valori_domain::Metric::SquaredL2,
                valori_domain::IndexKind::Ivf,
            )
            .unwrap();
        e.insert_record_from_f32_ns(&[1.0, 2.0, 3.0, 4.0, 5.0], images)
            .unwrap();

        let snap = e.snapshot().unwrap();
        let mut e2 = Engine::with_config(tiny_cfg());
        // In the legacy whole-process snapshot format, records are serialized
        // assuming a single uniform vector length across the entire slab.
        // Mixed dimensions in a single legacy snapshot envelope cannot be
        // decoded by decode_state; the per-collection StorageProvider snapshot
        // path (tested below) is what supports mixed dimensions across collections.
        assert!(
            e2.restore(&snap).is_err(),
            "legacy single-blob snapshot restore is expected to fail on mixed dimension records"
        );
    }

    /// Phase 2.1 §25: the mandatory proof that mixed dimensions survive a
    /// REAL restart through the live `Engine` API — collection creation
    /// (which publishes manifests), `snapshot_collection_to_storage`
    /// (which durably materializes each collection + republishes its
    /// manifest), and `try_recover()` (which discovers collections from
    /// manifests and restores each with its own dimension) — using a
    /// brand-new `Engine` with zero access to the original's memory,
    /// exactly like the `worker_b_restores_from_storage_alone` scenario in
    /// `valori-state`, but exercised through the actual public `Engine` API
    /// a real node would call.
    #[test]
    fn mixed_dimensions_survive_real_restart_via_storage_provider() {
        use valori_storage::provider::local::LocalStorageProvider;

        let dir = tempfile::tempdir().unwrap();
        let provider: Arc<dyn valori_storage::provider::StorageProvider> =
            Arc::new(LocalStorageProvider::open(dir.path()).unwrap());
        let project_id = valori_domain::ProjectId::new();

        let (docs, images, products, doc_id, img_id, prod_id) = {
            let mut e = Engine::with_config(tiny_cfg());
            e.configure_storage_provider(provider.clone(), project_id, None);

            let docs = e
                .create_collection_with_config(
                    "documents",
                    384,
                    valori_domain::Metric::SquaredL2,
                    valori_domain::IndexKind::Brute,
                )
                .unwrap();
            let images = e
                .create_collection_with_config(
                    "images",
                    768,
                    valori_domain::Metric::SquaredL2,
                    valori_domain::IndexKind::Brute,
                )
                .unwrap();
            let products = e
                .create_collection_with_config(
                    "products",
                    1536,
                    valori_domain::Metric::SquaredL2,
                    valori_domain::IndexKind::Brute,
                )
                .unwrap();

            let doc_id = e
                .insert_record_from_f32_ns(&vec![1.0f32; 384], docs)
                .unwrap();
            let img_id = e
                .insert_record_from_f32_ns(&vec![2.0f32; 768], images)
                .unwrap();
            let prod_id = e
                .insert_record_from_f32_ns(&vec![3.0f32; 1536], products)
                .unwrap();

            e.snapshot_collection_to_storage(valori_kernel::types::id::NamespaceId(docs), 1)
                .unwrap();
            e.snapshot_collection_to_storage(valori_kernel::types::id::NamespaceId(images), 1)
                .unwrap();
            e.snapshot_collection_to_storage(valori_kernel::types::id::NamespaceId(products), 1)
                .unwrap();

            (docs, images, products, doc_id, img_id, prod_id)
            // `e` (and its in-memory state) is dropped here — nothing below
            // has access to it.
        };

        // Brand-new Engine, same provider, same project — the actual
        // Engine::try_recover() entry point a real node startup calls.
        let mut e2 = Engine::with_config(tiny_cfg());
        e2.configure_storage_provider(provider, project_id, None);
        let mode = e2.try_recover();
        assert!(
            matches!(mode, RecoveryMode::StorageProvider(_)),
            "expected the new path, got {mode:?}"
        );

        // Correct dimension AND correct data per collection — no global
        // snapshot dimension anywhere in this path.
        assert_eq!(e2.state.namespace_dim(docs), Some(384));
        assert_eq!(e2.state.namespace_dim(images), Some(768));
        assert_eq!(e2.state.namespace_dim(products), Some(1536));

        assert_eq!(
            e2.state
                .get_record(RecordId(doc_id))
                .unwrap()
                .vector
                .data
                .len(),
            384
        );
        assert_eq!(
            e2.state
                .get_record(RecordId(img_id))
                .unwrap()
                .vector
                .data
                .len(),
            768
        );
        assert_eq!(
            e2.state
                .get_record(RecordId(prod_id))
                .unwrap()
                .vector
                .data
                .len(),
            1536
        );

        // Search/insert validation continues to work correctly per
        // collection after recovery — wrong dimension is still rejected,
        // right dimension still succeeds, for every collection.
        assert!(e2
            .insert_record_from_f32_ns(&vec![9.0f32; 768], docs)
            .is_err());
        e2.insert_record_from_f32_ns(&vec![9.0f32; 384], docs)
            .unwrap();
        let hits = e2.search_l2_ns(&vec![1.0f32; 384], 5, docs).unwrap();
        assert!(hits.iter().any(|(id, _)| *id == doc_id));
        let hits = e2.search_l2_ns(&vec![2.0f32; 768], 5, images).unwrap();
        assert!(hits.iter().any(|(id, _)| *id == img_id));
    }

    #[test]
    fn test_create_edge_ns_and_deletion() {
        let mut e = Engine::with_config(tiny_cfg());
        let ns = e
            .create_collection_with_config(
                "tenant-test",
                4,
                valori_domain::Metric::SquaredL2,
                valori_domain::IndexKind::Brute,
            )
            .unwrap();
        let n1 = e.create_node_for_record(None, 0, ns).unwrap();
        let n2 = e.create_node_for_record(None, 1, ns).unwrap();
        let e1 = e.create_edge_ns(n1, n2, 6, ns).unwrap();

        // Edge creation in non-zero namespace should succeed and return edge ID
        assert_eq!(e1, 0);

        // Create a second edge
        let n3 = e.create_node_for_record(None, 1, ns).unwrap();
        let e2 = e.create_edge_ns(n1, n3, 6, ns).unwrap();
        assert_eq!(e2, 1);

        // Delete edge 0
        e.delete_edge(e1).unwrap();

        // Creating a third edge after edge deletion must use next_edge_id() (2)
        // and NOT edge_count() (1), preserving ID monotonicity.
        let e3 = e.create_edge_ns(n2, n3, 6, ns).unwrap();
        assert_eq!(e3, 2);
    }
}
