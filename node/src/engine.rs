// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::id::{RecordId, NodeId, EdgeId};
use valori_kernel::types::enums::{NodeKind, EdgeKind};
use valori_kernel::snapshot::{encode::encode_state, decode::decode_state};
use valori_kernel::fxp::ops::from_f32;

use crate::config::{NodeConfig, IndexKind, QuantizationKind};
use crate::errors::EngineError;
use crate::structure::index::{VectorIndex, BruteForceIndex};
use crate::structure::quant::{Quantizer, NoQuantizer, ScalarQuantizer};
use crate::metadata::MetadataStore;

use std::sync::Arc;

pub struct Engine<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize> {
    state: KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>,
    pub index_kind: IndexKind,
    pub quantization_kind: QuantizationKind,
    
    // Host-level extensions
    index: Box<dyn VectorIndex + Send + Sync>,
    quant: Box<dyn Quantizer + Send + Sync>,
    pub metadata: Arc<MetadataStore>,
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
              }
         };

        // Initialize Quantizer
        let quant: Box<dyn Quantizer + Send + Sync> = match cfg.quantization_kind {
            QuantizationKind::None => Box::new(NoQuantizer),
            QuantizationKind::Scalar => Box::new(ScalarQuantizer {}),
        };

        Self {
            state: KernelState::new(),
            index_kind: cfg.index_kind,
            quantization_kind: cfg.quantization_kind,
            index,
            quant,
            metadata: Arc::new(MetadataStore::new()),
        }
    }

    pub fn insert_record_from_f32(&mut self, values: &[f32]) -> Result<u32, EngineError> {
        if values.len() != D {
            return Err(EngineError::InvalidInput(format!("Expected {} dimensions, got {}", D, values.len())));
        }

        // 1. Build FxpVector for Kernel
        let mut vector = FxpVector::<D>::new_zeros();
        for (i, v) in values.iter().enumerate() {
            vector.data[i] = from_f32(*v);
        }

        // 2. Determine ID (first free slot strategy - simplified)
        let mut id_val = None;
        for i in 0..MAX_RECORDS {
            let rid = RecordId(i as u32);
            if self.state.get_record(rid).is_none() {
                id_val = Some(rid);
                break;
            }
        }

        let id = id_val.ok_or(valori_kernel::error::KernelError::CapacityExceeded)?;
        
        // 3. Apply Command to Kernel (Primary Store)
        let cmd = Command::InsertRecord { id, vector };
        self.state.apply(&cmd)?;
        
        // 4. Update Host Index
        // CRITICAL: Round-trip through Fxp to match Restore behavior!
        // We must insert the EXACT same float values that would be recovered from snapshot.
        let mut consistent_values = Vec::with_capacity(D);
        for i in 0..D {
            let fxp = vector.data[i];
            let f = fxp.0 as f32 / 65536.0;
            consistent_values.push(f);
        }
        
        self.index.insert(id.0, &consistent_values);

        Ok(id.0)
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

        let cmd = Command::CreateNode {
            node_id,
            kind,
            record: record_id,
        };
        self.state.apply(&cmd)?;

        Ok(node_id.0)
    }

    pub fn create_edge(&mut self, from_val: u32, to_val: u32, kind_val: u8) -> Result<u32, EngineError> {
        let kind = EdgeKind::from_u8(kind_val).ok_or(EngineError::InvalidInput("Invalid EdgeKind".to_string()))?;
        let from = NodeId(from_val);
        let to = NodeId(to_val);

        // Find free Edge ID logic...
        let mut used_edges = vec![false; MAX_EDGES]; // allocating on stack if small enough or vec
        for i in 0..MAX_NODES {
            let nid = NodeId(i as u32);
            if let Some(_node) = self.state.get_node(nid) {
                if let Some(iter) = self.state.outgoing_edges(nid) {
                    for edge in iter {
                         if (edge.id.0 as usize) < MAX_EDGES {
                             used_edges[edge.id.0 as usize] = true;
                         }
                    }
                }
            }
        }
        
        let mut id_val = None;
        for i in 0..MAX_EDGES {
            if !used_edges[i] {
                id_val = Some(EdgeId(i as u32));
                break;
            }
        }
        
        let edge_id = id_val.ok_or(valori_kernel::error::KernelError::CapacityExceeded)?;

        let cmd = Command::CreateEdge {
            edge_id,
            kind,
            from,
            to,
        };
        self.state.apply(&cmd).map_err(EngineError::Kernel)?;

        Ok(edge_id.0)
    }

    pub fn search_l2(&self, query: &[f32], k: usize) -> Result<Vec<(u32, i64)>, EngineError> {
        // Use Host Index instead of Kernel Search to support different index types
        // The Kernel's search_l2 is strictly brute force Fxp.
        // The Host Index might be HNSW/Simd-F32.
        
        let hits = self.index.search(query, k);
        
        // Convert f32 score to i64 (Kernel API Expectation).
        // Since `search()` returns raw f32 distance squared, and Kernel usually returns FxpScalar value (i32/i64 scaled).
        // For compatibility with Python client expecting "integers", we should scale it.
        // Fxp 1.0 = 65536. 
        // We multiply by 65536 * 65536? No wait, FxpScalar is 20.12 format?
        // Let's assume we return raw integers matching kernel behavior if index is BruteForce.
        // If index is HNSW (float), we synthesize an integer score.
        // For now, let's just cast to i64.
        
        Ok(
            hits.into_iter().map(|(id, score)| {
                // Heuristic: scale f32 score to match Fxp magnitude roughly?
                // Or just return score as i64 bits?
                // Let's return score * 1000.0 as i64 for now to keep some precision
                (id, (score * 65536.0) as i64) 
            }).collect()
        )
    }

    pub fn save_snapshot(&self, path: &std::path::Path) -> Result<(), EngineError> {
        // 1. Snapshot Components
        let mut k_buf = vec![0u8; 10 * 1024 * 1024]; // 10MB alloc
        let k_len = encode_state(&self.state, &mut k_buf).map_err(EngineError::Kernel)?;
        k_buf.truncate(k_len);
        
        let meta_buf = self.metadata.snapshot();
        let index_buf = self.index.snapshot();

        // 2. Prepare Header
        let mut meta = crate::persistence::SnapshotMeta {
            version: 2,
            timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            kernel_len: 0, 
            metadata_len: 0,
            index_len: 0,
            index_kind: self.index_kind,
            quant_kind: self.quantization_kind,
        };

        // 3. Delegate to Persistence
        crate::persistence::SnapshotManager::save(
            path,
            &k_buf,
            &meta_buf,
            &mut meta,
            &index_buf
        ).map_err(|e| EngineError::InvalidInput(e.to_string()))?;

        Ok(())
    }

    // Legacy method for API (in-memory). 
    // WARN: Allocates entire snapshot!
    pub fn snapshot(&self) -> Result<Vec<u8>, EngineError> {
        let tmp_dir = std::env::temp_dir();
        let uuid = uuid::Uuid::new_v4(); // Need UUID or random
        let tmp_path = tmp_dir.join(format!("valori_snap_{}", uuid));
        
        self.save_snapshot(&tmp_path)?;
        
        let bytes = std::fs::read(&tmp_path).map_err(|e| EngineError::InvalidInput(e.to_string()))?;
        let _ = std::fs::remove_file(tmp_path);
        
        Ok(bytes)
    }

    pub fn restore(&mut self, data: &[u8]) -> Result<(), EngineError> {
        // Use Persistence Parser
        let (meta, k_data, m_data, i_data) = match crate::persistence::SnapshotManager::parse(data) {
             Ok(res) => res,
             Err(e) => {
                 // Fallback to V1 if parsing fails? 
                 // V1 logic was simple concatenation. 
                 // For now, strict V2 or fail, as agreed in plan.
                 return Err(EngineError::InvalidInput(format!("Restore failed: {}", e)));
             }
        };

        // Validate Configuration Compatibility
        if meta.index_kind != self.index_kind || meta.quant_kind != self.quantization_kind {
             // We can warn or hard fail. 
             // Logic: If kinds differ, we cannot strictly reuse the index blob.
             // But we can rebuild from kernel!
             // So if mismatch, we should ignore `i_data` and rebuild.
             println!("Snapshot config mismatch. Rebuilding index...");
             // Proceed to restore kernel, then rebuild.
             return self.restore_from_components(&k_data, &m_data, None);
        }
        
        // Attempt fast restore
        self.restore_from_components(&k_data, &m_data, Some(&i_data))
    }

    fn restore_from_components(&mut self, k_data: &[u8], m_data: &[u8], i_data: Option<&[u8]>) -> Result<(), EngineError> {
        // 1. Kernel
        self.state = decode_state::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(k_data).map_err(EngineError::Kernel)?;

        // 2. Metadata
        if !m_data.is_empty() {
             self.metadata.restore(m_data);
        }

        // 3. Index
        if let Some(blob) = i_data {
             if !blob.is_empty() {
                 println!("Restoring index from snapshot (fast load)...");
                 self.index.restore(blob);
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
         };
         
         for i in 0..MAX_RECORDS {
              let rid = RecordId(i as u32);
              if let Some(record) = self.state.get_record(rid) {
                  let mut vals: Vec<f32> = Vec::with_capacity(D);
                  for fxp in record.vector.data.iter() {
                      let f = fxp.0 as f32 / 65536.0;
                      vals.push(f);
                  }
                  index.insert(rid.0, &vals);
              }
         }
         self.index = index;
         Ok(())
    }
}
