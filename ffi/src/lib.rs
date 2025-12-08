use pyo3::prelude::*;
use std::sync::Mutex;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::state::command::Command;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::RecordId;
use valori_kernel::index::SearchResult;
use valori_kernel::snapshot::{encode::encode_state, decode::decode_state};

// Constants for KernelState
const MAX_RECORDS: usize = 1024;
const D: usize = 16;
const MAX_NODES: usize = 1024;
const MAX_EDGES: usize = 2048;

// Helper to convert f32 to FXP using kernel config
fn from_f32(f: f32) -> FxpScalar {
    // Use valori-kernel's SCALE constant so this never diverges
    let scale = valori_kernel::config::SCALE as f32;
    let scaled = f * scale;
    if scaled >= (i32::MAX as f32) {
        FxpScalar(i32::MAX)
    } else if scaled <= (i32::MIN as f32) {
        FxpScalar(i32::MIN)
    } else {
        FxpScalar(scaled as i32)
    }
}

#[pyclass]
struct PyKernel {
    inner: Mutex<KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>>,
}

#[pymethods]
impl PyKernel {
    #[new]
    fn new() -> Self {
        PyKernel {
            inner: Mutex::new(KernelState::new()),
        }
    }

    fn insert(&self, vector: Vec<f32>) -> PyResult<u32> {
        if vector.len() != D {
            return Err(pyo3::exceptions::PyValueError::new_err(format!("Expected {} dims", D)));
        }
        
        // Use generic parameter syntax for FxpVector
        let mut fxp_vec = FxpVector::<D>::new_zeros();
        for (i, v) in vector.iter().enumerate() {
            fxp_vec.data[i] = from_f32(*v);
        }

        let mut state = self.inner.lock().unwrap();

        // Find first free id (same strategy as valori-node Engine)
        let mut free_id: Option<RecordId> = None;
        for i in 0..MAX_RECORDS {
            let rid = RecordId(i as u32);
            if state.get_record(rid).is_none() {
                free_id = Some(rid);
                break;
            }
        }

        let id = free_id.ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("Capacity exceeded in kernel")
        })?;

        let cmd = Command::InsertRecord {
            id,
            vector: fxp_vec,
        };

        state.apply(&cmd).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))?;
        
        Ok(id.0)
    }

    fn search(&self, query: Vec<f32>, k: usize) -> PyResult<Vec<(u32, i64)>> {
        if query.len() != D {
             return Err(pyo3::exceptions::PyValueError::new_err(format!("Expected {} dims", D)));
        }

        let mut q_vec = FxpVector::<D>::new_zeros();
        for (i, v) in query.iter().enumerate() {
            q_vec.data[i] = from_f32(*v);
        }

        let mut results = vec![SearchResult::default(); k];
        
        let state = self.inner.lock().unwrap();
        let count = state.search_l2(&q_vec, &mut results);
        
        let mut hits = Vec::with_capacity(count);
        for i in 0..count {
             hits.push((results[i].id.0, results[i].score.0 as i64));
        }

        Ok(hits)
    }

    fn snapshot(&self) -> PyResult<Vec<u8>> {
        let state = self.inner.lock().unwrap();
        let mut buf = vec![0u8; 10 * 1024 * 1024]; // 10MB approx
        let len = encode_state(&state, &mut buf).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))?;
        buf.truncate(len);
        Ok(buf)
    }

    fn restore(&self, data: &[u8]) -> PyResult<()> {
        let new_state = decode_state::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>(data)
             .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))?;
        
        let mut state = self.inner.lock().unwrap();
        *state = new_state;
        Ok(())
    }

    #[pyo3(signature = (kind, record_id=None))]
    fn create_node(&self, kind: u8, record_id: Option<u32>) -> PyResult<u32> {
        use valori_kernel::types::enums::NodeKind;
        use valori_kernel::types::id::NodeId;

        let node_kind = NodeKind::from_u8(kind)
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid NodeKind: {}", kind)))?;
        
        let rid = record_id.map(RecordId);

        let mut state = self.inner.lock().unwrap();

        // Find free NodeId
        let mut free_id: Option<NodeId> = None;
        for i in 0..MAX_NODES {
            let nid = NodeId(i as u32);
            if state.get_node(nid).is_none() {
                free_id = Some(nid);
                break;
            }
        }
        let node_id = free_id.ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("Node capacity exceeded")
        })?;

        let cmd = Command::CreateNode {
            node_id,
            kind: node_kind,
            record: rid,
        };

        state.apply(&cmd).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))?;
        Ok(node_id.0)
    }

    fn create_edge(&self, from_id: u32, to_id: u32, kind: u8) -> PyResult<u32> {
        use valori_kernel::types::enums::EdgeKind;
        use valori_kernel::types::id::{NodeId, EdgeId};

        let edge_kind = EdgeKind::from_u8(kind)
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid EdgeKind: {}", kind)))?;

        let mut state = self.inner.lock().unwrap();

        // Scan for free EdgeID (KernelState doesn't expose public EdgePool access)
        let mut used_edges = vec![false; MAX_EDGES];
        for i in 0..MAX_NODES {
            let nid = NodeId(i as u32);
            if let Some(iter) = state.outgoing_edges(nid) {
                for edge in iter {
                     if (edge.id.0 as usize) < MAX_EDGES {
                         used_edges[edge.id.0 as usize] = true;
                     }
                }
            }
        }

        let mut free_id: Option<EdgeId> = None;
        for i in 0..MAX_EDGES {
             if !used_edges[i] {
                 free_id = Some(EdgeId(i as u32));
                 break;
             }
        }
        let edge_id = free_id.ok_or_else(|| {
             pyo3::exceptions::PyRuntimeError::new_err("Edge capacity exceeded")
        })?;

        let cmd = Command::CreateEdge {
            edge_id,
            kind: edge_kind,
            from: NodeId(from_id),
            to: NodeId(to_id),
        };

        state.apply(&cmd).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))?;
        Ok(edge_id.0)
    }
}

/// A Python module implemented in Rust.
#[pymodule]
fn valori_ffi(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyKernel>()?;
    Ok(())
}
