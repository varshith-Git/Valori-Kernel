// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Node Engine - The High-Level Orchestrator
//!
//! This module coordinates the Valori Kernel with persistence, indexing,
//! and node-level services.

use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::encode_state;
use valori_kernel::types::id::RecordId;
use valori_kernel::fxp::qformat::SCALE;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::enums::{NodeKind, EdgeKind};

use crate::config::{NodeConfig, IndexKind, QuantizationKind};
use crate::structure::index::{VectorIndex, BruteForceIndex};
use crate::structure::quant::{Quantizer, NoQuantizer, ScalarQuantizer};
use crate::wal_writer::WalWriter;
use crate::events::event_commit::EventCommitter;
use crate::events::event_log::EventLogWriter;
use crate::events::event_journal::EventJournal;
use crate::errors::EngineError;
use valori_kernel::error::KernelError;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ── Health response types ─────────────────────────────────────────────────────

/// Utilisation stats for a single bounded pool (records, nodes, or edges).
#[derive(Debug, serde::Serialize)]
pub struct PoolStats {
    /// Number of live (non-deleted) entries.
    pub live: usize,
    /// Total allocated slots, including soft-deleted tombstones.
    pub slots_used: usize,
    /// Hard capacity limit from config.
    pub capacity: usize,
    /// `live / capacity × 100`, rounded to one decimal place.
    pub fill_pct: f64,
}

/// Structured response for `GET /health`.
///
/// `status` drives load-balancer routing:
/// * `"ok"`       → 200, route freely
/// * `"degraded"` → 200, any pool ≥ 90 % full; still serves all operations
///                  but operator should increase capacity soon
/// * `"full"`     → 503, at least one pool at 100 %; inserts will be rejected
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
    /// Height of the event journal if the event log is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_log_height: Option<u64>,
}

/// Result of `Engine::try_recover()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryMode {
    /// Recovered by replaying N events from the event log.
    EventLog(u64),
    /// Recovered by loading a snapshot file.
    Snapshot,
    /// No durable state found; engine started from scratch.
    Fresh,
}

/// Namespace registry: maps collection name → NamespaceId (u16).
///
/// "default" is always id 0 and is never stored in the map (hardcoded).
/// All other names are allocated sequentially starting at 1.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct NamespaceRegistry {
    pub map: HashMap<String, u16>,
    pub next_id: u16,
}

impl NamespaceRegistry {
    pub fn new() -> Self {
        Self { map: HashMap::new(), next_id: 1 }
    }

    /// Resolve a collection name to a NamespaceId.
    /// Returns `Some(0)` for `None` or `"default"`, `Some(id)` for registered names,
    /// `None` for unknown names.
    pub fn resolve(&self, name: Option<&str>) -> Option<u16> {
        match name {
            None | Some("default") => Some(0),
            Some(n) => self.map.get(n).copied(),
        }
    }

    /// Create a collection; idempotent — returns existing id if already registered.
    /// Returns error if `MAX_NAMESPACES` (1024) would be exceeded or name is "default".
    pub fn create(&mut self, name: &str) -> Result<u16, EngineError> {
        if name == "default" {
            return Ok(0);
        }
        if let Some(&id) = self.map.get(name) {
            return Ok(id);
        }
        if self.next_id as usize >= valori_kernel::types::id::MAX_NAMESPACES {
            return Err(EngineError::InvalidInput(format!(
                "namespace limit reached ({} max)", valori_kernel::types::id::MAX_NAMESPACES
            )));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.map.insert(name.to_string(), id);
        Ok(id)
    }

    /// Drop a collection by name. Returns its former id, or None if not found.
    /// "default" (id 0) cannot be dropped.
    pub fn drop_collection(&mut self, name: &str) -> Option<u16> {
        if name == "default" { return None; }
        self.map.remove(name)
    }

    /// All collections including the implicit "default".
    pub fn list(&self) -> Vec<(String, u16)> {
        let mut out = vec![("default".to_string(), 0u16)];
        let mut rest: Vec<_> = self.map.iter().map(|(k, &v)| (k.clone(), v)).collect();
        rest.sort_by_key(|&(_, id)| id);
        out.extend(rest);
        out
    }
}

/// The Node Engine orchestrates state, persistence, and indexing.
pub struct Engine {
    pub state: KernelState,
    pub metadata: crate::metadata::MetadataStore,
    pub index: Box<dyn VectorIndex + Send + Sync>,
    pub quant: Box<dyn Quantizer + Send + Sync>,

    // Config tracking
    pub index_kind: IndexKind,
    pub quantization_kind: QuantizationKind,
    pub wal_path: Option<PathBuf>,
    pub snapshot_path: Option<PathBuf>,

    // Capacity limits — stored so health() and metrics can compute fill ratios
    // without needing the original NodeConfig to be kept alive.
    pub max_records: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub dim: usize,

    // WAL Persistence (Phase 20)
    pub wal_writer: Option<WalWriter>,
    pub wal_accumulator: blake3::Hasher,

    // Event-sourced persistence (Phase 23 - NEW)
    pub event_committer: Option<EventCommitter>,

    /// Sidecar file for `MetadataStore` (JSON key-value pairs set via
    /// `set_metadata` / `meta_set`).  Written atomically on every mutation;
    /// loaded by `try_recover()` after event-log replay so user metadata
    /// survives crashes without needing its own event-log entries.
    pub metadata_path: Option<PathBuf>,

    /// Derived index: record_id → node_id.
    /// Enables O(1) auto-cascade: deleting a vector automatically removes its graph node.
    /// Not persisted — rebuilt from the node pool on restore.
    pub record_to_node: HashMap<u32, u32>,

    /// Collection (namespace) registry — maps names to NamespaceIds.
    pub namespaces: NamespaceRegistry,

    /// Phase 3.1: object-store backend for snapshot offload and WAL archival.
    /// `None` when `VALORI_OBJECT_STORE_URL` is not set.
    pub object_store: Option<Arc<crate::object_store::ObjectStoreBackend>>,

    /// Number of snapshots to keep in the object store after pruning.
    pub object_store_keep: u32,
}

impl Engine {
    pub fn new(cfg: &NodeConfig) -> Self {
         // Initialize Index
         let index: Box<dyn VectorIndex + Send + Sync> = match cfg.index_kind {
              IndexKind::BruteForce => Box::new(BruteForceIndex::new()),
              IndexKind::Hnsw => {
                  use crate::structure::hnsw::HnswIndex;
                  Box::new(HnswIndex::new())
              },
              IndexKind::Ivf => {
                  use crate::structure::ivf::{IvfIndex, IvfConfig};
                  Box::new(IvfIndex::new(IvfConfig::default(), cfg.dim))
              }
         };

        // Initialize Quantizer
        let quant: Box<dyn Quantizer + Send + Sync> = match cfg.quantization_kind {
            QuantizationKind::None => Box::new(NoQuantizer),
            QuantizationKind::Scalar => Box::new(ScalarQuantizer {}),
            QuantizationKind::Product => {
                use crate::structure::quant::pq::{ProductQuantizer, PqConfig};
                Box::new(ProductQuantizer::new(PqConfig::default(), cfg.dim))
            }
        };

        // WAL is the legacy persistence path.  When the event log is active it
        // supersedes the WAL entirely — every mutation goes through EventCommitter.
        // Initialising both would waste an fd and create a confusing dual-write.
        let wal_writer = if cfg.event_log_path.is_none() {
            if let Some(ref path) = cfg.wal_path {
                match WalWriter::open(path, cfg.dim as u32) {
                    Ok(writer) => {
                        tracing::info!("WAL initialized at {:?}", path);
                        Some(writer)
                    },
                    Err(e) => {
                        tracing::error!("Failed to open WAL: {}", e);
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        
        let wal_accumulator = blake3::Hasher::new();
        
        let event_committer = if let Some(ref path) = cfg.event_log_path {
             match EventLogWriter::open(path, Some(cfg.dim as u32)) {
                 Ok(log_writer) => {
                     let journal = EventJournal::new();
                     let live_state = KernelState::new();
                     let mut committer = EventCommitter::new(log_writer, journal, live_state);
                     if let Some(limit) = cfg.event_log_rotation_bytes {
                         committer = committer.with_rotation_bytes(if limit == 0 { None } else { Some(limit) });
                     }
                     Some(committer)
                 }
                 Err(e) => {
                     tracing::error!("Failed to open Event Log: {}", e);
                     None
                 }
             }
        } else {
            None
        };

        // Derive the metadata sidecar path from the event log path so the two
        // files always live in the same directory and are easy to identify.
        let metadata_path = cfg.event_log_path.as_ref()
            .map(|p| p.with_extension("metadata.json"));

        Self {
            state: KernelState::new(),
            metadata: crate::metadata::MetadataStore::new(),
            index,
            quant,
            index_kind: cfg.index_kind,
            quantization_kind: cfg.quantization_kind,
            wal_path: cfg.wal_path.clone(),
            snapshot_path: cfg.snapshot_path.clone(),
            max_records: cfg.max_records,
            max_nodes: cfg.max_nodes,
            max_edges: cfg.max_edges,
            dim: cfg.dim,
            wal_writer,
            wal_accumulator,
            event_committer,
            record_to_node: HashMap::new(),
            metadata_path,
            namespaces: NamespaceRegistry::new(),
            object_store: crate::object_store::ObjectStoreBackend::from_env(),
            object_store_keep: cfg.object_store_keep,
        }
    }

    /// Rebuild the `record_to_node` map from the current node pool.
    /// Called after snapshot restore so the derived index stays consistent.
    fn rebuild_record_to_node(&mut self) {
        self.record_to_node.clear();
        for node in self.state.iter_nodes() {
            if let Some(rid) = node.record {
                self.record_to_node.insert(rid.0, node.id.0);
            }
        }
    }

    // ── Metadata sidecar helpers ──────────────────────────────────────────────

    /// Atomically persist the `MetadataStore` to the sidecar file.
    ///
    /// No-op when `metadata_path` is not configured.  Called by every HTTP /
    /// FFI handler that mutates metadata so the file stays in sync.
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

    /// Load the `MetadataStore` from the sidecar file, replacing in-memory state.
    ///
    /// A missing file is silently treated as an empty store (valid fresh start).
    /// No-op when `metadata_path` is not configured.  Called by `try_recover()`
    /// after every successful recovery branch.
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

    // ── Observability ─────────────────────────────────────────────────────────

    /// Snapshot the current engine state into a structured health report.
    ///
    /// `status` is computed from pool fill levels:
    /// * `"full"`     — any pool at 100 % capacity; inserts will be rejected → 503
    /// * `"degraded"` — any pool ≥ 90 % full; still operational but needs attention → 200
    /// * `"ok"`       — all pools below 90 % → 200
    pub fn health(&self) -> EngineHealth {
        let live_records  = self.state.record_count();
        let slot_records  = self.state.total_record_slots();
        let live_nodes    = self.state.node_count();
        let live_edges    = self.state.edge_count();

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

        let persistence = if self.event_committer.is_some() {
            "event_log"
        } else if self.wal_writer.is_some() {
            "wal"
        } else if self.snapshot_path.is_some() {
            "snapshot"
        } else {
            "none"
        };

        let event_log_height = self.event_committer.as_ref()
            .map(|c| c.journal().committed_height());

        EngineHealth {
            status,
            version: env!("CARGO_PKG_VERSION"),
            dim: self.dim,
            index: format!("{:?}", self.index_kind),
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
            event_log_height,
        }
    }

    /// Push current KernelState gauges into the Prometheus recorder.
    ///
    /// Called by both `GET /health` and `GET /metrics` so the scrape always
    /// reflects the live state rather than a value that was last set during a
    /// mutation.
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

        if let Some(ref c) = self.event_committer {
            metrics::gauge!("valori_event_log_height", c.journal().committed_height() as f64);
        }
    }

    /// Insert into the default namespace (backward-compat entry point).
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
                return Err(EngineError::InvalidInput("Vector values must be between -32768.0 and 32767.99".to_string()));
            }
            fxp_data.push(FxpScalar((v * SCALE as f32) as i32));
        }
        let vector = FxpVector { data: fxp_data };
        let rid = self.state.next_record_id();

        let event = valori_kernel::event::KernelEvent::InsertRecord {
            id: rid,
            vector,
            metadata: None,
            tag: 0,
        };

        if let Some(ref mut committer) = self.event_committer {
            committer.commit_event(event.clone()).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            self.apply_committed_event_ns(&event, namespace_id)?;
        } else {
            let (rid, vector) = if let valori_kernel::event::KernelEvent::InsertRecord { id, vector, .. } = &event {
                (*id, vector.clone())
            } else {
                unreachable!()
            };

            let cmd = Command::InsertRecord {
                namespace_id,
                id: rid,
                vector,
                metadata: None,
                tag: 0,
            };
            if let Some(ref mut writer) = self.wal_writer {
                writer.append_command(&cmd).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            }
            self.state.apply(&cmd)?;
            if namespace_id == valori_kernel::types::id::DEFAULT_NS.0 {
                self.index.insert(rid.0, values);
            }
        }

        Ok(rid.0)
    }

    pub fn insert_batch(&mut self, batch: &[Vec<f32>]) -> Result<Vec<u32>, EngineError> {
        self.insert_batch_ns(batch, valori_kernel::types::id::DEFAULT_NS.0)
    }

    pub fn insert_batch_ns(&mut self, batch: &[Vec<f32>], namespace_id: u16) -> Result<Vec<u32>, EngineError> {
        // ── Capacity guard ─────────────────────────────────────────────────────
        // Reject the entire batch atomically if it would overflow the record
        // pool.  This prevents partial writes: either the whole batch fits, or
        // none of it is committed.
        if self.state.record_count() + batch.len() > self.max_records {
            return Err(EngineError::Kernel(KernelError::CapacityExceeded));
        }

        if let Some(ref mut committer) = self.event_committer {
            let mut events = Vec::with_capacity(batch.len());
            let mut ids = Vec::with_capacity(batch.len());
            let start_id = self.state.next_record_id().0;

            for (i, values) in batch.iter().enumerate() {
                let mut fxp_data = Vec::with_capacity(values.len());
                for &v in values {
                    if v > 32767.99 || v < -32768.0 {
                        return Err(EngineError::InvalidInput("Vector values must be between -32768.0 and 32767.99".to_string()));
                    }
                    fxp_data.push(FxpScalar((v * SCALE as f32) as i32));
                }
                let id = start_id + i as u32;
                events.push(valori_kernel::event::KernelEvent::InsertRecord {
                    id: RecordId(id),
                    vector: FxpVector { data: fxp_data },
                    metadata: None,
                    tag: 0,
                });
                ids.push(id);
            }

            committer.commit_batch(events.clone()).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            for event in &events {
                self.apply_committed_event_ns(event, namespace_id)?;
            }
            Ok(ids)
        } else {
            let mut ids = Vec::with_capacity(batch.len());
            for values in batch {
                ids.push(self.insert_record_from_f32_ns(values, namespace_id)?);
            }
            Ok(ids)
        }
    }

    pub fn search_l2(&self, query: &[f32], k: usize) -> Result<Vec<(u32, f32)>, EngineError> {
        for &v in query {
            if v > 32767.99 || v < -32768.0 {
                return Err(EngineError::InvalidInput("Query vector values must be between -32768.0 and 32767.99".to_string()));
            }
        }
        Ok(self.index.search(query, k))
    }

    // ── Collection / namespace management ────────────────────────────────────

    /// Resolve an optional collection name to a kernel NamespaceId.
    /// Returns an error for unknown names.
    pub fn resolve_collection(&self, name: Option<&str>) -> Result<u16, EngineError> {
        self.namespaces.resolve(name).ok_or_else(|| {
            EngineError::InvalidInput(format!(
                "unknown collection '{}' — create it first with POST /v1/namespaces",
                name.unwrap_or("default")
            ))
        })
    }

    /// Create a new collection. Idempotent — returns existing id if already present.
    pub fn create_collection(&mut self, name: &str) -> Result<u16, EngineError> {
        let id = self.namespaces.create(name)?;
        // Tell the kernel about the namespace (no-op if already exists).
        let cmd = valori_kernel::state::command::Command::CreateNamespace { namespace_id: id };
        self.state.apply(&cmd)?;
        Ok(id)
    }

    /// Drop a collection and all its records/nodes.
    pub fn drop_collection(&mut self, name: &str) -> Result<(), EngineError> {
        if name == "default" {
            return Err(EngineError::InvalidInput(
                "the 'default' collection cannot be dropped".into(),
            ));
        }
        let id = self.namespaces.drop_collection(name).ok_or_else(|| {
            EngineError::InvalidInput(format!("collection '{name}' not found"))
        })?;
        let cmd = valori_kernel::state::command::Command::DropNamespace { namespace_id: id };
        self.state.apply(&cmd)?;
        Ok(())
    }

    /// List all collections with their ids.
    pub fn list_collections(&self) -> Vec<(String, u16)> {
        self.namespaces.list()
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, EngineError> {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"VAL1");

        // Compute a tight upper bound for the kernel state encoding.
        // Per record slot: 1 (presence flag) + 4 (id) + 1 (flags) + 8 (tag)
        //                  + dim×4 (Q16.16 vector) + 4 (metadata len) = 18 + dim×4
        // Per node: up to 15 bytes; per edge: up to 18 bytes.
        // Header: 64 bytes.  2 MB safety margin covers node/edge metadata variance.
        let dim = self.state.dim.unwrap_or(384);
        let total_slots = self.state.total_record_slots();
        let node_count  = self.state.node_count();
        let edge_count  = self.state.edge_count();
        // V4 layout: nodes gain `first_in_edge` (5 bytes), edges gain `next_in` (5 bytes)
        let k_buf_size  = 64
            + total_slots * (18 + dim * 4)
            + node_count  * 25   // 20 + 5 for first_in_edge
            + edge_count  * 29   // 24 + 5 for next_in
            + 2 * 1024 * 1024;   // 2 MB safety margin
        let mut k_buf = vec![0u8; k_buf_size];
        let k_len = encode_state(&self.state, &mut k_buf)?;
        k_buf.truncate(k_len);
        buffer.extend_from_slice(&(k_len as u32).to_le_bytes());
        buffer.extend_from_slice(&k_buf);

        let m_buf = self.metadata.snapshot();
        buffer.extend_from_slice(&(m_buf.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&m_buf);

        let i_buf = self.index.snapshot().map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        buffer.extend_from_slice(&(i_buf.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&i_buf);

        // Namespace registry section (magic tag "NSRG" + u32 len + JSON bytes).
        let ns_json = serde_json::to_vec(&self.namespaces)
            .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        buffer.extend_from_slice(b"NSRG");
        buffer.extend_from_slice(&(ns_json.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&ns_json);

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

        // Read kernel section ─────────────────────────────────────────────────
        if offset + 4 > data.len() {
            return Err(EngineError::InvalidInput("Truncated snapshot: missing k_len".into()));
        }
        let k_len = u32::from_le_bytes(data[offset..offset+4].try_into()
            .map_err(|_| EngineError::InvalidInput("Failed to read k_len".into()))?) as usize;
        offset += 4;
        if offset + k_len > data.len() {
            return Err(EngineError::InvalidInput("Truncated snapshot: k_data out of bounds".into()));
        }
        let k_data = &data[offset..offset+k_len];
        offset += k_len;

        // Read metadata section ────────────────────────────────────────────────
        if offset + 4 > data.len() {
            return Err(EngineError::InvalidInput("Truncated snapshot: missing m_len".into()));
        }
        let m_len = u32::from_le_bytes(data[offset..offset+4].try_into()
            .map_err(|_| EngineError::InvalidInput("Failed to read m_len".into()))?) as usize;
        offset += 4;
        if offset + m_len > data.len() {
            return Err(EngineError::InvalidInput("Truncated snapshot: m_data out of bounds".into()));
        }
        let m_data = &data[offset..offset+m_len];
        offset += m_len;

        // Read index section ───────────────────────────────────────────────────
        if offset + 4 > data.len() {
            return Err(EngineError::InvalidInput("Truncated snapshot: missing i_len".into()));
        }
        let i_len = u32::from_le_bytes(data[offset..offset+4].try_into()
            .map_err(|_| EngineError::InvalidInput("Failed to read i_len".into()))?) as usize;
        offset += 4;
        let i_data = if offset + i_len <= data.len() {
             Some(&data[offset..offset+i_len])
        } else {
             None
        };
        offset += i_len;

        // Namespace registry section (optional — older snapshots lack it).
        let ns_registry: Option<NamespaceRegistry> = if offset + 4 <= data.len()
            && &data[offset..offset + 4] == b"NSRG"
        {
            offset += 4;
            let ns_len = u32::from_le_bytes(
                data[offset..offset + 4].try_into()
                    .map_err(|_| EngineError::InvalidInput("Failed to read ns_len".into()))?,
            ) as usize;
            offset += 4;
            if offset + ns_len > data.len() {
                return Err(EngineError::InvalidInput("Truncated snapshot: ns_data out of bounds".into()));
            }
            let ns_json = &data[offset..offset + ns_len];
            Some(serde_json::from_slice(ns_json)
                .map_err(|e| EngineError::InvalidInput(format!("ns registry decode: {e}")))?)
        } else {
            None
        };

        self.restore_from_components(k_data, m_data, i_data, ns_registry)
    }

    /// Soft-delete a record: mark it as a tombstone and remove it from the search index.
    /// Also auto-deletes any graph node that references this record (Issue 4).
    pub fn soft_delete_record(&mut self, id: u32) -> Result<(), EngineError> {
        // Auto-cascade: remove the associated graph node first
        if let Some(node_id) = self.record_to_node.get(&id).copied() {
            self.delete_node(node_id)?;
        }

        let rid = RecordId(id);
        let event = valori_kernel::event::KernelEvent::SoftDeleteRecord { id: rid };

        if let Some(ref mut committer) = self.event_committer {
            committer.commit_event(event.clone()).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            self.apply_committed_event(&event)?;
        } else {
            let cmd = valori_kernel::state::command::Command::SoftDeleteRecord { id: rid };
            if let Some(ref mut writer) = self.wal_writer {
                writer.append_command(&cmd).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            }
            self.state.apply(&cmd)?;
            self.index.delete(id);
        }
        Ok(())
    }

    /// Hard-delete a record and its associated graph node (if any).
    pub fn delete_record(&mut self, id: u32) -> Result<(), EngineError> {
        // Auto-cascade: remove the associated graph node first
        if let Some(node_id) = self.record_to_node.get(&id).copied() {
            self.delete_node(node_id)?;
        }

        let rid = RecordId(id);
        let event = valori_kernel::event::KernelEvent::DeleteRecord { id: rid };

        if let Some(ref mut committer) = self.event_committer {
            committer.commit_event(event.clone()).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            self.apply_committed_event(&event)?;
        } else {
            let cmd = Command::DeleteRecord { id: rid };
            if let Some(ref mut writer) = self.wal_writer {
                writer.append_command(&cmd).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            }
            self.state.apply(&cmd)?;
            self.index.delete(id);
        }
        Ok(())
    }

    /// Delete a graph node and cascade-delete all its incident edges.
    /// Writes a `DeleteNode` event to the WAL / event log so the deletion survives crashes.
    pub fn delete_node(&mut self, id: u32) -> Result<(), EngineError> {
        use valori_kernel::types::id::NodeId;
        let node_id = NodeId(id);

        let event = valori_kernel::event::KernelEvent::DeleteNode { id: node_id };
        if let Some(ref mut committer) = self.event_committer {
            committer.commit_event(event.clone()).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            self.apply_committed_event(&event)?;
        } else {
            let cmd = Command::DeleteNode { node_id };
            if let Some(ref mut writer) = self.wal_writer {
                writer.append_command(&cmd).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            }
            // Pre-apply: clean up record_to_node before the node is gone
            if let Some(node) = self.state.get_node(node_id) {
                if let Some(rid) = node.record {
                    self.record_to_node.remove(&rid.0);
                }
            }
            self.state.apply(&cmd)?;
        }
        Ok(())
    }

    /// Delete a single graph edge by ID.
    /// Writes a `DeleteEdge` event to the WAL / event log.
    pub fn delete_edge(&mut self, id: u32) -> Result<(), EngineError> {
        use valori_kernel::types::id::EdgeId;
        let edge_id = EdgeId(id);

        let event = valori_kernel::event::KernelEvent::DeleteEdge { id: edge_id };
        if let Some(ref mut committer) = self.event_committer {
            committer.commit_event(event.clone()).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            self.apply_committed_event(&event)?;
        } else {
            let cmd = Command::DeleteEdge { edge_id };
            if let Some(ref mut writer) = self.wal_writer {
                writer.append_command(&cmd).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            }
            self.state.apply(&cmd)?;
        }
        Ok(())
    }

    pub fn create_node_for_record(&mut self, record_id: Option<u32>, kind: u8) -> Result<u32, EngineError> {
         // ── Capacity guard ────────────────────────────────────────────────────
         if self.state.node_count() >= self.max_nodes {
             return Err(EngineError::Kernel(KernelError::CapacityExceeded));
         }

         use valori_kernel::types::id::NodeId;
         let node_id = NodeId(self.state.node_count() as u32);
         let kind = NodeKind::from_u8(kind).unwrap_or_default();
         let record = record_id.map(RecordId);

         let event = valori_kernel::event::KernelEvent::CreateNode {
             id: node_id,
             kind,
             record,
         };

         if let Some(ref mut committer) = self.event_committer {
             committer.commit_event(event.clone()).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
             self.apply_committed_event(&event)?;
         } else {
             let cmd = Command::CreateNode { namespace_id: valori_kernel::types::id::DEFAULT_NS.0, node_id, kind, record };
             if let Some(ref mut writer) = self.wal_writer {
                 writer.append_command(&cmd).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
             }
             self.state.apply(&cmd)?;
             // Keep derived record→node index in sync even without event log
             if let Some(r) = record {
                 self.record_to_node.insert(r.0, node_id.0);
             }
         }
         Ok(node_id.0)
    }

    pub fn create_edge(&mut self, from: u32, to: u32, kind: u8) -> Result<u32, EngineError> {
         // ── Capacity guard ────────────────────────────────────────────────────
         if self.state.edge_count() >= self.max_edges {
             return Err(EngineError::Kernel(KernelError::CapacityExceeded));
         }

         use valori_kernel::types::id::{NodeId, EdgeId};
         let edge_id = EdgeId(self.state.edge_count() as u32);
         let kind = EdgeKind::from_u8(kind).unwrap_or_default();
         let from = NodeId(from);
         let to = NodeId(to);

         let event = valori_kernel::event::KernelEvent::CreateEdge {
             id: edge_id,
             kind,
             from,
             to,
         };

         if let Some(ref mut committer) = self.event_committer {
             committer.commit_event(event.clone()).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
             self.apply_committed_event(&event)?;
         } else {
             let cmd = Command::CreateEdge { edge_id, kind, from, to };
             if let Some(ref mut writer) = self.wal_writer {
                 writer.append_command(&cmd).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
             }
             self.state.apply(&cmd)?;
         }
         Ok(edge_id.0)
    }

    pub fn get_proof(&self) -> valori_kernel::proof::DeterministicProof {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        let final_state_hash = hash_state_blake3(&self.state);
        valori_kernel::proof::DeterministicProof {
            kernel_version: 1,
            snapshot_hash: [0u8; 32], // Default for now
            wal_hash: [0u8; 32],      // Default for now
            final_state_hash,
        }
    }

    pub fn apply_committed_event(&mut self, event: &valori_kernel::event::KernelEvent) -> Result<(), EngineError> {
        use valori_kernel::event::KernelEvent;

        // ── Pre-apply: capture derived state BEFORE the kernel mutates it ──────
        match event {
            KernelEvent::DeleteNode { id } => {
                // The node is about to be deleted; capture its record association
                // so we can clean up record_to_node *before* the slot disappears.
                if let Some(node) = self.state.get_node(*id) {
                    if let Some(rid) = node.record {
                        self.record_to_node.remove(&rid.0);
                    }
                }
            }
            _ => {}
        }

        // ── Apply the event to the kernel state ──────────────────────────────
        self.state.apply_event(event)?;

        // ── Post-apply: update derived indexes AFTER the kernel mutates ───────
        match event {
            KernelEvent::InsertRecord { id, vector, .. } => {
                let mut vals = Vec::with_capacity(vector.data.len());
                for fxp in &vector.data {
                    vals.push(fxp.0 as f32 / SCALE as f32);
                }
                self.index.insert(id.0, &vals);
            }
            KernelEvent::DeleteRecord { id } => {
                self.index.delete(id.0);
            }
            KernelEvent::SoftDeleteRecord { id } => {
                self.index.delete(id.0);
            }
            KernelEvent::CreateNode { id, record, .. } => {
                if let Some(rid) = record {
                    self.record_to_node.insert(rid.0, id.0);
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Like `apply_committed_event` but routes the event into a specific namespace.
    pub fn apply_committed_event_ns(&mut self, event: &valori_kernel::event::KernelEvent, namespace_id: u16) -> Result<(), EngineError> {
        use valori_kernel::event::KernelEvent;

        match event {
            KernelEvent::DeleteNode { id } => {
                if let Some(node) = self.state.get_node(*id) {
                    if let Some(rid) = node.record {
                        self.record_to_node.remove(&rid.0);
                    }
                }
            }
            _ => {}
        }

        self.state.apply_event_ns(event, namespace_id)?;

        match event {
            KernelEvent::InsertRecord { id, vector, .. } => {
                // Non-default namespaces are searched via the kernel's intrusive
                // linked list (search_l2_ns); they must NOT enter the global index.
                if namespace_id == valori_kernel::types::id::DEFAULT_NS.0 {
                    let mut vals = Vec::with_capacity(vector.data.len());
                    for fxp in &vector.data {
                        vals.push(fxp.0 as f32 / SCALE as f32);
                    }
                    self.index.insert(id.0, &vals);
                }
            }
            KernelEvent::DeleteRecord { id } => { self.index.delete(id.0); }
            KernelEvent::SoftDeleteRecord { id } => { self.index.delete(id.0); }
            KernelEvent::CreateNode { id, record, .. } => {
                if let Some(rid) = record {
                    self.record_to_node.insert(rid.0, id.0);
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Add a namespace-scoped search method.
    pub fn search_l2_ns(&self, query: &[f32], k: usize, namespace_id: u16) -> Result<Vec<(u32, f32)>, EngineError> {
        use valori_kernel::index::SearchResult;
        use valori_kernel::types::scalar::FxpScalar;
        use valori_kernel::types::vector::FxpVector;

        for &v in query {
            if v > 32767.99 || v < -32768.0 {
                return Err(EngineError::InvalidInput("Query vector values must be between -32768.0 and 32767.99".to_string()));
            }
        }

        let fxp_data: Vec<FxpScalar> = query.iter()
            .map(|&v| FxpScalar((v * SCALE as f32) as i32))
            .collect();
        let fxp_query = FxpVector { data: fxp_data };

        let mut results = vec![SearchResult::default(); k];
        let found = self.state.search_l2_ns(&fxp_query, &mut results, namespace_id);

        Ok(results[..found].iter().map(|r| {
            let score = r.score.0 as f32 / (SCALE as f32 * SCALE as f32);
            (r.id.0, score)
        }).collect())
    }

    /// Trigger a full index build from the current kernel state.
    ///
    /// Unlike `rebuild_index()`, which reconstructs the index by inserting each
    /// record one at a time, `build_index()` collects all live records into a
    /// batch and calls `VectorIndex::build()`.  This is important for
    /// cluster-based indexes like IVF that need to see the full data distribution
    /// before they can compute centroids.  Call this once after bulk-loading data.
    pub fn build_index(&mut self) {
        let total_slots = self.state.total_record_slots();
        let mut records: Vec<(u32, Vec<f32>)> = Vec::with_capacity(total_slots);
        for i in 0..total_slots {
            if let Some(record) = self.state.get_record(RecordId(i as u32)) {
                if !record.is_active() { continue; }
                // Non-default namespace records are found via the kernel's
                // intrusive linked list (search_l2_ns); skip the global index.
                if record.namespace_id != valori_kernel::types::id::DEFAULT_NS.0 { continue; }
                let vals: Vec<f32> = record.vector.data.iter()
                    .map(|fxp| fxp.0 as f32 / SCALE as f32)
                    .collect();
                records.push((i as u32, vals));
            }
        }
        self.index.build(&records);
    }

    /// Discard and rebuild the search index from the current kernel state.
    ///
    /// A fresh, empty index of the correct type is allocated first, then
    /// `build_index()` fills it from the record pool.  Using `build()` (batch
    /// path) rather than repeated `insert()` calls is critical for cluster-based
    /// indexes like IVF, which need to see the full data distribution before
    /// computing centroids.
    pub fn rebuild_index(&mut self) {
         // Replace the live index with a fresh empty one of the same type.
         let blank: Box<dyn VectorIndex + Send + Sync> = match self.index_kind {
              IndexKind::BruteForce => Box::new(BruteForceIndex::new()),
              IndexKind::Hnsw => {
                  use crate::structure::hnsw::HnswIndex;
                  Box::new(HnswIndex::new())
              },
              IndexKind::Ivf => {
                  use crate::structure::ivf::{IvfIndex, IvfConfig};
                  let dim = self.state.dim.unwrap_or(0);
                  Box::new(IvfIndex::new(IvfConfig::default(), dim))
              }
         };
         self.index = blank;

         // Batch-build from the full record set (critical for IVF centroid init).
         self.build_index();
    }

    /// Attempt crash recovery using the best available source, in priority order:
    ///
    /// 1. **Event log** — canonical truth.  If the event log file exists and
    ///    contains at least one committed event, replay all events from scratch
    ///    to rebuild `state`, the search index, and `record_to_node`.  The
    ///    existing `EventCommitter` is torn down and rebuilt with the recovered
    ///    journal so that future commits append correctly to the existing file.
    ///
    /// 2. **Snapshot** — fast-path cache.  Loaded only when the event log is
    ///    absent or empty.
    ///
    /// 3. **Fresh start** — no durable state found.
    ///
    /// The method never panics.  On partial failure it logs an error and falls
    /// through to the next priority.
    pub fn try_recover(&mut self) -> RecoveryMode {
        // ── Priority 1: event log ─────────────────────────────────────────────
        let log_info = self.event_committer.as_ref().map(|c| {
            (c.event_log().path().to_path_buf(), c.event_log().dim())
        });

        if let Some((log_path, dim)) = log_info {
            if log_path.exists() {
                match crate::recovery::recover_from_events(&log_path) {
                    Ok((recovered_state, recovered_journal, count)) => {
                        if count == 0 {
                            tracing::info!("Event log exists but is empty; trying snapshot");
                        } else {
                            tracing::info!("Event-log recovery: replaying {} events from {:?}", count, log_path);

                            // Drop the old committer (releases its BufWriter / file handle).
                            self.event_committer = None;

                            // Re-open the log for append (preserves existing content).
                            match EventLogWriter::open(&log_path, Some(dim)) {
                                Ok(log_writer) => {
                                    let state_for_committer = recovered_state.clone();
                                    self.state = recovered_state;
                                    self.event_committer = Some(EventCommitter::new(
                                        log_writer,
                                        recovered_journal,
                                        state_for_committer,
                                    ));
                                    self.rebuild_index();
                                    self.rebuild_record_to_node();
                                    self.load_metadata().ok();
                                    return RecoveryMode::EventLog(count);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to reopen event log after recovery: {}", e);
                                    // Fall through to snapshot.
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

        // ── Priority 2: snapshot ──────────────────────────────────────────────
        if let Some(path) = self.snapshot_path.clone() {
            if path.exists() {
                match std::fs::read(&path) {
                    Ok(data) => match self.restore(&data) {
                        Ok(()) => {
                            tracing::info!("Snapshot recovery succeeded from {:?}", path);
                            self.load_metadata().ok();
                            return RecoveryMode::Snapshot;
                        }
                        Err(e) => {
                            tracing::error!("Snapshot restore failed ({:?}); starting fresh", e);
                        }
                    },
                    Err(e) => {
                        tracing::error!("Failed to read snapshot file {:?}: {}", path, e);
                    }
                }
            }
        }

        // ── Priority 3: fresh start ───────────────────────────────────────────
        tracing::info!("No durable state found; starting from an empty store");
        RecoveryMode::Fresh
    }

    fn restore_from_components(&mut self, k_data: &[u8], m_data: &[u8], i_data: Option<&[u8]>, ns_registry: Option<NamespaceRegistry>) -> Result<(), EngineError> {
        self.state = decode_state(k_data)?;

        if !m_data.is_empty() {
             self.metadata.restore(m_data);
        }

        if let Some(blob) = i_data {
             if !blob.is_empty() {
                 self.index.restore(blob).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
             } else {
                 self.rebuild_index();
             }
        } else {
             self.rebuild_index();
        }

        // Always rebuild the derived record→node map after any restore
        self.rebuild_record_to_node();

        if let Some(reg) = ns_registry {
            self.namespaces = reg;
        }
        Ok(())
    }
}

// ── Module-level helpers for health computation ───────────────────────────────

/// Compute `used / capacity × 100`; returns 0.0 when capacity is 0.
#[inline]
fn pct(used: usize, capacity: usize) -> f64 {
    if capacity == 0 { 0.0 } else { used as f64 / capacity as f64 * 100.0 }
}

/// Round a percentage to one decimal place.
#[inline]
fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
