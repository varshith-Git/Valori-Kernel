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
struct ValoricoreEngine {
    inner: Arc<Mutex<Engine>>,
    path: String,
}

#[pymethods]
impl ValoricoreEngine {
    #[new]
    fn new(path: String) -> PyResult<Self> {
        let mut config = NodeConfig::default();
        let wal_path = std::path::PathBuf::from(format!("{}/wal.log", path));
        let event_log_path = std::path::PathBuf::from(format!("{}/events.log", path));
        config.wal_path = Some(wal_path);
        config.event_log_path = Some(event_log_path);
        
        std::fs::create_dir_all(&path)?;

        let engine = Engine::new(&config);
        
        Ok(ValoricoreEngine {
            inner: Arc::new(Mutex::new(engine)),
            path,
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

    #[pyo3(signature = (node_id))]
    fn get_node(&self, node_id: u32) -> PyResult<Option<(u8, Option<u32>)>> {
        let engine = self.inner.lock().unwrap();
        use valori_kernel::types::id::NodeId;
        
        match engine.state.get_node(NodeId(node_id)) {
            Some(n) => {
                let rec = n.record.map(|r| r.0);
                Ok(Some((n.kind as u8, rec)))
            },
            None => Ok(None)
        }
    }

    #[pyo3(signature = (node_id))]
    fn get_edges(&self, node_id: u32) -> PyResult<Vec<(u32, u32, u8)>> {
        let engine = self.inner.lock().unwrap();
        use valori_kernel::types::id::NodeId;
        
        let mut py_edges = Vec::new();
        
        if let Some(iter) = engine.state.outgoing_edges(NodeId(node_id)) {
            for edge in iter {
                py_edges.push((edge.id.0, edge.to.0, edge.kind as u8));
            }
        }
        
        Ok(py_edges)
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

    #[pyo3(signature = (vectors, tags))]
    fn insert_batch_with_proof(&self, vectors: Vec<Vec<f32>>, tags: Vec<u64>) -> PyResult<Vec<(u32, String)>> {
        if vectors.len() != tags.len() {
            return Err(pyo3::exceptions::PyValueError::new_err("Vectors and tags must have the same length"));
        }

        let mut results = Vec::with_capacity(vectors.len());
        let mut engine = self.inner.lock().unwrap();
        
        for (i, vector) in vectors.iter().enumerate() {
            let mut fxp_data = Vec::with_capacity(vector.len());
            let mut fixed_values = Vec::with_capacity(vector.len());
            for &f in vector {
                let scalar = valori_kernel::types::scalar::FxpScalar(from_f32(f).0);
                fxp_data.push(scalar);
                fixed_values.push(scalar.0);
            }
            let fxp_vec = FxpVector { data: fxp_data };
            let proof_bytes = generate_proof_bytes(&fixed_values);
            let proof_hex = hex::encode(&proof_bytes);

            let rid = RecordId(engine.state.record_count() as u32);
            let tag = tags[i];

            if let Some(ref mut committer) = engine.event_committer {
                let event = KernelEvent::InsertRecord {
                    id: rid,
                    vector: fxp_vec,
                    metadata: Some(proof_bytes),
                    tag,
                };
                committer.commit_event(event.clone()).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!("Commit failed: {:?}", e))
                })?;
                engine.apply_committed_event(&event).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!("Apply failed: {:?}", e))
                })?;
                results.push((rid.0, proof_hex));
            } else {
                return Err(pyo3::exceptions::PyRuntimeError::new_err("Event Log not initialized"));
            }
        }
        
        Ok(results)
    }
    
    fn get_metadata(&self, record_id: u32) -> PyResult<Option<Vec<u8>>> {
        let engine = self.inner.lock().unwrap();
        let rid = RecordId(record_id);
        
        // 1. Check MetadataStore (high-level metadata)
        let key = format!("record_{}", record_id);
        if let Some(val) = engine.metadata.get(&key) {
             // Deserialize from JSON back to Vec<u8>
             if let Ok(vec) = serde_json::from_value::<Vec<u8>>(val) {
                 return Ok(Some(vec));
             }
        }

        // 2. Fallback to Record-level metadata (proofs)
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

    fn snapshot(&self) -> PyResult<Vec<u8>> {
        let engine = self.inner.lock().unwrap();
        match engine.snapshot() {
            Ok(data) => Ok(data),
            Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(
                format!("Snapshot failed: {:?}", e)
            ))
        }
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
    
    fn get_timeline(&self) -> PyResult<Vec<String>> {
        let log_path = format!("{}/events.log", self.path);
        if !std::path::Path::new(&log_path).exists() {
            return Ok(Vec::new());
        }

        let mut file = std::fs::File::open(&log_path)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(format!("Could not open events.log: {}", e)))?;
            
        use std::io::Read;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(format!("Could not read events.log: {}", e)))?;

        if bytes.len() < 16 {
            return Ok(Vec::new());
        }

        use valori_node::events::event_log::LogEntry;
        let mut events = Vec::new();
        let mut offset = 16;
        let mut event_id = 0;

        while offset < bytes.len() {
            match bincode::serde::decode_from_slice::<LogEntry, _>(
                &bytes[offset..],
                bincode::config::standard()
            ) {
                Ok((entry, bytes_read)) => {
                    let event_str = match entry {
                        LogEntry::Event(e) => match e {
                            KernelEvent::InsertRecord { id, tag, .. } => format!("Event ID {}: InsertRecord (Record {}, Tag: {})", event_id, id.0, tag),
                            KernelEvent::DeleteRecord { id } => format!("Event ID {}: DeleteRecord (Record {})", event_id, id.0),
                            KernelEvent::CreateNode { id, kind, .. } => format!("Event ID {}: CreateNode (Node {}, Kind: {:?})", event_id, id.0, kind),
                            KernelEvent::CreateEdge { id, from, to, kind } => format!("Event ID {}: CreateEdge (Edge {}, {:?} -> {:?}, Kind: {:?})", event_id, id.0, from, to, kind),
                            KernelEvent::DeleteEdge { id } => format!("Event ID {}: DeleteEdge (Edge {})", event_id, id.0),
                        },
                        LogEntry::Checkpoint { event_count, .. } => format!("Event ID {}: Checkpoint (Event Count {})", event_id, event_count),
                    };
                    events.push(event_str);
                    offset += bytes_read;
                    event_id += 1;
                }
                Err(e) => {
                    events.push(format!("Decoding stopped at offset {} due to error: {:?}", offset, e));
                    break;
                }
            }
        }
        
        Ok(events)
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
fn valoricore_ffi(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ValoricoreEngine>()?;
    m.add_function(wrap_pyfunction!(ingest_embedding, m)?)?;
    m.add_function(wrap_pyfunction!(generate_proof, m)?)?;
    m.add_function(wrap_pyfunction!(verify_embedding, m)?)?;
    Ok(())
}
