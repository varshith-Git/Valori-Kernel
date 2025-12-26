// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use pyo3::prelude::*;
use std::sync::{Arc, Mutex};
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::RecordId;
use valori_kernel::event::KernelEvent;

// Fixed Generics for Python Binding (MVP)
// Reduced to 100 to avoid stack overflow (Kernel allocates on stack currently!)
const MAX_RECORDS: usize = 100;
const D: usize = 384; 
const MAX_NODES: usize = 100; 
const MAX_EDGES: usize = 100;

const SCALE: f32 = 65536.0;

#[pyclass]
struct ValoriEngine {
    inner: Arc<Mutex<Engine<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>>>,
}

#[pymethods]
impl ValoriEngine {
    #[new]
    fn new(path: String) -> PyResult<Self> {
        let mut config = NodeConfig::default();
        let wal_path = std::path::PathBuf::from(format!("{}/wal.log", path));
        config.wal_path = Some(wal_path);
        
        // Ensure consistent configuration constants
        config.max_records = MAX_RECORDS;
        config.dim = D;
        config.max_nodes = MAX_NODES;
        config.max_edges = MAX_EDGES;
        
        std::fs::create_dir_all(&path)?;

        let engine = Engine::<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>::new(&config);
        
        Ok(ValoriEngine {
            inner: Arc::new(Mutex::new(engine)),
        })
    }

    /// Insert a record. Returns the assigned ID.
    /// Valori Kernel enforces dense ID packing (first free slot).
    fn insert(&self, vector: Vec<f32>) -> PyResult<u32> {
        if vector.len() != D {
            return Err(pyo3::exceptions::PyValueError::new_err(format!("Expected {} dims", D)));
        }

        let mut engine = self.inner.lock().unwrap();
        
        // 1. Convert to Fixed Point
        let mut fxp_vec = FxpVector::<D>::new_zeros();
        for (i, v) in vector.iter().enumerate() {
            let fixed = (v * SCALE).round().clamp(i32::MIN as f32, i32::MAX as f32) as i32;
            fxp_vec.data[i] = FxpScalar(fixed);
        }
        
        // 2. Determine ID (first free slot) - Must match Kernel's deterministic logic
        // We use engine.state to find the free slot (same as Engine logic)
        let mut id_val = None;
        for i in 0..MAX_RECORDS {
            let rid = RecordId(i as u32);
            if engine.state.get_record(rid).is_none() {
                id_val = Some(rid);
                break;
            }
        }
        
        let rid = id_val.ok_or_else(|| {
             pyo3::exceptions::PyRuntimeError::new_err("Capacity Exceeded")
        })?;

        // 3. Commit via Event Log (Preferred) or WAL (Fallback)
        if let Some(ref mut committer) = engine.event_committer {
             let event = KernelEvent::InsertRecord { id: rid, vector: fxp_vec };
             match committer.commit_event(event.clone()) {
                 Ok(_) => {
                     // Sync Engine State
                     engine.apply_committed_event(&event).map_err(|e| {
                         pyo3::exceptions::PyRuntimeError::new_err(format!("Apply failed: {:?}", e))
                     })?;
                     Ok(rid.0)
                 }
                 Err(e) => {
                     Err(pyo3::exceptions::PyRuntimeError::new_err(format!("Commit failed: {:?}", e)))
                 }
             }
        } else {
             Err(pyo3::exceptions::PyRuntimeError::new_err("Event Log not initialized"))
        }
    }

    fn search(&self, vector: Vec<f32>, k: usize) -> PyResult<Vec<(u32, i64)>> {
         if vector.len() != D {
            return Err(pyo3::exceptions::PyValueError::new_err(format!("Expected {} dims", D)));
        }
        
        let engine = self.inner.lock().unwrap();
        match engine.search_l2(&vector, k) {
            Ok(results) => Ok(results),
            Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))
        }
    }
    
    fn save(&mut self) -> PyResult<String> {
         let mut engine = self.inner.lock().unwrap();
         match engine.save_snapshot(None) {
             Ok(path) => Ok(path.to_string_lossy().to_string()),
             Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))
         }
    }
}

#[pymodule]
fn valori_ffi(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ValoriEngine>()?;
    Ok(())
}
