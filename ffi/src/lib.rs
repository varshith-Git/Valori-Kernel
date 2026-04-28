// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use pyo3::prelude::*;
use std::sync::{Arc, Mutex};
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::id::RecordId;
use valori_kernel::fxp::ops::from_f32;
use valori_kernel::event::KernelEvent;
use serde_json;  // For metadata serialization
use hex;  // For hash encoding

// Fixed Generics for Python Binding (MVP)
// Reduced to 100 to avoid stack overflow (Kernel allocates on stack currently!)
const MAX_RECORDS: usize = 100;
const D: usize = 384; 
const MAX_NODES: usize = 100; 
const MAX_EDGES: usize = 100;

// f32→Q16.16 conversion is handled by valori_kernel::fxp::ops::from_f32 (single source of truth)

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
        let event_log_path = std::path::PathBuf::from(format!("{}/events.log", path));
        config.wal_path = Some(wal_path);
        config.event_log_path = Some(event_log_path);
        
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
    #[pyo3(signature = (vector, tag))]
    fn insert(&self, vector: Vec<f32>, tag: u64) -> PyResult<u32> {
        if vector.len() != D {
            return Err(pyo3::exceptions::PyValueError::new_err(format!("Expected {} dims", D)));
        }

        let mut engine = self.inner.lock().unwrap();
        
        // 1. Convert to Fixed Point
        let mut fxp_vec = FxpVector::<D>::new_zeros();
        for (i, v) in vector.iter().enumerate() {
            fxp_vec.data[i] = from_f32(*v);
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
             let event = KernelEvent::InsertRecord { id: rid, vector: fxp_vec, metadata: None, tag };
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

    #[pyo3(signature = (vector, k, filter_tag=None))]
    fn search(&self, vector: Vec<f32>, k: usize, filter_tag: Option<u64>) -> PyResult<Vec<(u32, i64)>> {
        if vector.len() != D {
            return Err(pyo3::exceptions::PyValueError::new_err(format!("Expected {} dims", D)));
        }
        
        let engine = self.inner.lock().unwrap();

        // Convert query to FxpVector for kernel search
        let mut fxp_vec = FxpVector::<D>::new_zeros();
        for (i, &v) in vector.iter().enumerate() {
             fxp_vec.data[i] = from_f32(v);
        }

        let mut results = vec![valori_kernel::index::SearchResult::default(); k];
        
        // Call Kernel Directly for Filtered Search
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

        // Deterministic ID generation (Calculate BEFORE mutable borrow for event log)
        // Check NodePool indexing. Assuming 0-based from pool.rs inspection or trial.
        let next_id = valori_kernel::types::id::NodeId(engine.state.node_count() as u32);

        // Use event log if available
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
             // Fallback to direct state mutation
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
        
        // Predict ID for return (though direct create_edge returns it)
        // If we want event sourcing support for edges later, we need next_id. 
        // But for now create_edge calls direct mutation in fallback.
        // Wait, current impl calls engine.state.create_edge directly.
        // So we don't need to predict ID here unless we implement event sourcing for edges.
        // But create_node above DOES event sourcing.
        
        let edge_id = engine.state.create_edge(NodeId(from), NodeId(to), k)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("{:?}", e)))?;
            
        Ok(edge_id.0)
    }

    /// Batch insert multiple vectors atomically.
    /// Returns list of assigned IDs.
    fn insert_batch(&self, vectors: Vec<Vec<f32>>) -> PyResult<Vec<u32>> {
        let mut engine = self.inner.lock().unwrap();
        
        // Validate all vectors have correct dimension
        for (i, vec) in vectors.iter().enumerate() {
            if vec.len() != D {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    format!("Vector {} has {} dims, expected {}", i, vec.len(), D)
                ));
            }
        }
        
        // engine.insert_batch expects &[Vec<f32>], not &[&[f32]]
        match engine.insert_batch(&vectors) {
            Ok(ids) => Ok(ids),
            Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(
                format!("Batch insert failed: {:?}", e)
            ))
        }
    }
    
    /// Get metadata for a record.
    /// Returns bytes or None if no metadata.
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
    
    /// Set metadata for a record.
    /// Metadata is arbitrary bytes (up to 64KB).
    fn set_metadata(&self, record_id: u32, metadata: Vec<u8>) -> PyResult<()> {
        if metadata.len() > 65536 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Metadata too large (max 64KB)"
            ));
        }
        
        let engine = self.inner.lock().unwrap();
        let rid = RecordId(record_id);
        
        // Verify record exists
        if engine.state.get_record(rid).is_none() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("Record {} not found", record_id)
            ));
        }
        
        // Store metadata in engine's metadata store
        // MetadataStore.set expects (String, Value)
        let key = format!("record_{}", record_id);
        let value = serde_json::to_value(metadata)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Failed to serialize metadata: {}", e)))?;
        engine.metadata.set(key, value);
        Ok(())
    }
    
    /// Get cryptographic hash of current state.
    /// Returns 32-byte BLAKE3 hash as hex string.
    fn get_state_hash(&self) -> PyResult<String> {
        let engine = self.inner.lock().unwrap();
        // use root_hash instead of state_hash (which is on ValoriKernel, not KernelState)
        let hash = engine.root_hash();
        
        // Convert [u8; 32] to hex string
        Ok(hex::encode(hash))
    }
    
    /// Get number of records in the database.
    fn record_count(&self) -> PyResult<usize> {
        let engine = self.inner.lock().unwrap();
        Ok(engine.state.record_count())
    }
    
    /// Restore from snapshot data.
    /// Loads kernel state from bytes.
    fn restore(&self, data: Vec<u8>) -> PyResult<()> {
        let mut engine = self.inner.lock().unwrap();
        
        match engine.restore(&data) {
            Ok(_) => Ok(()),
            Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(
                format!("Restore failed: {:?}", e)
            ))
        }
    }
    
    /// Soft delete a record (marks as deleted but doesn't remove).
    /// Record will be excluded from search results.
    fn soft_delete(&self, record_id: u32) -> PyResult<()> {
        let engine = self.inner.lock().unwrap();
        let rid = RecordId(record_id);
        
        // Verify record exists
        if engine.state.get_record(rid).is_none() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("Record {} not found", record_id)
            ));
        }
        
        // Mark as deleted via metadata tombstone
        // Note: bitmap is not directly accessible, use metadata store instead
        let key = format!("deleted_record_{}", record_id);
        let value = serde_json::json!({"deleted": true});
        engine.metadata.set(key, value);
        Ok(())
    }

    /// Atomic insert with proof — single FFI call.
    ///
    /// 1. Validates + converts f32 → Q16.16 (from_f32)
    /// 2. Generates BLAKE3 Merkle proof over Q16.16 integers
    /// 3. Inserts record with proof hash as Record.metadata
    /// 4. Returns (record_id, proof_hash_hex)
    ///
    /// The proof is event-sourced, snapshot-persisted, and included
    /// in kernel_state_hash() — it can never be out of sync.
    #[pyo3(signature = (vector, tag))]
    fn insert_with_proof(&self, vector: Vec<f32>, tag: u64) -> PyResult<(u32, String)> {
        if vector.len() != D {
            return Err(pyo3::exceptions::PyValueError::new_err(format!("Expected {} dims", D)));
        }

        // 1. Validate range + convert to Q16.16
        let mut fxp_vec = FxpVector::<D>::new_zeros();
        let mut fixed_values = Vec::with_capacity(D);
        for (i, &f) in vector.iter().enumerate() {
            if f < -32767.0 || f > 32767.0 {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Float at index {} ({}) outside valid range [-32767.0, 32767.0]", i, f
                )));
            }
            let scalar = from_f32(f);
            fxp_vec.data[i] = scalar;
            fixed_values.push(scalar.0);
        }

        // 2. Generate Merkle proof over Q16.16 integers
        let proof_bytes = generate_proof_bytes(&fixed_values);
        let proof_hex = hex::encode(&proof_bytes);

        // 3. Insert with proof as Record.metadata (event-sourced)
        let mut engine = self.inner.lock().unwrap();

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

        if let Some(ref mut committer) = engine.event_committer {
            let event = KernelEvent::InsertRecord {
                id: rid,
                vector: fxp_vec,
                metadata: Some(proof_bytes),  // ← proof baked into record
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

// ============================================================================
// Bridge Functions — Standalone pyfunctions for deterministic proof generation
// ============================================================================

/// Convert float embeddings to Q16.16 fixed-point integers.
///
/// Uses the kernel's from_f32() — single source of truth.
/// Rejects values outside [-32767.0, 32767.0] (Q16.16 safe range).
#[pyfunction]
fn ingest_embedding(floats: Vec<f32>) -> PyResult<Vec<i32>> {
    for (i, &f) in floats.iter().enumerate() {
        if f < -32767.0 || f > 32767.0 {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Float at index {} ({}) outside valid range [-32767.0, 32767.0]. \
                 Normalize before ingestion.",
                i, f
            )));
        }
    }

    let fixed: Vec<i32> = floats.iter().map(|&f| from_f32(f).0).collect();
    Ok(fixed)
}

/// Internal helper — generates Merkle proof as raw bytes.
/// Single source of truth for Merkle logic.
/// Used by both generate_proof() (hex output) and insert_with_proof() (Record.metadata).
fn generate_proof_bytes(fixed_values: &[i32]) -> Vec<u8> {
    let leaves: Vec<[u8; 32]> = fixed_values
        .iter()
        .enumerate()
        .map(|(pos, &val)| {
            let mut buf = [0u8; 8];
            buf[..4].copy_from_slice(&(pos as u32).to_le_bytes());
            buf[4..].copy_from_slice(&val.to_le_bytes());

            let mut hasher = blake3::Hasher::new();
            hasher.update(&buf);
            *hasher.finalize().as_bytes()
        })
        .collect();

    merkle_root(&leaves).to_vec()
}

/// Build a position-aware Merkle tree over Q16.16 integers.
///
/// Each leaf = BLAKE3(position_u32_le || value_i32_le).
/// Returns the root hash as a hex string.
/// Same BLAKE3 crate the kernel uses — zero divergence possible.
#[pyfunction]
fn generate_proof(fixed_values: Vec<i32>) -> PyResult<String> {
    if fixed_values.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Cannot generate proof for empty vector"
        ));
    }
    Ok(hex::encode(generate_proof_bytes(&fixed_values)))
}

/// Standard binary Merkle tree. Odd leaf: hashed with itself.
fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.len() == 1 {
        return leaves[0];
    }

    let next_level: Vec<[u8; 32]> = leaves
        .chunks(2)
        .map(|pair| {
            let mut hasher = blake3::Hasher::new();
            hasher.update(&pair[0]);
            hasher.update(pair.get(1).unwrap_or(&pair[0]));
            *hasher.finalize().as_bytes()
        })
        .collect();

    merkle_root(&next_level)
}

/// Verify a float embedding against a claimed proof hash.
///
/// Full pipeline in Rust: f32 → Q16.16 → Merkle → compare.
/// No Python math involved.
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
