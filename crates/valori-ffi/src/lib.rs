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
        let snapshot_path = std::path::PathBuf::from(format!("{}/current.snap", path));
        config.wal_path = Some(wal_path);
        config.event_log_path = Some(event_log_path);
        config.snapshot_path = Some(snapshot_path);

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
        // Must read dim from live_state when committer is active — engine.state.dim
        // is always None in the committer path because engine.state is never mutated.
        let current_dim = if let Some(ref c) = engine.event_committer {
            c.live_state().dim
        } else {
            engine.state.dim
        };
        if let Some(dim) = current_dim {
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

        // I-1: scope the committer borrow so engine.index can be borrowed after.
        // commit_event only updates live_state; engine.index is a separate structure
        // that must be populated explicitly for HNSW/IVF to work.
        let rid = {
            let committer = engine.event_committer.as_mut()
                .ok_or_else(|| PyRuntimeError::new_err("event log not initialized"))?;
            let rid = committer.live_state().next_record_id();
            let event = KernelEvent::InsertRecord { id: rid, vector: fxp_vec, metadata: None, tag };
            // C-1: commit_event() already applies the event to live_state internally.
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("commit failed: {:?}", e))
            })?;
            rid
        };
        engine.index.insert(rid.0, &vector);
        Ok(rid.0)
    }

    #[pyo3(signature = (vector, k, filter_tag=None))]
    fn search(&self, vector: Vec<f32>, k: usize, filter_tag: Option<u64>) -> PyResult<Vec<(u32, i64)>> {
        let engine = lock_engine!(self);

        // When a committer is active, engine.state is never mutated — reads must
        // go to live_state.  Fall back to engine.state in the no-committer path.
        let state_ref: &valori_kernel::state::kernel::KernelState = if let Some(ref c) = engine.event_committer {
            c.live_state()
        } else {
            &engine.state
        };

        // H-3: Reject dimension mismatches; the kernel silently truncates to
        // min(query.len(), record.len()) which produces wrong distances, not errors.
        if let Some(dim) = state_ref.dim {
            if vector.len() != dim {
                return Err(PyValueError::new_err(format!(
                    "dimension mismatch: engine expects {dim}, got {}", vector.len()
                )));
            }
        }

        // I-1: use engine.index (HNSW/IVF/brute) when no tag filter — gives the
        // correct index for the configured kind.  Fall back to kernel search_l2
        // only when tag filtering is required (the engine index has no tag awareness).
        let py_results: Vec<(u32, i64)> = if filter_tag.is_none() {
            let hits = engine.index.search(&vector, k);
            hits.into_iter().map(|(id, dist)| (id, (dist * 65536.0) as i64)).collect()
        } else {
            let mut fxp_data = Vec::with_capacity(vector.len());
            for &v in &vector {
                fxp_data.push(valori_kernel::types::scalar::FxpScalar(from_f32(v).0));
            }
            let fxp_vec = FxpVector { data: fxp_data };
            let mut results = vec![valori_kernel::index::SearchResult::default(); k];
            let count = state_ref.search_l2(&fxp_vec, &mut results, filter_tag);
            results[..count].iter().map(|r| (r.id.0 as u32, r.score)).collect()
        };

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
        use valori_kernel::types::id::NodeId;
        let nid = NodeId(node_id);

        // Verify node exists in live_state (engine.state is stale in committer path).
        let exists = if let Some(ref c) = engine.event_committer {
            c.live_state().get_node(nid).is_some()
        } else {
            engine.state.get_node(nid).is_some()
        };
        if !exists {
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("node {} not found", node_id)
            ));
        }

        let event = KernelEvent::DeleteNode { id: nid };
        if let Some(ref mut committer) = engine.event_committer {
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to delete node {}: {:?}", node_id, e))
            })?;
        } else {
            engine.delete_node(node_id).map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to delete node {}: {:?}", node_id, e))
            })?;
        }
        Ok(())
    }

    fn delete_edge(&self, edge_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        use valori_kernel::types::id::EdgeId;
        let eid = EdgeId(edge_id);

        // Verify edge exists in live_state (engine.state is stale in committer path).
        // Edges are checked via their presence in the state's edge pool.
        let exists = if let Some(ref c) = engine.event_committer {
            c.live_state().get_edge(eid).is_some()
        } else {
            engine.state.get_edge(eid).is_some()
        };
        if !exists {
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("edge {} not found", edge_id)
            ));
        }

        let event = KernelEvent::DeleteEdge { id: eid };
        if let Some(ref mut committer) = engine.event_committer {
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to delete edge {}: {:?}", edge_id, e))
            })?;
        } else {
            engine.delete_edge(edge_id).map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to delete edge {}: {:?}", edge_id, e))
            })?;
        }
        Ok(())
    }

    #[pyo3(signature = (node_id))]
    fn get_node(&self, node_id: u32) -> PyResult<Option<(u8, Option<u32>)>> {
        let engine = lock_engine!(self);
        use valori_kernel::types::id::NodeId;

        let state_ref: &valori_kernel::state::kernel::KernelState = if let Some(ref c) = engine.event_committer {
            c.live_state()
        } else {
            &engine.state
        };

        match state_ref.get_node(NodeId(node_id)) {
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

        let state_ref: &valori_kernel::state::kernel::KernelState = if let Some(ref c) = engine.event_committer {
            c.live_state()
        } else {
            &engine.state
        };

        let mut py_edges = Vec::new();
        if let Some(iter) = state_ref.outgoing_edges(NodeId(node_id)) {
            for edge in iter {
                py_edges.push((edge.id.0, edge.to.0, edge.kind as u8));
            }
        }

        Ok(py_edges)
    }

    #[pyo3(signature = (start_node, max_depth = 2))]
    fn walk(&self, start_node: u32, max_depth: u32) -> PyResult<Vec<u32>> {
        let engine = lock_engine!(self);
        use valori_kernel::types::id::NodeId;
        use std::collections::{HashSet, VecDeque};

        let state_ref: &valori_kernel::state::kernel::KernelState = if let Some(ref c) = engine.event_committer {
            c.live_state()
        } else {
            &engine.state
        };

        let max_depth = std::cmp::min(max_depth, 10);
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        visited.insert(start_node);
        queue.push_back((start_node, 0));

        while let Some((current, depth)) = queue.pop_front() {
            result.push(current);
            if depth >= max_depth {
                continue;
            }

            if let Some(iter) = state_ref.outgoing_edges(NodeId(current)) {
                for edge in iter {
                    let nxt = edge.to.0;
                    if visited.insert(nxt) {
                        queue.push_back((nxt, depth + 1));
                    }
                }
            }
        }

        Ok(result)
    }

    #[pyo3(signature = (start_node, max_depth = 2))]
    fn expand(&self, start_node: u32, max_depth: u32) -> PyResult<Vec<u32>> {
        let engine = lock_engine!(self);
        use valori_kernel::types::id::NodeId;
        use std::collections::{HashSet, VecDeque};

        let state_ref: &valori_kernel::state::kernel::KernelState = if let Some(ref c) = engine.event_committer {
            c.live_state()
        } else {
            &engine.state
        };

        let max_depth = std::cmp::min(max_depth, 10);
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut record_ids = HashSet::new();

        visited.insert(start_node);
        queue.push_back((start_node, 0));

        while let Some((current, depth)) = queue.pop_front() {
            if let Some(node) = state_ref.get_node(NodeId(current)) {
                if let Some(rid) = node.record {
                    record_ids.insert(rid.0);
                }
            }
            if depth >= max_depth {
                continue;
            }

            if let Some(iter) = state_ref.outgoing_edges(NodeId(current)) {
                for edge in iter {
                    let nxt = edge.to.0;
                    if visited.insert(nxt) {
                        queue.push_back((nxt, depth + 1));
                    }
                }
            }
        }

        Ok(record_ids.into_iter().collect())
    }

    #[pyo3(signature = (vectors, tags=None))]
    fn insert_batch(&self, vectors: Vec<Vec<f32>>, tags: Option<Vec<u64>>) -> PyResult<Vec<u32>> {
        // Validate tags length upfront.
        if let Some(ref t) = tags {
            if t.len() != vectors.len() {
                return Err(PyValueError::new_err(format!(
                    "tags length {} does not match vectors length {}", t.len(), vectors.len()
                )));
            }
        }

        let mut engine = lock_engine!(self);

        // Build all events first (validate everything before touching the log).
        // Then commit as a single batch: one shadow-apply pass + one fsync.
        if engine.event_committer.is_some() {
            let first_id = engine.event_committer.as_ref().unwrap().live_state().next_record_id().0;
            let mut events = Vec::with_capacity(vectors.len());

            for (i, vector) in vectors.iter().enumerate() {
                if let Some(dim) = engine.event_committer.as_ref().unwrap().live_state().dim {
                    if vector.len() != dim {
                        return Err(PyValueError::new_err(format!(
                            "vector[{i}] dimension mismatch: engine expects {dim}, got {}", vector.len()
                        )));
                    }
                }
                let mut fxp_data = Vec::with_capacity(vector.len());
                for (j, &f) in vector.iter().enumerate() {
                    if f < -32767.0 || f > 32767.0 {
                        return Err(PyValueError::new_err(format!(
                            "vectors[{i}][{j}] ({f}) outside valid Q16.16 range [-32767, 32767]"
                        )));
                    }
                    fxp_data.push(valori_kernel::types::scalar::FxpScalar(from_f32(f).0));
                }
                let tag = tags.as_ref().map_or(0, |t| t[i]);
                events.push(KernelEvent::InsertRecord {
                    id: valori_kernel::types::id::RecordId(first_id + i as u32),
                    vector: FxpVector { data: fxp_data },
                    metadata: None,
                    tag,
                });
            }

            // Single shadow-apply + single fsync for the whole batch.
            engine.event_committer.as_mut().unwrap().commit_batch(events).map_err(|e| {
                PyRuntimeError::new_err(format!("batch insert failed: {:?}", e))
            })?;

            let ids: Vec<u32> = (first_id..first_id + vectors.len() as u32).collect();
            for (i, vector) in vectors.iter().enumerate() {
                engine.index.insert(ids[i], vector);
            }
            Ok(ids)
        } else {
            engine.insert_batch(&vectors).map_err(|e| {
                PyRuntimeError::new_err(format!("batch insert failed: {:?}", e))
            })
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
            // H-3: dimension check before touching the log. Read dim from
            // live_state when committer active — engine.state.dim is always None there.
            let current_dim = if let Some(ref c) = engine.event_committer {
                c.live_state().dim
            } else {
                engine.state.dim
            };
            if let Some(dim) = current_dim {
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

            let rid = {
                let committer = engine.event_committer.as_mut()
                    .ok_or_else(|| PyRuntimeError::new_err("event log not initialized"))?;
                let rid = committer.live_state().next_record_id();
                let event = KernelEvent::InsertRecord {
                    id: rid,
                    vector: fxp_vec,
                    metadata: Some(proof_bytes),
                    tag,
                };
                committer.commit_event(event).map_err(|e| {
                    PyRuntimeError::new_err(format!("commit failed: {:?}", e))
                })?;
                rid
            };
            engine.index.insert(rid.0, vector);
            results.push((rid.0, proof_hex));
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
        // Must read from live_state when committer is active.
        let state_ref: &valori_kernel::state::kernel::KernelState = if let Some(ref c) = engine.event_committer {
            c.live_state()
        } else {
            &engine.state
        };
        match state_ref.get_record(rid) {
            Some(record) => Ok(record.metadata.clone()),
            None => Ok(None),
        }
    }

    fn set_metadata(&self, record_id: u32, metadata: Vec<u8>) -> PyResult<()> {
        if metadata.len() > 65536 {
            return Err(PyValueError::new_err("metadata too large (max 64 KB)"));
        }

        let mut engine = lock_engine!(self);
        let rid = RecordId(record_id);

        // Must check live_state when committer is active.
        let record_exists = if let Some(ref c) = engine.event_committer {
            c.live_state().get_record(rid).is_some()
        } else {
            engine.state.get_record(rid).is_some()
        };
        if !record_exists {
            return Err(PyValueError::new_err(format!("record {} not found", record_id)));
        }

        // H-2: Commit a SetMeta event so the metadata is in the BLAKE3 audit chain.
        let key = format!("record_{}", record_id);
        let value = hex::encode(&metadata);
        if let Some(ref mut committer) = engine.event_committer {
            let event = KernelEvent::SetMeta { key: key.clone(), value };
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("set_metadata commit failed: {:?}", e))
            })?;
        }

        // metadata.json sidecar is the live retrieval path for get_metadata().
        // get_metadata() has no WAL-replay fallback, so a sidecar write failure
        // is a real durability loss — propagate it rather than swallowing it.
        let json_value = serde_json::to_value(&metadata)
            .map_err(|e| PyValueError::new_err(format!("serialize failed: {}", e)))?;
        engine.metadata.set(key, json_value);
        engine.flush_metadata()
            .map_err(|e| PyRuntimeError::new_err(format!("set_metadata: sidecar flush failed: {:?}", e)))?;
        Ok(())
    }

    fn get_state_hash(&self) -> PyResult<String> {
        let engine = lock_engine!(self);
        let state_ref: &valori_kernel::state::kernel::KernelState = if let Some(ref c) = engine.event_committer {
            c.live_state()
        } else {
            &engine.state
        };
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        Ok(hex::encode(hash_state_blake3(state_ref)))
    }

    fn record_count(&self) -> PyResult<usize> {
        let engine = lock_engine!(self);
        let state_ref: &valori_kernel::state::kernel::KernelState = if let Some(ref c) = engine.event_committer {
            c.live_state()
        } else {
            &engine.state
        };
        Ok(state_ref.record_count())
    }

    fn snapshot(&self) -> PyResult<Vec<u8>> {
        let mut engine = lock_engine!(self);
        // In the committer path, engine.state is never mutated — all mutations
        // land in committer.live_state. Sync live_state → engine.state before
        // snapshotting so the snapshot captures the actual current records.
        if let Some(ref c) = engine.event_committer {
            engine.state = c.live_state().clone();
        }
        match engine.snapshot() {
            Ok(data) => Ok(data),
            Err(e) => Err(PyRuntimeError::new_err(format!("snapshot failed: {:?}", e)))
        }
    }

    /// Write the current snapshot to `<db_path>/current.snap` and return the path.
    fn save_snapshot(&self) -> PyResult<String> {
        let mut engine = lock_engine!(self);
        // Flush any buffered WAL entries before snapshotting so the snapshot
        // and WAL are in sync — crash recovery will replay from the right offset.
        if let Some(ref mut c) = engine.event_committer {
            c.flush_pending().map_err(|e| PyRuntimeError::new_err(format!("flush failed: {:?}", e)))?;
            engine.state = c.live_state().clone();
        }
        match engine.save_snapshot(None) {
            Ok(path) => Ok(path.to_string_lossy().into_owned()),
            Err(e) => Err(PyRuntimeError::new_err(format!("save_snapshot failed: {:?}", e)))
        }
    }

    /// Flush buffered WAL entries to disk immediately.
    /// Call this when you need durability before an explicit snapshot.
    fn flush(&self) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        if let Some(ref mut c) = engine.event_committer {
            c.flush_pending().map_err(|e| PyRuntimeError::new_err(format!("flush failed: {:?}", e)))?;
        }
        Ok(())
    }

    fn restore(&self, data: Vec<u8>) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        engine.restore(&data).map_err(|e| {
            PyRuntimeError::new_err(format!("restore failed: {:?}", e))
        })?;
        // After restore, engine.state holds the correct records. Copy it into
        // the committer's live_state so that reads via live_state() (search,
        // record_count, get_node, etc.) reflect the restored snapshot.
        let restored_state = engine.state.clone();
        if let Some(ref mut committer) = engine.event_committer {
            *committer.live_state_mut() = restored_state;
        }
        Ok(())
    }

    fn soft_delete(&self, record_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        let rid = RecordId(record_id);

        // Verify record exists in live_state (engine.state is stale in committer path).
        let exists = if let Some(ref c) = engine.event_committer {
            c.live_state().get_record(rid).is_some()
        } else {
            engine.state.get_record(rid).is_some()
        };
        if !exists {
            return Err(pyo3::exceptions::PyValueError::new_err(format!("record {} not found", record_id)));
        }

        let event = KernelEvent::SoftDeleteRecord { id: rid };
        if let Some(ref mut committer) = engine.event_committer {
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("SoftDelete failed: {:?}", e))
            })?;
        } else {
            engine.soft_delete_record(record_id).map_err(|e| {
                PyRuntimeError::new_err(format!("SoftDelete failed: {:?}", e))
            })?;
        }
        engine.index.delete(record_id);
        Ok(())
    }

    fn delete(&self, record_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        let rid = RecordId(record_id);

        // Verify record exists in live_state (engine.state is stale in committer path).
        let exists = if let Some(ref c) = engine.event_committer {
            c.live_state().get_record(rid).is_some()
        } else {
            engine.state.get_record(rid).is_some()
        };
        if !exists {
            return Err(pyo3::exceptions::PyValueError::new_err(format!("record {} not found", record_id)));
        }

        let event = KernelEvent::DeleteRecord { id: rid };
        if let Some(ref mut committer) = engine.event_committer {
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("Delete failed: {:?}", e))
            })?;
        } else {
            engine.delete_record(record_id).map_err(|e| {
                PyRuntimeError::new_err(format!("Delete failed: {:?}", e))
            })?;
        }
        engine.index.delete(record_id);
        Ok(())
    }

    #[pyo3(signature = (vector, tag))]
    fn insert_with_proof(&self, vector: Vec<f32>, tag: u64) -> PyResult<(u32, String)> {
        let mut engine = lock_engine!(self);

        // H-3: dim + range checks before any allocation so a bad vector is
        // rejected without computing proof bytes or allocating FXP vecs.
        let current_dim = if let Some(ref c) = engine.event_committer {
            c.live_state().dim
        } else {
            engine.state.dim
        };
        if let Some(dim) = current_dim {
            if vector.len() != dim {
                return Err(PyValueError::new_err(format!(
                    "dimension mismatch: engine expects {dim}, got {}", vector.len()
                )));
            }
        }
        for (i, &f) in vector.iter().enumerate() {
            if f < -32767.0 || f > 32767.0 {
                return Err(PyValueError::new_err(format!(
                    "float at index {i} ({f}) outside valid Q16.16 range [-32767, 32767]"
                )));
            }
        }

        // Validation passed — now convert and compute proof.
        let mut fxp_data = Vec::with_capacity(vector.len());
        let mut fixed_values = Vec::with_capacity(vector.len());
        for &f in &vector {
            let scalar = valori_kernel::types::scalar::FxpScalar(from_f32(f).0);
            fxp_data.push(scalar);
            fixed_values.push(scalar.0);
        }
        let fxp_vec = FxpVector { data: fxp_data };
        let proof_bytes = generate_proof_bytes(&fixed_values);
        let proof_hex = hex::encode(&proof_bytes);

        let rid = {
            let committer = engine.event_committer.as_mut()
                .ok_or_else(|| PyRuntimeError::new_err("event log not initialized"))?;
            let rid = committer.live_state().next_record_id();
            let event = KernelEvent::InsertRecord {
                id: rid,
                vector: fxp_vec,
                metadata: Some(proof_bytes),
                tag,
            };
            committer.commit_event(event).map_err(|e| {
                PyRuntimeError::new_err(format!("commit failed: {:?}", e))
            })?;
            rid
        };
        engine.index.insert(rid.0, &vector);
        Ok((rid.0, proof_hex))
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
                KernelEvent::AutoCreateNamespace { name } =>
                    format!("Event ID {event_id}: AutoCreateNamespace (Name: {name:?})"),
                KernelEvent::DropNamespace { name } =>
                    format!("Event ID {event_id}: DropNamespace (Name: {name:?})"),
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

/// Replay an event log file in-process and return a JSON verification report string.
///
/// This is identical to `valori-verify --report` but runs inside the already-loaded
/// `.so` — no subprocess, no binary on PATH, no Rust toolchain required for pip users.
///
/// Args:
///     log_path:      Path to the events.log file.
///     expected_hash: Optional 64-char hex BLAKE3 state hash to compare against.
///
/// Returns:
///     JSON string with the same schema as `valori-verify --report`.
///
/// Raises:
///     RuntimeError: if the file cannot be read or has a corrupt header.
#[pyfunction]
#[pyo3(signature = (log_path, expected_hash = None))]
fn verify_log_file(log_path: String, expected_hash: Option<String>) -> PyResult<String> {
    use std::path::Path;
    let path = Path::new(&log_path);
    let result = valori_verify::verify_log_file(path, expected_hash.as_deref())
        .map_err(|e| PyRuntimeError::new_err(e))?;
    serde_json::to_string(&result).map_err(|e| PyRuntimeError::new_err(e.to_string()))
}

#[pymodule]
fn valoricore_ffi(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ValoricoreEngine>()?;
    m.add_function(wrap_pyfunction!(ingest_embedding, m)?)?;
    m.add_function(wrap_pyfunction!(generate_proof, m)?)?;
    m.add_function(wrap_pyfunction!(verify_embedding, m)?)?;
    m.add_function(wrap_pyfunction!(verify_log_file, m)?)?;
    Ok(())
}
