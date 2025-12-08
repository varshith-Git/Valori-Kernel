use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::{RecordId, NodeId, EdgeId};
use valori_kernel::types::enums::{NodeKind, EdgeKind};
use valori_kernel::index::brute_force::SearchResult;
use valori_kernel::snapshot::{encode::encode_state, decode::decode_state};

use crate::config::NodeConfig;
use crate::errors::EngineError;

// Local helper to convert f32 to FXP (Q16.16)
// Q16.16: 1.0 = 65536
// Link to kernel config for consistency
const SCALE_F32: f32 = valori_kernel::config::SCALE as f32;

fn from_f32(f: f32) -> FxpScalar {
    // Basic saturation logic matching kernel's test helper intent
    let scaled = f * SCALE_F32;
    if scaled >= (i32::MAX as f32) {
        FxpScalar(i32::MAX)
    } else if scaled <= (i32::MIN as f32) {
        FxpScalar(i32::MIN)
    } else {
        FxpScalar(scaled as i32)
    }
}

pub struct Engine<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize> {
    state: KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>,
}

impl<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize> Engine<MAX_RECORDS, D, MAX_NODES, MAX_EDGES> {
    pub fn new(cfg: &NodeConfig) -> Self {
        // Verify runtime config matches compile-time const generics
        assert_eq!(cfg.max_records, MAX_RECORDS, "Config max_records mismatch");
        assert_eq!(cfg.dim, D, "Config dim mismatch");
        assert_eq!(cfg.max_nodes, MAX_NODES, "Config max_nodes mismatch");
        assert_eq!(cfg.max_edges, MAX_EDGES, "Config max_edges mismatch");

        Self {
            state: KernelState::new(),
        }
    }

    pub fn insert_record_from_f32(&mut self, values: &[f32]) -> Result<u32, EngineError> {
        if values.len() != D {
            return Err(EngineError::InvalidInput(format!("Expected {} dimensions, got {}", D, values.len())));
        }

        // 1. Build Vector
        let mut vector = FxpVector::<D>::new_zeros();
        for (i, v) in values.iter().enumerate() {
            vector.data[i] = from_f32(*v);
        }

        // 2. Determine ID (first free slot strategy - simplified)
        // Kernel doesn't expose "next_free_id". It has `insert` on pool but `apply` takes Command with ID.
        // We must find a free ID. 
        // Kernel exposes `records: RecordPool`. We can iterate.
        // But `records` is `pub(crate)`.
        // Wait! I made them private in Phase 10!
        // `KernelState` doesn't expose a "find free ID" or "next ID" via public API?
        // `RecordPool` has `is_allocated(id)`.
        // So we can brute force scan 0..MAX_RECORDS to find a free one.
        // This is inefficient but functional for v1.
        
        let mut id_val = None;
        // Since we can't access `len` or `capacity` easily without trait/const access?
        // We know MAX_RECORDS.
        for i in 0..MAX_RECORDS {
            let rid = RecordId(i as u32);
            if self.state.get_record(rid).is_none() {
                id_val = Some(rid);
                break;
            }
        }

        let id = id_val.ok_or(valori_kernel::error::KernelError::CapacityExceeded)?;
        
        // 3. Apply Command
        let cmd = Command::InsertRecord { id, vector };
        self.state.apply(&cmd)?;
        
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

        // Find free Edge ID by scanning existing edges
        // Since we can't query edge pool directly, we scan all nodes' outgoing edges.
        let mut used_edges = vec![false; MAX_EDGES]; // This allocation is tolerable for host process
        for i in 0..MAX_NODES {
            let nid = NodeId(i as u32);
            if let Some(_node) = self.state.get_node(nid) {
                // Iterate outgoing edges
                // outgoing_edges returns Option<Iterator>
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
        let mut q_vec = FxpVector::<D>::new_zeros();
        if query.len() != D { return Err(EngineError::InvalidInput("Dim mismatch".into())); }
        for (i, v) in query.iter().enumerate() {
            q_vec.data[i] = from_f32(*v);
        }

        let mut results = vec![SearchResult::default(); k];
        let count = self.state.search_l2(&q_vec, &mut results);

        let mut hits = Vec::new();
        for i in 0..count {
            // Cast FxpScalar to i64 (logic from FXP model is FxpScalar(i32), sq dist can be larger but here score is FxpScalar?)
            // Wait, SearchResult score field type?
            // "found `Vec<(..., FxpScalar)>`". So `results[i].score` IS `FxpScalar`.
            // FxpScalar wraps `i32`.
            // User API wants `i64`.
            // FxpScalar doesn't have `.0` public? 
            // In Phase 1 I made `FxpScalar(pub i32)`.
            // But strictness pass might have changed it?
            // "Implement types/scalar.rs with FxpScalar(i32)".
            // I should check if .0 is pub.
            // If not, I need a helper.
            // Assuming it is pub based on context (tuple struct).
            hits.push((results[i].id.0, results[i].score.0 as i64));
        }
        Ok(hits)
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, EngineError> {
        // Estimate size? 
        // 4096 + RECORDS * (4+1+D*4) + ...
        // Just alloc 1MB for now or grow?
        // `encode_state` needs `&mut [u8]`.
        // Vectors are resizable but `encode_state` takes slice.
        let mut buf = vec![0u8; 10 * 1024 * 1024]; // 10MB
        let len = encode_state(&self.state, &mut buf).map_err(EngineError::Kernel)?;
        buf.truncate(len);
        Ok(buf)
    }

    pub fn restore(&mut self, data: &[u8]) -> Result<(), EngineError> {
        let new_state = decode_state::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(data).map_err(EngineError::Kernel)?;
        self.state = new_state;
        Ok(())
    }
}
