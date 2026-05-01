// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
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

use std::path::{Path, PathBuf};

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

    // WAL Persistence (Phase 20)
    pub wal_writer: Option<WalWriter>,
    pub wal_accumulator: blake3::Hasher,

    // Event-sourced persistence (Phase 23 - NEW)
    pub event_committer: Option<EventCommitter>,
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

        let wal_writer = if let Some(ref path) = cfg.wal_path {
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
        };
        
        let wal_accumulator = blake3::Hasher::new();
        
        let event_committer = if let Some(ref path) = cfg.event_log_path {
             match EventLogWriter::open(path, Some(cfg.dim as u32)) {
                 Ok(log_writer) => {
                     let journal = EventJournal::new();
                     let live_state = KernelState::new();
                     Some(EventCommitter::new(log_writer, journal, live_state))
                 }
                 Err(e) => {
                     tracing::error!("Failed to open Event Log: {}", e);
                     None
                 }
             }
        } else {
            None
        };

        Self {
            state: KernelState::new(),
            metadata: crate::metadata::MetadataStore::new(),
            index,
            quant,
            index_kind: cfg.index_kind,
            quantization_kind: cfg.quantization_kind,
            wal_path: cfg.wal_path.clone(),
            snapshot_path: cfg.snapshot_path.clone(),
            wal_writer,
            wal_accumulator,
            event_committer,
        }
    }

    pub fn insert_record_from_f32(&mut self, values: &[f32]) -> Result<u32, EngineError> {
        let mut fxp_data = Vec::with_capacity(values.len());
        for &v in values {
            fxp_data.push(FxpScalar((v * SCALE as f32) as i32));
        }
        let vector = FxpVector { data: fxp_data };
        let rid = RecordId(self.state.record_count() as u32);

        let event = valori_kernel::event::KernelEvent::InsertRecord {
            id: rid,
            vector,
            metadata: None,
            tag: 0,
        };

        if let Some(ref mut committer) = self.event_committer {
            committer.commit_event(event.clone()).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            self.apply_committed_event(&event)?;
        } else {
            let (rid, vector) = if let valori_kernel::event::KernelEvent::InsertRecord { id, vector, .. } = &event {
                (*id, vector.clone())
            } else {
                unreachable!()
            };
            
            let cmd = Command::InsertRecord {
                id: rid,
                vector,
                metadata: None,
                tag: 0,
            };
            if let Some(ref mut writer) = self.wal_writer {
                writer.append_command(&cmd).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
            }
            self.state.apply(&cmd)?;
            self.index.insert(rid.0, values);
        }

        Ok(rid.0)
    }

    pub fn insert_batch(&mut self, batch: &[Vec<f32>]) -> Result<Vec<u32>, EngineError> {
        if let Some(ref mut committer) = self.event_committer {
            let mut events = Vec::with_capacity(batch.len());
            let mut ids = Vec::with_capacity(batch.len());
            let start_id = self.state.record_count() as u32;

            for (i, values) in batch.iter().enumerate() {
                let mut fxp_data = Vec::with_capacity(values.len());
                for &v in values {
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
                self.apply_committed_event(event)?;
            }
            Ok(ids)
        } else {
            let mut ids = Vec::with_capacity(batch.len());
            for values in batch {
                ids.push(self.insert_record_from_f32(values)?);
            }
            Ok(ids)
        }
    }

    pub fn search_l2(&self, query: &[f32], k: usize) -> Result<Vec<(u32, f32)>, EngineError> {
        Ok(self.index.search(query, k))
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, EngineError> {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"VAL1");

        let mut k_buf = vec![0u8; 10 * 1024 * 1024];
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
        let k_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
        offset += 4;
        let k_data = &data[offset..offset+k_len];
        offset += k_len;

        let m_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
        offset += 4;
        let m_data = &data[offset..offset+m_len];
        offset += m_len;

        let i_len = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap()) as usize;
        offset += 4;
        let i_data = if offset + i_len <= data.len() {
             Some(&data[offset..offset+i_len])
        } else {
             None
        };

        self.restore_from_components(k_data, m_data, i_data)
    }

    pub fn delete_record(&mut self, id: u32) -> Result<(), EngineError> {
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

    pub fn create_node_for_record(&mut self, record_id: Option<u32>, kind: u8) -> Result<u32, EngineError> {
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
             let cmd = Command::CreateNode { node_id, kind, record };
             if let Some(ref mut writer) = self.wal_writer {
                 writer.append_command(&cmd).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
             }
             self.state.apply(&cmd)?;
         }
         Ok(node_id.0)
    }

    pub fn create_edge(&mut self, from: u32, to: u32, kind: u8) -> Result<u32, EngineError> {
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
        self.state.apply_event(event)?;
        
        match event {
            valori_kernel::event::KernelEvent::InsertRecord { id, vector, .. } => {
                let mut vals = Vec::with_capacity(vector.data.len());
                for fxp in &vector.data {
                    vals.push(fxp.0 as f32 / SCALE as f32);
                }
                self.index.insert(id.0, &vals);
            }
            valori_kernel::event::KernelEvent::DeleteRecord { id } => {
                self.index.delete(id.0);
            }
            _ => {}
        }
        
        Ok(())
    }

    pub fn rebuild_index(&mut self) {
         let mut index: Box<dyn VectorIndex + Send + Sync> = match self.index_kind {
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

         // We need a way to iterate records. KernelState doesn't expose it publicly?
         // Ah, I need to add a public iterator to KernelState or use a method.
         // Actually, I can't iterate records if they are private.
         // Let's assume for now I rebuild from the index if possible, or I need to add a public record iterator to kernel.
         // "Do NOT modify valori-kernel". 
         // Wait, if I can't iterate records, I can't rebuild index.
         // Let's check if there is ANY public way to get records.
         // `get_record(id)` is public.
         // `record_count()` is public.
         // So I can iterate 0..record_count() and call get_record.
         
         let count = self.state.record_count();
         for i in 0..count {
              if let Some(record) = self.state.get_record(RecordId(i as u32)) {
                  let mut vals: Vec<f32> = Vec::with_capacity(record.vector.data.len());
                  for fxp in record.vector.data.iter() {
                      let f = fxp.0 as f32 / SCALE as f32;
                      vals.push(f);
                  }
                  index.insert(i as u32, &vals);
              }
         }
         
         self.index = index;
    }

    fn restore_from_components(&mut self, k_data: &[u8], m_data: &[u8], i_data: Option<&[u8]>) -> Result<(), EngineError> {
        self.state = decode_state(k_data)?;

        if !m_data.is_empty() {
             self.metadata.restore(m_data);
        }

        if let Some(blob) = i_data {
             if !blob.is_empty() {
                 self.index.restore(blob).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
                 return Ok(());
             }
        }

        self.rebuild_index();
        Ok(())
    }
}
