// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use pyo3::prelude::*;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
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

/// Acquire the engine lock, returning a Python error if the mutex is poisoned
/// (which happens when a prior call panicked while holding the lock).
macro_rules! lock_engine {
    ($self:expr) => {
        $self.inner.lock().map_err(|_| {
            PyRuntimeError::new_err("engine mutex poisoned by a prior panic; restart the process")
        })?
    };
}

#[pyclass]
struct ValoricoreEngine {
    inner: Arc<Mutex<Engine>>,
}

#[pymethods]
impl ValoricoreEngine {
    #[new]
    #[pyo3(signature = (path, index_kind = "bruteforce"))]
    fn new(path: String, index_kind: &str) -> PyResult<Self> {
        // M-4: build a clean config rather than NodeConfig::default(), which reads all
        // VALORI_* env vars and may inadvertently pick up auth tokens, S3 credentials,
        // or embed provider settings from the surrounding process.
        let mut config = NodeConfig::default();
        // Null out server-mode-only fields so they have no effect in the embedded SDK.
        config.auth_token = None;
        config.keys_path = None;
        config.object_store_url = None;
        config.embed_provider = None;
        config.cors_origin = None;

        let wal_path = std::path::PathBuf::from(format!("{}/wal.log", path));
        let event_log_path = std::path::PathBuf::from(format!("{}/events.log", path));
        config.wal_path = Some(wal_path);
        config.event_log_path = Some(event_log_path);

        use valori_node::config::IndexKind;
        config.index_kind = match index_kind {
            "hnsw" => IndexKind::Hnsw,
            "ivf" => IndexKind::Ivf,
            _ => IndexKind::BruteForce,
        };

        std::fs::create_dir_all(&path)?;

        let mut engine = Engine::new(&config);

        // L-3: Recover any prior state from existing WAL/snapshots at this path.
        // Without this call, reopening an existing database path yields an empty state.
        engine.try_recover();

        Ok(ValoricoreEngine {
            inner: Arc::new(Mutex::new(engine)),
        })
    }

    #[pyo3(signature = (vector, tag))]
    fn insert(&self, vector: Vec<f32>, tag: u64) -> PyResult<u32> {
        let mut engine = lock_engine!(self);

        // H-3: Reject mismatched dimensions before writing any event to the log.
        if let Some(dim) = engine.state.dim {
            if vector.len() != dim {
                return Err(PyValueError::new_err(format!(
                    "dimension mismatch: engine expects {dim}, got {}", vector.len()
                )));
            }
        }

        let mut fxp_data = Vec::with_capacity(vector.len());
        for (i, &f) in vector.iter().enumerate() {
            // M-1: consistent range validation across all insert paths.
            if f < -32767.0 || f > 32767.0 {
                return Err(PyValueError::new_err(format!(
                    "float at index {i} ({f}) outside valid Q16.16 range [-32767, 32767]"
                )));
            }
            fxp_data.push(valori_kernel::types::scalar::FxpScalar(from_f32(f).0));
        }
        let fxp_vec = FxpVector { data: fxp_data };

        if let Some(ref mut committer) = engine.event_committer {
            let rid = committer.live_state().next_record_id();
            let event = KernelEvent::InsertRecord { id: rid, vector: fxp_vec, metadata: None, tag };
            // C-1: commit_event() already applies the event to live_state internally
            // (shadow-apply → persist → live-apply).  Do NOT call apply_committed_event again.
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("commit failed: {:?}", e))
            })?;
            Ok(rid.0)
        } else {
            Err(PyRuntimeError::new_err("event log not initialized"))
        }
    }

    #[pyo3(signature = (vector, k, filter_tag=None))]
    fn search(&self, vector: Vec<f32>, k: usize, filter_tag: Option<u64>) -> PyResult<Vec<(u32, i64)>> {
        let engine = lock_engine!(self);

        // H-3: Reject dimension mismatches for search too; the kernel silently truncates
        // to min(query.len(), record.len()) which produces wrong distances, not errors.
        if let Some(dim) = engine.state.dim {
            if vector.len() != dim {
                return Err(PyValueError::new_err(format!(
                    "dimension mismatch: engine expects {dim}, got {}", vector.len()
                )));
            }
        }

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
            py_results.push((r.id.0 as u32, r.score));
        }

        Ok(py_results)
    }

    // L-2: &mut self is unnecessary; the Arc<Mutex<>> provides interior mutability.
    fn save(&self) -> PyResult<String> {
        let engine = lock_engine!(self);
        match engine.save_snapshot(None) {
            Ok(path) => Ok(path.to_string_lossy().to_string()),
            Err(e) => Err(PyRuntimeError::new_err(format!("{:?}", e)))
        }
    }

    #[pyo3(signature = (kind, record_id=None))]
    fn create_node(&self, kind: u8, record_id: Option<u32>) -> PyResult<u32> {
        let mut engine = lock_engine!(self);

        let rid = record_id.map(|r| RecordId(r));

        use valori_kernel::types::enums::NodeKind;
        let k = NodeKind::from_u8(kind)
            .ok_or_else(|| PyValueError::new_err(format!("invalid NodeKind: {}", kind)))?;

        if let Some(ref mut committer) = engine.event_committer {
            // Must read next_node_id from the committer's live_state, not engine.state.
            // engine.state is never mutated when a committer is present — only
            // committer.live_state is. Using engine.state gives a stale (always-0)
            // ID after the first node, causing ShadowApply(InvalidOperation).
            let next_id = committer.live_state().next_node_id();
            let event = KernelEvent::CreateNode { id: next_id, kind: k, record: rid };
            // C-1: commit_event applies internally; do NOT call apply_committed_event.
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("commit failed: {:?}", e))
            })?;
            Ok(next_id.0)
        } else {
            let node_id = engine.state.create_node(k, rid)
                .map_err(|e| PyRuntimeError::new_err(format!("{:?}", e)))?;
            if let Some(r) = rid {
                engine.record_to_node.insert(r.0, node_id.0);
            }
            Ok(node_id.0)
        }
    }

    fn create_edge(&self, from: u32, to: u32, kind: u8) -> PyResult<u32> {
        let mut engine = lock_engine!(self);
        engine.create_edge(from, to, kind).map_err(|e| {
            PyRuntimeError::new_err(format!("CreateEdge failed: {:?}", e))
        })
    }

    fn delete_node(&self, node_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        engine.delete_node(node_id).map_err(|e| {
            PyRuntimeError::new_err(format!("DeleteNode failed: {:?}", e))
        })
    }

    fn delete_edge(&self, edge_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        engine.delete_edge(edge_id).map_err(|e| {
            PyRuntimeError::new_err(format!("DeleteEdge failed: {:?}", e))
        })
    }

    #[pyo3(signature = (node_id))]
    fn get_node(&self, node_id: u32) -> PyResult<Option<(u8, Option<u32>)>> {
        let engine = lock_engine!(self);
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
        let engine = lock_engine!(self);
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
        let mut engine = lock_engine!(self);
        match engine.insert_batch(&vectors) {
            Ok(ids) => Ok(ids),
            Err(e) => Err(PyRuntimeError::new_err(
                format!("batch insert failed: {:?}", e)
            ))
        }
    }

    #[pyo3(signature = (vectors, tags))]
    fn insert_batch_with_proof(&self, vectors: Vec<Vec<f32>>, tags: Vec<u64>) -> PyResult<Vec<(u32, String)>> {
        if vectors.len() != tags.len() {
            return Err(PyValueError::new_err("vectors and tags must have the same length"));
        }

        let mut results = Vec::with_capacity(vectors.len());
        let mut engine = lock_engine!(self);

        for (i, vector) in vectors.iter().enumerate() {
            // H-3: dimension check before touching the log.
            if let Some(dim) = engine.state.dim {
                if vector.len() != dim {
                    return Err(PyValueError::new_err(format!(
                        "vector[{i}] dimension mismatch: engine expects {dim}, got {}", vector.len()
                    )));
                }
            }

            let mut fxp_data = Vec::with_capacity(vector.len());
            let mut fixed_values = Vec::with_capacity(vector.len());
            for (j, &f) in vector.iter().enumerate() {
                // M-1: consistent range validation across all insert paths.
                if f < -32767.0 || f > 32767.0 {
                    return Err(PyValueError::new_err(format!(
                        "vectors[{i}][{j}] ({f}) outside valid Q16.16 range [-32767, 32767]"
                    )));
                }
                let scalar = valori_kernel::types::scalar::FxpScalar(from_f32(f).0);
                fxp_data.push(scalar);
                fixed_values.push(scalar.0);
            }
            let fxp_vec = FxpVector { data: fxp_data };
            let proof_bytes = generate_proof_bytes(&fixed_values);
            let proof_hex = hex::encode(&proof_bytes);

            let tag = tags[i];

            if let Some(ref mut committer) = engine.event_committer {
                let rid = committer.live_state().next_record_id();
                let event = KernelEvent::InsertRecord {
                    id: rid,
                    vector: fxp_vec,
                    // M-2 note: proof is stored in record metadata intentionally for
                    // cryptographic provenance.  Users needing application metadata
                    // should use set_metadata() on the returned record id.
                    metadata: Some(proof_bytes),
                    tag,
                };
                // C-1: commit_event applies internally; do NOT call apply_committed_event.
                committer.commit_event(event).map_err(|e| {
                    PyRuntimeError::new_err(format!("commit failed: {:?}", e))
                })?;
                results.push((rid.0, proof_hex));
            } else {
                return Err(PyRuntimeError::new_err("event log not initialized"));
            }
        }

        Ok(results)
    }

    fn get_metadata(&self, record_id: u32) -> PyResult<Option<Vec<u8>>> {
        let engine = lock_engine!(self);
        let rid = RecordId(record_id);

        // 1. Check MetadataStore (high-level metadata committed via set_metadata).
        let key = format!("record_{}", record_id);
        if let Some(val) = engine.metadata.get(&key) {
            if let Ok(vec) = serde_json::from_value::<Vec<u8>>(val) {
                return Ok(Some(vec));
            }
        }

        // 2. Fallback to Record-level metadata (proof bytes from insert_with_proof).
        match engine.state.get_record(rid) {
            Some(record) => Ok(record.metadata.clone()),
            None => Err(PyValueError::new_err(
                format!("record {} not found", record_id)
            ))
        }
    }

    fn set_metadata(&self, record_id: u32, metadata: Vec<u8>) -> PyResult<()> {
        if metadata.len() > 65536 {
            return Err(PyValueError::new_err("metadata too large (max 64 KB)"));
        }

        let mut engine = lock_engine!(self);
        let rid = RecordId(record_id);

        if engine.state.get_record(rid).is_none() {
            return Err(PyValueError::new_err(format!("record {} not found", record_id)));
        }

        // H-2: Commit a SetMeta event so the metadata is in the BLAKE3 audit chain,
        // crash-safe, and replayable.  The sidecar write below is a redundant cache.
        let key = format!("record_{}", record_id);
        let value = hex::encode(&metadata);
        if let Some(ref mut committer) = engine.event_committer {
            let event = KernelEvent::SetMeta { key: key.clone(), value };
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("set_metadata commit failed: {:?}", e))
            })?;
        }

        // Redundant sidecar for fast get_metadata lookups (non-authoritative).
        let json_value = serde_json::to_value(&metadata)
            .map_err(|e| PyValueError::new_err(format!("serialize failed: {}", e)))?;
        engine.metadata.set(key, json_value);
        if let Err(e) = engine.flush_metadata() {
            eprintln!("[valoricore-ffi] set_metadata: failed to persist metadata sidecar: {:?}", e);
        }
        Ok(())
    }

    fn get_state_hash(&self) -> PyResult<String> {
        let engine = lock_engine!(self);
        let proof = engine.get_proof();
        Ok(hex::encode(proof.final_state_hash))
    }

    fn record_count(&self) -> PyResult<usize> {
        let engine = lock_engine!(self);
        Ok(engine.state.record_count())
    }

    fn snapshot(&self) -> PyResult<Vec<u8>> {
        let engine = lock_engine!(self);
        match engine.snapshot() {
            Ok(data) => Ok(data),
            Err(e) => Err(PyRuntimeError::new_err(
                format!("snapshot failed: {:?}", e)
            ))
        }
    }

    fn restore(&self, data: Vec<u8>) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        match engine.restore(&data) {
            Ok(_) => Ok(()),
            Err(e) => Err(PyRuntimeError::new_err(
                format!("restore failed: {:?}", e)
            ))
        }
    }

    fn soft_delete(&self, record_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        engine.soft_delete_record(record_id).map_err(|e| {
            PyRuntimeError::new_err(format!("SoftDelete failed: {:?}", e))
        })?;
        Ok(())
    }

    fn delete(&self, record_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        engine.delete_record(record_id).map_err(|e| {
            PyRuntimeError::new_err(format!("Delete failed: {:?}", e))
        })?;
        Ok(())
    }

    #[pyo3(signature = (vector, tag))]
    fn insert_with_proof(&self, vector: Vec<f32>, tag: u64) -> PyResult<(u32, String)> {
        let mut fxp_data = Vec::with_capacity(vector.len());
        let mut fixed_values = Vec::with_capacity(vector.len());
        for (i, &f) in vector.iter().enumerate() {
            if f < -32767.0 || f > 32767.0 {
                return Err(PyValueError::new_err(format!(
                    "float at index {i} ({f}) outside valid Q16.16 range [-32767, 32767]"
                )));
            }
            let scalar = valori_kernel::types::scalar::FxpScalar(from_f32(f).0);
            fxp_data.push(scalar);
            fixed_values.push(scalar.0);
        }
        let fxp_vec = FxpVector { data: fxp_data };

        let proof_bytes = generate_proof_bytes(&fixed_values);
        let proof_hex = hex::encode(&proof_bytes);

        let mut engine = lock_engine!(self);

        // H-3: dimension check after lock (dim is set by first insert).
        if let Some(dim) = engine.state.dim {
            if vector.len() != dim {
                return Err(PyValueError::new_err(format!(
                    "dimension mismatch: engine expects {dim}, got {}", vector.len()
                )));
            }
        }

        if let Some(ref mut committer) = engine.event_committer {
            let rid = committer.live_state().next_record_id();
            let event = KernelEvent::InsertRecord {
                id: rid,
                vector: fxp_vec,
                metadata: Some(proof_bytes),
                tag,
            };
            // C-1: commit_event applies internally; do NOT call apply_committed_event.
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("commit failed: {:?}", e))
            })?;
            Ok((rid.0, proof_hex))
        } else {
            Err(PyRuntimeError::new_err("event log not initialized"))
        }
    }

    fn get_timeline(&self) -> PyResult<Vec<String>> {
        let engine = lock_engine!(self);
        let Some(ref committer) = engine.event_committer else {
            return Ok(Vec::new());
        };

        let committed = committer.journal().committed();
        let mut events = Vec::with_capacity(committed.len());

        for (event_id, event) in committed.iter().enumerate() {
            let event_str = match event {
                KernelEvent::InsertRecord { id, tag, .. } =>
                    format!("Event ID {event_id}: InsertRecord (Record {}, Tag: {tag})", id.0),
                KernelEvent::DeleteRecord { id } =>
                    format!("Event ID {event_id}: DeleteRecord (Record {})", id.0),
                KernelEvent::SoftDeleteRecord { id } =>
                    format!("Event ID {event_id}: SoftDeleteRecord (Record {})", id.0),
                KernelEvent::CreateNode { id, kind, .. } =>
                    format!("Event ID {event_id}: CreateNode (Node {}, Kind: {kind:?})", id.0),
                KernelEvent::CreateEdge { id, from, to, kind } =>
                    format!("Event ID {event_id}: CreateEdge (Edge {}, {from:?} -> {to:?}, Kind: {kind:?})", id.0),
                KernelEvent::DeleteEdge { id } =>
                    format!("Event ID {event_id}: DeleteEdge (Edge {})", id.0),
                KernelEvent::DeleteNode { id } =>
                    format!("Event ID {event_id}: DeleteNode (Node {})", id.0),
                KernelEvent::InsertRecordEncrypted { id, key_id, .. } =>
                    format!("Event ID {event_id}: InsertRecordEncrypted (Record {}, key {})",
                        id.0, key_id.iter().take(4).map(|b| format!("{b:02x}")).collect::<String>()),
                KernelEvent::ShredKey { key_id } =>
                    format!("Event ID {event_id}: ShredKey (key {})",
                        key_id.iter().take(4).map(|b| format!("{b:02x}")).collect::<String>()),
                KernelEvent::AutoInsertRecord { tag, .. } =>
                    format!("Event ID {event_id}: AutoInsertRecord (Tag: {tag})"),
                KernelEvent::AutoCreateNode { kind, .. } =>
                    format!("Event ID {event_id}: AutoCreateNode (Kind: {kind:?})"),
                KernelEvent::AutoCreateEdge { from, to, kind } =>
                    format!("Event ID {event_id}: AutoCreateEdge ({from:?} -> {to:?}, Kind: {kind:?})"),
                KernelEvent::AutoInsertRecordEncrypted { key_id, tag, .. } =>
                    format!("Event ID {event_id}: AutoInsertRecordEncrypted (key {}, Tag: {tag})",
                        key_id.iter().take(4).map(|b| format!("{b:02x}")).collect::<String>()),
                KernelEvent::SetMeta { key, value } =>
                    format!("Event ID {event_id}: SetMeta ({key:?} = {value:?})"),
            };
            events.push(event_str);
        }

        Ok(events)
    }
}

#[pyfunction]
fn ingest_embedding(floats: Vec<f32>) -> PyResult<Vec<i32>> {
    for (i, &f) in floats.iter().enumerate() {
        if f < -32767.0 || f > 32767.0 {
            return Err(PyValueError::new_err(format!(
                "float at index {i} ({f}) outside valid range [-32767, 32767]"
            )));
        }
    }
    let fixed: Vec<i32> = floats.iter().map(|&f| from_f32(f).0).collect();
    Ok(fixed)
}

#[pyfunction]
fn generate_proof(fixed_values: Vec<i32>) -> PyResult<String> {
    if fixed_values.is_empty() {
        return Err(PyValueError::new_err("cannot generate proof for empty vector"));
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
