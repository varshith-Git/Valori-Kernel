// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use pyo3::prelude::*;
use std::sync::{Arc, Mutex};
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::id::RecordId;
use valori_kernel::fxp::ops::from_f32;
use valori_kernel::event::KernelEvent;
use valori_kernel::proof::generate_proof_bytes;
use serde_json;
use hex;

#[pyclass]
struct ValoriEngine {
    inner: Arc<Mutex<Engine>>,
}

#[pymethods]
impl ValoriEngine {
    #[new]
    fn new(path: String) -> PyResult<Self> {
        let mut config = NodeConfig::default();
        let wal_path = std::path::PathBuf::from(format!("{}/wal.log", path));
        let event_log_path = std::path::PathBuf::from(format!("{}/events.log", path));
        config.wal_path = Some(wal_path);
        config.event_log_path = Some(event_log_path);
        
        std::fs::create_dir_all(&path)?;

        let engine = Engine::new(&config);
        
        Ok(ValoriEngine {
            inner: Arc::new(Mutex::new(engine)),
        })
    }

    #[pyo3(signature = (vector, tag))]
    fn insert(&self, vector: Vec<f32>, tag: u64) -> PyResult<u32> {
        let mut engine = self.inner.lock().unwrap();
        
        let mut fxp_data = Vec::with_capacity(vector.len());
        for &v in &vector {
            fxp_data.push(valori_kernel::types::scalar::FxpScalar(from_f32(v).0));
        }
        let fxp_vec = FxpVector { data: fxp_data };
        
        let rid = RecordId(engine.state.record_count() as u32);

        if let Some(ref mut committer) = engine.event_committer {
             let event = KernelEvent::InsertRecord { id: rid, vector: fxp_vec, metadata: None, tag };
             match committer.commit_event(event.clone()) {
                 Ok(_) => {
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

    #[pyo3(signature = (vector, k, filter_tag=None))]
    fn search(&self, vector: Vec<f32>, k: usize, filter_tag: Option<u64>) -> PyResult<Vec<(u32, i64)>> {
        let engine = self.inner.lock().unwrap();

        let mut fxp_data = Vec::with_capacity(vector.len());
        for &v in &vector {
            fxp_data.push(valori_kernel::types::scalar::FxpScalar(from_f32(v).0));
        }
        let fxp_vec = FxpVector { data: fxp_data };

        let mut results = vec![valori_kernel::index::SearchResult::default(); k];
        let count = engine.state.search_l2(&fxp_vec, &mut results, filter_tag);
        
        let mut py_results = Vec::with_capacity(count);
        for i in 0..count {
            let r = results[i];
            py_results.push((r.id.0 as u32, r.score.0 as i64));
        }

        Ok(py_results)
    }
    
    fn save(&mut self) -> PyResult<String> {
         let mut engine = self.inner.lock().unwrap();
         match engine.save_snapshot(None) {
             Ok(path) => Ok(path.to_string_lossy().to_string()),
             Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))
         }
    }

    #[pyo3(signature = (kind, record_id=None))]
    fn create_node(&self, kind: u8, record_id: Option<u32>) -> PyResult<u32> {
        let mut engine = self.inner.lock().unwrap();
        
        let rid = record_id.map(|r| RecordId(r));
        
        use valori_kernel::types::enums::NodeKind;
        let k = NodeKind::from_u8(kind)
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid NodeKind: {}", kind)))?;

        let next_id = valori_kernel::types::id::NodeId(engine.state.node_count() as u32);

        if let Some(ref mut committer) = engine.event_committer {
             let event = KernelEvent::CreateNode { id: next_id, kind: k, record: rid };
             
             match committer.commit_event(event.clone()) {
                 Ok(_) => {
                     engine.apply_committed_event(&event).map_err(|e| {
                         pyo3::exceptions::PyRuntimeError::new_err(format!("Apply failed: {:?}", e))
                     })?;
                     Ok(next_id.0)
                 }
                 Err(e) => return Err(pyo3::exceptions::PyRuntimeError::new_err(format!("Commit failed: {:?}", e))),
             }
        } else {
             let node_id = engine.state.create_node(k, rid)
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))?;
             Ok(node_id.0)
        }
    }

    fn create_edge(&self, from: u32, to: u32, kind: u8) -> PyResult<u32> {
        let mut engine = self.inner.lock().unwrap();
        use valori_kernel::types::id::NodeId;
        use valori_kernel::types::enums::EdgeKind;
        
        let k = EdgeKind::from_u8(kind)
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("Invalid EdgeKind: {}", kind)))?;
        
        let edge_id = engine.state.create_edge(NodeId(from), NodeId(to), k)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))?;
            
        Ok(edge_id.0)
    }

    fn insert_batch(&self, vectors: Vec<Vec<f32>>) -> PyResult<Vec<u32>> {
        let mut engine = self.inner.lock().unwrap();
        match engine.insert_batch(&vectors) {
            Ok(ids) => Ok(ids),
            Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(
                format!("Batch insert failed: {:?}", e)
            ))
        }
    }
    
    fn get_metadata(&self, record_id: u32) -> PyResult<Option<Vec<u8>>> {
        let engine = self.inner.lock().unwrap();
        let rid = RecordId(record_id);
        
        match engine.state.get_record(rid) {
            Some(record) => Ok(record.metadata.clone()),
            None => Err(pyo3::exceptions::PyValueError::new_err(
                format!("Record {} not found", record_id)
            ))
        }
    }
    
    fn set_metadata(&self, record_id: u32, metadata: Vec<u8>) -> PyResult<()> {
        if metadata.len() > 65536 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Metadata too large (max 64KB)"
            ));
        }
        
        let engine = self.inner.lock().unwrap();
        let rid = RecordId(record_id);
        
        if engine.state.get_record(rid).is_none() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("Record {} not found", record_id)
            ));
        }
        
        let key = format!("record_{}", record_id);
        let value = serde_json::to_value(metadata)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Failed to serialize metadata: {}", e)))?;
        engine.metadata.set(key, value);
        Ok(())
    }
    
    fn get_state_hash(&self) -> PyResult<String> {
        let engine = self.inner.lock().unwrap();
        let proof = engine.get_proof();
        Ok(hex::encode(proof.final_state_hash))
    }
    
    fn record_count(&self) -> PyResult<usize> {
        let engine = self.inner.lock().unwrap();
        Ok(engine.state.record_count())
    }
    
    fn restore(&self, data: Vec<u8>) -> PyResult<()> {
        let mut engine = self.inner.lock().unwrap();
        match engine.restore(&data) {
            Ok(_) => Ok(()),
            Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(
                format!("Restore failed: {:?}", e)
            ))
        }
    }
    
    fn soft_delete(&self, record_id: u32) -> PyResult<()> {
        let engine = self.inner.lock().unwrap();
        let rid = RecordId(record_id);
        if engine.state.get_record(rid).is_none() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("Record {} not found", record_id)
            ));
        }
        let key = format!("deleted_record_{}", record_id);
        let value = serde_json::json!({"deleted": true});
        engine.metadata.set(key, value);
        Ok(())
    }

    #[pyo3(signature = (vector, tag))]
    fn insert_with_proof(&self, vector: Vec<f32>, tag: u64) -> PyResult<(u32, String)> {
        let mut fxp_data = Vec::with_capacity(vector.len());
        let mut fixed_values = Vec::with_capacity(vector.len());
        for (i, &f) in vector.iter().enumerate() {
            if f < -32767.0 || f > 32767.0 {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Float at index {} ({}) outside valid range [-32767.0, 32767.0]", i, f
                )));
            }
            let scalar = valori_kernel::types::scalar::FxpScalar(from_f32(f).0);
            fxp_data.push(scalar);
            fixed_values.push(scalar.0);
        }
        let fxp_vec = FxpVector { data: fxp_data };

        let proof_bytes = generate_proof_bytes(&fixed_values);
        let proof_hex = hex::encode(&proof_bytes);

        let mut engine = self.inner.lock().unwrap();
        let rid = RecordId(engine.state.record_count() as u32);

        if let Some(ref mut committer) = engine.event_committer {
            let event = KernelEvent::InsertRecord {
                id: rid,
                vector: fxp_vec,
                metadata: Some(proof_bytes),
                tag,
            };
            match committer.commit_event(event.clone()) {
                Ok(_) => {
                    engine.apply_committed_event(&event).map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(format!("Apply failed: {:?}", e))
                    })?;
                    Ok((rid.0, proof_hex))
                }
                Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(
                    format!("Commit failed: {:?}", e)
                )),
            }
        } else {
            Err(pyo3::exceptions::PyRuntimeError::new_err("Event Log not initialized"))
        }
    }
}

#[pyfunction]
fn ingest_embedding(floats: Vec<f32>) -> PyResult<Vec<i32>> {
    for (i, &f) in floats.iter().enumerate() {
        if f < -32767.0 || f > 32767.0 {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Float at index {} ({}) outside valid range [-32767.0, 32767.0].", i, f
            )));
        }
    }
    let fixed: Vec<i32> = floats.iter().map(|&f| from_f32(f).0).collect();
    Ok(fixed)
}

#[pyfunction]
fn generate_proof(fixed_values: Vec<i32>) -> PyResult<String> {
    if fixed_values.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err("Cannot generate proof for empty vector"));
    }
    Ok(hex::encode(generate_proof_bytes(&fixed_values)))
}

#[pyfunction]
fn verify_embedding(floats: Vec<f32>, claimed_hash: String) -> PyResult<bool> {
    let fixed = ingest_embedding(floats)?;
    let computed_hash = generate_proof(fixed)?;
    Ok(computed_hash == claimed_hash)
}

#[pymodule]
fn valori_ffi(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ValoriEngine>()?;
    m.add_function(wrap_pyfunction!(ingest_embedding, m)?)?;
    m.add_function(wrap_pyfunction!(generate_proof, m)?)?;
    m.add_function(wrap_pyfunction!(verify_embedding, m)?)?;
    Ok(())
}
