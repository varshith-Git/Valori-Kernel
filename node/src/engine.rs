// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;
use valori_kernel::event::KernelEvent;  // Phase 23: For event generation
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::{RecordId, NodeId, EdgeId};
use valori_kernel::types::enums::{NodeKind, EdgeKind};
use valori_kernel::snapshot::{encode::encode_state, decode::decode_state};
// use valori_kernel::fxp::ops::from_f32; // Explicit rounding now preferred
use valori_kernel::verify::{kernel_state_hash, snapshot_hash};
use valori_kernel::proof::DeterministicProof;

use crate::config::{NodeConfig, IndexKind, QuantizationKind};
use crate::errors::EngineError;
use crate::structure::index::{VectorIndex, BruteForceIndex};
use crate::structure::quant::{Quantizer, NoQuantizer, ScalarQuantizer};
use crate::metadata::MetadataStore;
use crate::wal_writer::WalWriter;

// Event-sourced persistence (Phase 23)
use crate::events::{EventCommitter, EventJournal, EventLogWriter, CommitResult};

use std::sync::Arc;

const SCALE: f32 = 65536.0;
const MAX_SAFE_F: f32 = (i32::MAX as f32) / SCALE; // ~32767.99
const MIN_SAFE_F: f32 = (i32::MIN as f32) / SCALE; // -32768.0

pub struct Engine<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize> {
    state: KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>,
    pub index_kind: IndexKind,
    pub quantization_kind: QuantizationKind,
    
    // Host-level extensions
    index: Box<dyn VectorIndex + Send + Sync>,
    quant: Box<dyn Quantizer + Send + Sync>,
    pub metadata: Arc<MetadataStore>,
    pub snapshot_path: Option<std::path::PathBuf>,
    pub wal_path: Option<std::path::PathBuf>,

    // Verification
    pub current_snapshot_hash: Option<[u8; 32]>,
    
    // WAL for durability (legacy - will be replaced by event_committer)
    wal_writer: Option<WalWriter<D>>,
    wal_accumulator: blake3::Hasher,
    
    // Event-sourced persistence (Phase 23 - NEW)
    // Optional during migration, will become mandatory after WAL deprecation
    pub event_committer: Option<EventCommitter<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>>,
    
    // Allocator State
    edge_bitmap: Vec<bool>,
}

impl<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize> Engine<MAX_RECORDS, D, MAX_NODES, MAX_EDGES> {
    pub fn new(cfg: &NodeConfig) -> Self {
        // Verify runtime config matches compile-time const generics
        assert_eq!(cfg.max_records, MAX_RECORDS, "Config max_records mismatch");
        assert_eq!(cfg.dim, D, "Config dim mismatch");
        assert_eq!(cfg.max_nodes, MAX_NODES, "Config max_nodes mismatch");
        assert_eq!(cfg.max_edges, MAX_EDGES, "Config max_edges mismatch");

         // Initialize Index
         let index: Box<dyn VectorIndex + Send + Sync> = match cfg.index_kind {
              IndexKind::BruteForce => Box::new(BruteForceIndex::new()),
              IndexKind::Hnsw => {
                  use crate::structure::hnsw::HnswIndex;
                  Box::new(HnswIndex::new())
              },
              IndexKind::Ivf => {
                  use crate::structure::ivf::{IvfIndex, IvfConfig};
                  // Use defaults for now, or derive from NodeConfig if we added params there
                  Box::new(IvfIndex::new(IvfConfig::default(), D))
              }
         };

        // Initialize Quantizer
        let quant: Box<dyn Quantizer + Send + Sync> = match cfg.quantization_kind {
            QuantizationKind::None => Box::new(NoQuantizer),
            QuantizationKind::Scalar => Box::new(ScalarQuantizer {}),
            QuantizationKind::Product => {
                use crate::structure::quant::pq::{ProductQuantizer, PqConfig};
                Box::new(ProductQuantizer::new(PqConfig::default(), D))
            }
        };

        // Initialize WAL if path configured
        let wal_writer = if let Some(ref path) = cfg.wal_path {
            match WalWriter::open(path) {
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
        };
        
        // Initialize Wal Accumulator (Default to Header only)
        // If WAL is replayed later, this will be overwritten.
        let mut wal_accumulator = blake3::Hasher::new();
        // Hash Header (16 bytes) match
         {
            let header_ver = 1u32;
            let enc_ver = 0u32;
            let dim = D as u32;
            let crc_len = 0u32;
            
            wal_accumulator.update(&header_ver.to_le_bytes());
            wal_accumulator.update(&enc_ver.to_le_bytes());
            wal_accumulator.update(&dim.to_le_bytes());
            wal_accumulator.update(&crc_len.to_le_bytes());
        }

        // Phase 23: Initialize Event Committer (event-sourced persistence)
        // Temporarily keep Engine.state for WAL compatibility during migration
        // Event log path derived from WAL directory
        let event_committer = if let Some(ref wal_path) = cfg.wal_path {
            if let Some(parent) = wal_path.parent() {
                let event_log_path = parent.join("events.log");
                match EventLogWriter::open(&event_log_path) {
                    Ok(event_log) => {
                        tracing::info!("Event log initialized at {:?}", event_log_path);
                        let journal = EventJournal::new();
                        // Create separate state for event committer
                        // TODO: Eventually Engine.state will be removed and only committer.state exists
                        let committer_state = KernelState::new();
                        Some(EventCommitter::new(event_log, journal, committer_state))
                    }
                    Err(e) => {
                        tracing::warn!("Event log not initialized: {}. Falling back to WAL-only mode.", e);
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        Self {
            state: KernelState::new(),
            index_kind: cfg.index_kind,
            quantization_kind: cfg.quantization_kind,
            index,
            quant,
            metadata: Arc::new(MetadataStore::new()),
            snapshot_path: cfg.snapshot_path.clone(),
            wal_path: cfg.wal_path.clone(),
            current_snapshot_hash: None,
            wal_writer,
            wal_accumulator,
            event_committer,  // Properly initialized
            edge_bitmap: vec![false; MAX_EDGES],
        }
    }



    pub fn insert_record_from_f32(&mut self, values: &[f32]) -> Result<u32, EngineError> {
        if values.len() != D {
            return Err(EngineError::InvalidInput(format!("Expected {} dimensions, got {}", D, values.len())));
        }

        // Validate Range for Q16.16 Safety
        for &v in values {
            if v > MAX_SAFE_F || v < MIN_SAFE_F {
                return Err(EngineError::InvalidInput(format!(
                    "Embedding value {} out of allowed range [{:.1}, {:.1}]",
                    v, MIN_SAFE_F, MAX_SAFE_F
                )));
            }
        }

        // 1. Build FxpVector for Kernel
        // STRICT DETERMINISM: Explicit Rounding to Nearest
        let mut vector = FxpVector::<D>::new_zeros();
        for (i, v) in values.iter().enumerate() {
            let fixed = (v * SCALE).round().clamp(i32::MIN as f32, i32::MAX as f32) as i32;
            vector.data[i] = FxpScalar(fixed);
        }

        // 2. Determine ID (first free slot strategy)
        let mut id_val = None;
        for i in 0..MAX_RECORDS {
            let rid = RecordId(i as u32);
            if self.state.get_record(rid).is_none() {
                id_val = Some(rid);
                break;
            }
        }
        let id = id_val.ok_or(valori_kernel::error::KernelError::CapacityExceeded)?;

        // Phase 23: Event-sourced path (preferred)
        if let Some(ref mut committer) = self.event_committer {
            // Generate event (no state change yet)
            let event = KernelEvent::InsertRecord { id, vector };
            
            // Commit via event pipeline (shadow → persist → commit → live)
            match committer.commit_event(event) {
                Ok(CommitResult::Committed) => {
                    // Event committed successfully
                    tracing::trace!("Record {} committed via event log", id.0);
                }
                Ok(CommitResult::RolledBack) => {
                    // Shadow apply failed - validation error
                    return Err(EngineError::InvalidInput(
                        "Event validation failed in shadow execution".to_string()
                    ));
                }
                Err(e) => {
                    return Err(EngineError::InvalidInput(format!("Event commit failed: {:?}", e)));
                }
            }
            
            // Update host index (using state from committer, not Engine.state)
            let mut consistent_values = Vec::with_capacity(D);
            for i in 0..D {
                let fxp = vector.data[i];
                let f = fxp.0 as f32 / SCALE;
                consistent_values.push(f);
            }
            self.index.insert(id.0, &consistent_values);
            
            Ok(id.0)
        } else {
            // Fallback: Legacy WAL path
            let cmd = Command::InsertRecord { id, vector };
            
            // Write to WAL FIRST
            if let Some(ref mut wal) = self.wal_writer {
                wal.append_command(&cmd)
                    .map_err(|e| EngineError::InvalidInput(format!("WAL write failed: {}", e)))?;
            }
            
            // Update Accumulator
            {
                let cmd_bytes = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
                    .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
                self.wal_accumulator.update(&cmd_bytes);
            }
            
            // Apply Command to Kernel
            self.state.apply(&cmd)?;
            
            // Update Host Index
            let mut consistent_values = Vec::with_capacity(D);
            for i in 0..D {
                let fxp = vector.data[i];
                let f = fxp.0 as f32 / SCALE;
                consistent_values.push(f);
            }
            self.index.insert(id.0, &consistent_values);
            
            Ok(id.0)
        }
    }

    pub fn create_node_for_record(&mut self, record_id_val: Option<u32>, kind_val: u8) -> Result<u32, EngineError> {
        let kind = NodeKind::from_u8(kind_val).ok_or(EngineError::InvalidInput("Invalid NodeKind".to_string()))?;
        let record_id = record_id_val.map(RecordId);

        // Find free Node ID
        let mut id_val = None;
        for i in 0..MAX_NODES {
             let nid = NodeId(i as u32);
             if self.state.get_node(nid).is_none() {
                 id_val = Some(nid);
                 break;
             }
        }
        let node_id = id_val.ok_or(valori_kernel::error::KernelError::CapacityExceeded)?;

        // Phase 23: Event-sourced path (preferred)
        if let Some(ref mut committer) = self.event_committer {
            let event = KernelEvent::CreateNode { id: node_id, kind, record: record_id };
            
            match committer.commit_event(event) {
                Ok(CommitResult::Committed) => {
                    tracing::trace!("Node {} created via event log", node_id.0);
                    Ok(node_id.0)
                }
                Ok(CommitResult::RolledBack) => {
                    Err(EngineError::InvalidInput(
                        "Node creation failed in shadow execution".to_string()
                    ))
                }
                Err(e) => {
                    Err(EngineError::InvalidInput(format!("Event commit failed: {:?}", e)))
                }
            }
        } else {
            // Fallback: Legacy WAL path
            let cmd = Command::CreateNode { node_id, kind, record: record_id };
            
            if let Some(ref mut wal) = self.wal_writer {
                wal.append_command(&cmd)
                    .map_err(|e| EngineError::InvalidInput(format!("WAL write failed: {}", e)))?;
            }
            
            self.state.apply(&cmd)?;
            Ok(node_id.0)
        }
    }

    pub fn create_edge(&mut self, from_val: u32, to_val: u32, kind_val: u8) -> Result<u32, EngineError> {
        let kind = EdgeKind::from_u8(kind_val).ok_or(EngineError::InvalidInput("Invalid EdgeKind".to_string()))?;
        let from = NodeId(from_val);
        let to = NodeId(to_val);

        // Find free Edge ID via bitmap scan
        let mut id_val = None;
        for i in 0..MAX_EDGES {
            if !self.edge_bitmap[i] {
                id_val = Some(EdgeId(i as u32));
                break;
            }
        }
        let edge_id = id_val.ok_or(valori_kernel::error::KernelError::CapacityExceeded)?;

        // Phase 23: Event-sourced path (preferred)
        if let Some(ref mut committer) = self.event_committer {
            let event = KernelEvent::CreateEdge { id: edge_id, kind, from, to };
            
            match committer.commit_event(event) {
                Ok(CommitResult::Committed) => {
                    tracing::trace!("Edge {} created via event log", edge_id.0);
                    // Update bitmap on success
                    self.edge_bitmap[edge_id.0 as usize] = true;
                    Ok(edge_id.0)
                }
                Ok(CommitResult::RolledBack) => {
                    Err(EngineError::InvalidInput(
                        "Edge creation failed in shadow execution".to_string()
                    ))
                }
                Err(e) => {
                    Err(EngineError::InvalidInput(format!("Event commit failed: {:?}", e)))
                }
            }
        } else {
            // Fallback: Legacy WAL path
            let cmd = Command::CreateEdge { edge_id, kind, from, to };
            
            if let Some(ref mut wal) = self.wal_writer {
                wal.append_command(&cmd)
                    .map_err(|e| EngineError::InvalidInput(format!("WAL write failed: {}", e)))?;
            }
            
            self.state.apply(&cmd).map_err(EngineError::Kernel)?;
            
            // Update bitmap on success
            self.edge_bitmap[edge_id.0 as usize] = true;
            Ok(edge_id.0)
        }
    }

    pub fn search_l2(&self, query: &[f32], k: usize) -> Result<Vec<(u32, i64)>, EngineError> {
        // Validate inputs
        if query.len() != D {
             return Err(EngineError::InvalidInput(format!("Expected {} dimensions, got {}", D, query.len())));
        }

        // Validate Range for Q16.16 Safety
        for &v in query {
            if v > MAX_SAFE_F || v < MIN_SAFE_F {
                return Err(EngineError::InvalidInput(format!(
                    "Query value {} out of allowed range [{:.1}, {:.1}]",
                    v, MIN_SAFE_F, MAX_SAFE_F
                )));
            }
        }

        let hits = self.index.search(query, k);
        
        // Convert f32 score to i64 with correct rounding and clamping
        Ok(hits.into_iter().map(|(id, score)| {
            let fixed = (score * SCALE).round();
            // Since distance is squared, it can be larger than MAX_SAFE_F * SCALE (i32 range).
            // But we return i64, so it should fit provided dist^2 doesn't exceed i64 max. 
            // Max L2^2 for 16 dims (each max 32k) is roughly 16 * (64k)^2 ~ big number.
            // But we can just cast to i64 safely as long as f32 is finite.
            let safe_i64 = if fixed.is_finite() {
                 fixed as i64 
            } else {
                 i64::MAX // or 0? MAX for distance is safer (worst match)
            };
            (id, safe_i64)
        }).collect())
    }

    pub fn save_snapshot(&mut self, path_override: Option<&std::path::Path>) -> Result<std::path::PathBuf, EngineError> {
        let path = path_override.or(self.snapshot_path.as_deref())
            .ok_or(EngineError::InvalidInput("No snapshot path configured".to_string()))?;
        // 1. Snapshot Components
        let mut k_buf = vec![0u8; 10 * 1024 * 1024]; // 10MB alloc
        let k_len = encode_state(&self.state, &mut k_buf).map_err(EngineError::Kernel)?;
        k_buf.truncate(k_len);
        
        let meta_buf = self.metadata.snapshot();
        let index_buf = self.index.snapshot().map_err(|e| EngineError::InvalidInput(e.to_string()))?;

        // 2. Prepare Header
        // Note: Lengths are updated inside SnapshotManager::save automatically before writing!
        let mut meta = crate::persistence::SnapshotMeta {
            version: 2,
            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            kernel_len: 0, 
            metadata_len: 0,
            index_len: 0,
            index_kind: self.index_kind,
            quant_kind: self.quantization_kind,
            deterministic_build: true, 
            algorithm_params: serde_json::json!({
                "kmeans_iterations": 20,
            }),
        };

        // 3. Delegate to Persistence
        crate::persistence::SnapshotManager::save(
            path,
            &k_buf,
            &meta_buf,
            &mut meta,
            &index_buf
        ).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        
        // 4. Update Cached Hash (Read-back for perfect consistency)
        // Performance: For V1, reading back is fine to ensure correctness of proof.
        // In future, SnapshotManager should return the computed hash.
        let full_bytes = std::fs::read(path).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        self.current_snapshot_hash = Some(snapshot_hash(&full_bytes));

        Ok(path.to_path_buf())
    }

    // Legacy method for API (in-memory). 
    // WARN: Allocates entire snapshot!
    // UPDATED: Prefers serving the last saved snapshot (on disk) if available and matches validation.
    pub fn snapshot(&self) -> Result<Vec<u8>, EngineError> {
        // 1. Try to serve from disk if we have a valid checkpoint
        if let Some(ref path) = self.snapshot_path {
            if path.exists() && self.current_snapshot_hash.is_some() {
                // Return the file derived from save_snapshot
                return std::fs::read(path).map_err(|e| EngineError::InvalidInput(e.to_string()));
            }
        }
        
        // 2. Fallback: Ephemeral Generation (Timestamp 0)
        let tmp_dir = std::env::temp_dir();
        // Deterministic filename to avoid randomness/UUIDs
        let tmp_path = tmp_dir.join("valori_snapshot_ephemeral.bin");
        
        let mut meta = crate::persistence::SnapshotMeta {
            version: 2,
            timestamp: 0,
            kernel_len: 0, 
            metadata_len: 0,
            index_len: 0,
            index_kind: self.index_kind,
            quant_kind: self.quantization_kind,
            deterministic_build: true, 
            algorithm_params: serde_json::Value::Null,
        };
        
        // Encode (Duplicated from save_snapshot mostly, could extract)
        let mut k_buf = vec![0u8; 10 * 1024 * 1024];
        let k_len = encode_state(&self.state, &mut k_buf).map_err(EngineError::Kernel)?;
        k_buf.truncate(k_len);
        let meta_buf = self.metadata.snapshot();
        let index_buf = self.index.snapshot().map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        
        crate::persistence::SnapshotManager::save(
            &tmp_path,
            &k_buf,
            &meta_buf,
            &mut meta,
            &index_buf
        ).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        
        let bytes = std::fs::read(&tmp_path).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        let _ = std::fs::remove_file(tmp_path);
        
        // Note: We do NOT update current_snapshot_hash here because this is ephemeral download, 
        // not "State Checkpointing".
        
        Ok(bytes)
    }

    pub fn restore(&mut self, data: &[u8]) -> Result<(), EngineError> {
        // Cache Input Hash FIRST to match the source
        self.current_snapshot_hash = Some(snapshot_hash(data));

        // Use Persistence Parser
        let (meta, k_data, m_data, i_data) = match crate::persistence::SnapshotManager::parse(data) {
             Ok(res) => res,
             Err(e) => {
                 return Err(EngineError::InvalidInput(format!("Restore failed: {}", e)));
             }
         };

        // Validate Configuration Compatibility
        if meta.index_kind != self.index_kind || meta.quant_kind != self.quantization_kind {
             println!("Snapshot config mismatch. Rebuilding index...");
             return self.restore_from_components(&k_data, &m_data, None);
        }
        
        // Attempt fast restore
        self.restore_from_components(&k_data, &m_data, Some(&i_data))
    }

    /// Restore from snapshot then replay WAL for crash recovery
    /// 
    /// This is the primary recovery method: snapshot + WAL replay = deterministic state
    pub fn restore_with_wal_replay(&mut self, snapshot_data: &[u8], wal_path: &std::path::Path) -> Result<usize, EngineError> {
        // 1. Restore from snapshot
        self.restore(snapshot_data)?;
        
        // 2. Check if WAL exists and has commands
        if !crate::recovery::has_wal(wal_path) {
            tracing::info!("No WAL to replay");
            return Ok(0);
        }
        
        // 3. Replay WAL commands
        tracing::info!("Replaying WAL from {:?}", wal_path);
        let (commands_applied, recovered_hasher) = crate::recovery::replay_wal(
            &mut self.state,
            wal_path
        )?;
        
        // Update Accumulator with recovered state
        self.wal_accumulator = recovered_hasher;
        
        tracing::info!("Replayed {} commands from WAL", commands_applied);
        
        // 4. Rebuild index from updated state (TODO: optimize by applying commands to index directly)
        if commands_applied > 0 {
            tracing::info!("Rebuilding index after WAL replay");
            self.rebuild_index();
        }
        
        Ok(commands_applied)
    }
    
    /// Rebuild index from kernel state
    fn rebuild_index(&mut self) {
        let mut index: Box<dyn VectorIndex + Send + Sync> = match self.index_kind {
              IndexKind::BruteForce => Box::new(BruteForceIndex::new()),
              IndexKind::Hnsw => {
                  use crate::structure::hnsw::HnswIndex;
                  Box::new(HnswIndex::new()) 
              },
              IndexKind::Ivf => {
                  use crate::structure::ivf::{IvfIndex, IvfConfig};
                  Box::new(IvfIndex::new(IvfConfig::default(), D))
              }
         };
         
         for i in 0..MAX_RECORDS {
              let rid = RecordId(i as u32);
              if let Some(record) = self.state.get_record(rid) {
                  let mut vals: Vec<f32> = Vec::with_capacity(D);
                  for fxp in record.vector.data.iter() {
                      let f = fxp.0 as f32 / SCALE;
                      vals.push(f);
                  }
                  index.insert(rid.0, &vals);
              }
         }
         
         self.index = index;
    }

    fn restore_from_components(&mut self, k_data: &[u8], m_data: &[u8], i_data: Option<&[u8]>) -> Result<(), EngineError> {
        // 1. Kernel
        self.state = decode_state::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(k_data).map_err(EngineError::Kernel)?;

        // Rebuild Edge Bitmap
        for i in 0..MAX_EDGES {
             self.edge_bitmap[i] = self.state.is_edge_active(EdgeId(i as u32));
        }

        // 2. Metadata
        if !m_data.is_empty() {
             self.metadata.restore(m_data);
        }

        // 3. Index
        if let Some(blob) = i_data {
             if !blob.is_empty() {
                 println!("Restoring index from snapshot (fast load)...");
                 self.index.restore(blob).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
                 return Ok(());
             }
        }

        // Fallback: Rebuild
        println!("Rebuilding index from kernel...");
        let mut index: Box<dyn VectorIndex + Send + Sync> = match self.index_kind {
              IndexKind::BruteForce => Box::new(BruteForceIndex::new()),
              IndexKind::Hnsw => {
                  use crate::structure::hnsw::HnswIndex;
                  Box::new(HnswIndex::new()) 
              },
              IndexKind::Ivf => {
                  use crate::structure::ivf::{IvfIndex, IvfConfig};
                  Box::new(IvfIndex::new(IvfConfig::default(), D))
              }
         };
         
         for i in 0..MAX_RECORDS {
              let rid = RecordId(i as u32);
              if let Some(record) = self.state.get_record(rid) {
                  let mut vals: Vec<f32> = Vec::with_capacity(D);
                  for fxp in record.vector.data.iter() {
                      // Explicit use of SCALE constant
                      let f = fxp.0 as f32 / SCALE;
                      vals.push(f);
                  }
                  index.insert(rid.0, &vals);
              }
         }
         self.index = index;
         Ok(())
    }

    pub fn get_proof(&self) -> DeterministicProof {
        // Compute Current State Hash
        let final_state_hash = kernel_state_hash(&self.state);
        
        // Derive/Fetch other components
        let snapshot_hash = self.current_snapshot_hash.unwrap_or([0u8; 32]);
        let wal_hash = *self.wal_accumulator.finalize().as_bytes();

        DeterministicProof {
            kernel_version: 1,
            snapshot_hash,
            wal_hash,
            final_state_hash,
        }
    }
}
