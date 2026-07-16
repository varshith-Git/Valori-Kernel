// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use std::sync::{Arc, Mutex};
use valori_kernel::event::KernelEvent;
use valori_kernel::fxp::ops::from_f32;
use valori_kernel::proof::generate_proof_bytes;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::EngineFromNodeConfig;

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
        // Null out server-mode-only fields so they have no effect in the embedded SDK.
        let mut config = NodeConfig {
            auth_token: None,
            keys_path: None,
            object_store_url: None,
            embed_provider: None,
            cors_origin: None,
            ..NodeConfig::default()
        };

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

        if let Some(dim) = engine.kernel_dim() {
            if vector.len() != dim {
                return Err(PyValueError::new_err(format!(
                    "dimension mismatch: engine expects {dim}, got {}",
                    vector.len()
                )));
            }
        }

        let mut fxp_data = Vec::with_capacity(vector.len());
        for (i, &f) in vector.iter().enumerate() {
            if f < -32767.0 || f > 32767.0 {
                return Err(PyValueError::new_err(format!(
                    "float at index {i} ({f}) outside valid Q16.16 range [-32767, 32767]"
                )));
            }
            fxp_data.push(valori_kernel::types::scalar::FxpScalar(from_f32(f).0));
        }
        let fxp_vec = FxpVector { data: fxp_data };

        engine
            .insert_record_fxp(fxp_vec, None, tag, valori_kernel::types::id::DEFAULT_NS.0)
            .map_err(|e| PyRuntimeError::new_err(format!("insert failed: {:?}", e)))
    }

    #[pyo3(signature = (vector, k, filter_tag=None))]
    fn search(
        &self,
        vector: Vec<f32>,
        k: usize,
        filter_tag: Option<u64>,
    ) -> PyResult<Vec<(u32, i64)>> {
        let engine = lock_engine!(self);

        // H-3: Reject dimension mismatches; the kernel silently truncates to
        // min(query.len(), record.len()) which produces wrong distances, not errors.
        if let Some(dim) = engine.kernel_dim() {
            if vector.len() != dim {
                return Err(PyValueError::new_err(format!(
                    "dimension mismatch: engine expects {dim}, got {}",
                    vector.len()
                )));
            }
        }

        // I-1: use engine.index (HNSW/IVF/brute) when no tag filter — gives the
        // correct index for the configured kind.  Fall back to tag-filtered brute-force
        // only when a tag is provided (the ANN index has no tag awareness).
        let py_results: Vec<(u32, i64)> = if filter_tag.is_none() {
            let hits = engine.index.search(&vector, k);
            hits.into_iter()
                .map(|(id, dist)| (id, (dist * 65536.0) as i64))
                .collect()
        } else {
            engine
                .search_l2_filtered(&vector, k, filter_tag)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
                .into_iter()
                .map(|(id, dist)| (id, (dist * 65536.0) as i64))
                .collect()
        };

        Ok(py_results)
    }

    #[pyo3(signature = (kind, record_id=None))]
    fn create_node(&self, kind: u8, record_id: Option<u32>) -> PyResult<u32> {
        let mut engine = lock_engine!(self);

        // When the event-log committer is active, insert_with_proof commits
        // records only to live_state (not engine.state). create_node_for_record
        // validates the record_id against engine.state via apply_committed_event_ns,
        // so it would return NotFound for records inserted via the committer path.
        // We must use the committer directly here to stay on the same state.
        if let Some(committer) = engine.event_committer_mut() {
            let node_kind =
                valori_kernel::types::enums::NodeKind::from_u8(kind).unwrap_or_default();
            let record = record_id.map(valori_kernel::types::id::RecordId);

            // Validate record exists in live_state before committing.
            if let Some(rid) = record {
                if committer.live_state().get_record(rid).is_none() {
                    return Err(PyRuntimeError::new_err(format!(
                        "CreateNode failed: Kernel(NotFound) — record {} not in live_state",
                        rid.0
                    )));
                }
            }

            let node_id = committer.live_state().next_node_id();
            let event = KernelEvent::CreateNode {
                id: node_id,
                kind: node_kind,
                record,
            };
            committer
                .commit_event(event)
                .map_err(|e| PyRuntimeError::new_err(format!("CreateNode failed: {:?}", e)))?;
            return Ok(node_id.0);
        }

        // WAL / ephemeral path: commit_and_apply_ns touches engine.state directly.
        let node_id = engine
            .create_node_for_record(record_id, kind, 0)
            .map_err(|e| PyRuntimeError::new_err(format!("CreateNode failed: {:?}", e)))?;
        Ok(node_id)
    }

    fn create_edge(&self, from: u32, to: u32, kind: u8) -> PyResult<u32> {
        let mut engine = lock_engine!(self);

        // Same live_state/engine.state split as create_node: use committer when active.
        if let Some(committer) = engine.event_committer_mut() {
            use valori_kernel::types::id::{EdgeId, NodeId};
            let from_id = NodeId(from);
            let to_id = NodeId(to);
            let edge_kind =
                valori_kernel::types::enums::EdgeKind::from_u8(kind).unwrap_or_default();
            let edge_id = committer.live_state().next_edge_id();
            let event = KernelEvent::CreateEdge {
                id: edge_id,
                kind: edge_kind,
                from: from_id,
                to: to_id,
            };
            committer
                .commit_event(event)
                .map_err(|e| PyRuntimeError::new_err(format!("CreateEdge failed: {:?}", e)))?;
            return Ok(edge_id.0);
        }

        engine
            .create_edge(from, to, kind)
            .map_err(|e| PyRuntimeError::new_err(format!("CreateEdge failed: {:?}", e)))
    }

    fn delete_node(&self, node_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        use valori_kernel::types::id::NodeId;
        let nid = NodeId(node_id);

        if engine.get_node(nid).is_none() {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "node {} not found",
                node_id
            )));
        }

        engine.delete_node(node_id).map_err(|e| {
            PyRuntimeError::new_err(format!("Failed to delete node {}: {:?}", node_id, e))
        })
    }

    fn delete_edge(&self, edge_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        use valori_kernel::types::id::EdgeId;
        let eid = EdgeId(edge_id);

        if engine.get_edge(eid).is_none() {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "edge {} not found",
                edge_id
            )));
        }

        engine.delete_edge(edge_id).map_err(|e| {
            PyRuntimeError::new_err(format!("Failed to delete edge {}: {:?}", edge_id, e))
        })
    }

    #[pyo3(signature = (node_id))]
    fn get_node(&self, node_id: u32) -> PyResult<Option<(u8, Option<u32>)>> {
        let engine = lock_engine!(self);
        use valori_kernel::types::id::NodeId;

        match engine.get_node(NodeId(node_id)) {
            Some(n) => {
                let rec = n.record.map(|r| r.0);
                Ok(Some((n.kind as u8, rec)))
            }
            None => Ok(None),
        }
    }

    #[pyo3(signature = (node_id))]
    fn get_edges(&self, node_id: u32) -> PyResult<Vec<(u32, u32, u8)>> {
        let engine = lock_engine!(self);
        use valori_kernel::types::id::NodeId;

        let mut py_edges = Vec::new();
        if let Some(iter) = engine.outgoing_edges(NodeId(node_id)) {
            for edge in iter {
                py_edges.push((edge.id.0, edge.to.0, edge.kind as u8));
            }
        }

        Ok(py_edges)
    }

    #[pyo3(signature = (start_node, max_depth = 2))]
    fn walk(&self, start_node: u32, max_depth: u32) -> PyResult<Vec<u32>> {
        let engine = lock_engine!(self);
        use std::collections::{HashSet, VecDeque};
        use valori_kernel::types::id::NodeId;

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

            if let Some(iter) = engine.outgoing_edges(NodeId(current)) {
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
        use std::collections::{HashSet, VecDeque};
        use valori_kernel::types::id::NodeId;

        let max_depth = std::cmp::min(max_depth, 10);
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut record_ids = HashSet::new();

        visited.insert(start_node);
        queue.push_back((start_node, 0));

        while let Some((current, depth)) = queue.pop_front() {
            if let Some(node) = engine.get_node(NodeId(current)) {
                if let Some(rid) = node.record {
                    record_ids.insert(rid.0);
                }
            }
            if depth >= max_depth {
                continue;
            }

            if let Some(iter) = engine.outgoing_edges(NodeId(current)) {
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
        if let Some(ref t) = tags {
            if t.len() != vectors.len() {
                return Err(PyValueError::new_err(format!(
                    "tags length {} does not match vectors length {}",
                    t.len(),
                    vectors.len()
                )));
            }
        }

        let mut engine = lock_engine!(self);
        let mut ids = Vec::with_capacity(vectors.len());

        for (i, vector) in vectors.iter().enumerate() {
            if let Some(dim) = engine.kernel_dim() {
                if vector.len() != dim {
                    return Err(PyValueError::new_err(format!(
                        "vector[{i}] dimension mismatch: engine expects {dim}, got {}",
                        vector.len()
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
            let fxp_vec = FxpVector { data: fxp_data };
            let rid = engine
                .insert_record_fxp(fxp_vec, None, tag, valori_kernel::types::id::DEFAULT_NS.0)
                .map_err(|e| {
                    PyRuntimeError::new_err(format!("batch insert failed at [{i}]: {:?}", e))
                })?;
            ids.push(rid);
        }

        Ok(ids)
    }

    #[pyo3(signature = (vectors, tags))]
    fn insert_batch_with_proof(
        &self,
        vectors: Vec<Vec<f32>>,
        tags: Vec<u64>,
    ) -> PyResult<Vec<(u32, String)>> {
        if vectors.len() != tags.len() {
            return Err(PyValueError::new_err(
                "vectors and tags must have the same length",
            ));
        }

        let mut results = Vec::with_capacity(vectors.len());
        let mut engine = lock_engine!(self);

        for (i, vector) in vectors.iter().enumerate() {
            if let Some(dim) = engine.kernel_dim() {
                if vector.len() != dim {
                    return Err(PyValueError::new_err(format!(
                        "vector[{i}] dimension mismatch: engine expects {dim}, got {}",
                        vector.len()
                    )));
                }
            }

            let mut fxp_data = Vec::with_capacity(vector.len());
            let mut fixed_values = Vec::with_capacity(vector.len());
            for (j, &f) in vector.iter().enumerate() {
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

            let rid = engine
                .insert_record_fxp(
                    fxp_vec,
                    Some(proof_bytes),
                    tags[i],
                    valori_kernel::types::id::DEFAULT_NS.0,
                )
                .map_err(|e| {
                    PyRuntimeError::new_err(format!(
                        "insert_batch_with_proof [{i}] failed: {:?}",
                        e
                    ))
                })?;

            results.push((rid, proof_hex));
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
        match engine.get_record(rid) {
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

        if engine.get_record(rid).is_none() {
            return Err(PyValueError::new_err(format!(
                "record {} not found",
                record_id
            )));
        }

        let key = format!("record_{}", record_id);
        let value = hex::encode(&metadata);
        engine
            .apply_meta_event(key.clone(), value)
            .map_err(|e| PyRuntimeError::new_err(format!("set_metadata commit failed: {:?}", e)))?;

        let json_value = serde_json::to_value(&metadata)
            .map_err(|e| PyValueError::new_err(format!("serialize failed: {}", e)))?;
        engine.metadata.set(key, json_value);
        engine.flush_metadata().map_err(|e| {
            PyRuntimeError::new_err(format!("set_metadata: sidecar flush failed: {:?}", e))
        })?;
        Ok(())
    }

    fn get_state_hash(&self) -> PyResult<String> {
        let engine = lock_engine!(self);
        Ok(engine.state_hash_hex())
    }

    fn record_count(&self) -> PyResult<usize> {
        let engine = lock_engine!(self);
        Ok(engine.record_count())
    }

    fn snapshot(&self) -> PyResult<Vec<u8>> {
        let engine = lock_engine!(self);
        // After E1 engine.state is always current — no sync needed before snapshot.
        match engine.snapshot() {
            Ok(data) => Ok(data),
            Err(e) => Err(PyRuntimeError::new_err(format!("snapshot failed: {:?}", e))),
        }
    }

    /// Write the current snapshot to `<db_path>/current.snap` and return the path.
    fn save_snapshot(&self) -> PyResult<String> {
        let mut engine = lock_engine!(self);
        // Flush any buffered WAL entries before snapshotting so the snapshot
        // and WAL are in sync — crash recovery will replay from the right offset.
        if let Some(c) = engine.event_committer_mut() {
            c.flush_pending()
                .map_err(|e| PyRuntimeError::new_err(format!("flush failed: {:?}", e)))?;
            // After E1 engine.state is always current; no sync needed.
        }
        match engine.save_snapshot(None) {
            Ok(path) => Ok(path.to_string_lossy().into_owned()),
            Err(e) => Err(PyRuntimeError::new_err(format!(
                "save_snapshot failed: {:?}",
                e
            ))),
        }
    }

    /// Flush buffered WAL entries to disk immediately.
    /// Call this when you need durability before an explicit snapshot.
    fn flush(&self) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        if let Some(c) = engine.event_committer_mut() {
            c.flush_pending()
                .map_err(|e| PyRuntimeError::new_err(format!("flush failed: {:?}", e)))?;
        }
        Ok(())
    }

    fn restore(&self, data: Vec<u8>) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        engine
            .restore(&data)
            .map_err(|e| PyRuntimeError::new_err(format!("restore failed: {:?}", e)))
    }

    fn soft_delete(&self, record_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        let rid = RecordId(record_id);

        if engine.get_record(rid).is_none() {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "record {} not found",
                record_id
            )));
        }

        engine
            .soft_delete_record(record_id)
            .map_err(|e| PyRuntimeError::new_err(format!("SoftDelete failed: {:?}", e)))
    }

    fn delete(&self, record_id: u32) -> PyResult<()> {
        let mut engine = lock_engine!(self);
        let rid = RecordId(record_id);

        if engine.get_record(rid).is_none() {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "record {} not found",
                record_id
            )));
        }

        engine
            .delete_record(record_id)
            .map_err(|e| PyRuntimeError::new_err(format!("Delete failed: {:?}", e)))
    }

    #[pyo3(signature = (vector, tag))]
    fn insert_with_proof(&self, vector: Vec<f32>, tag: u64) -> PyResult<(u32, String)> {
        let mut engine = lock_engine!(self);

        if let Some(dim) = engine.kernel_dim() {
            if vector.len() != dim {
                return Err(PyValueError::new_err(format!(
                    "dimension mismatch: engine expects {dim}, got {}",
                    vector.len()
                )));
            }
        }

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

        let rid = engine
            .insert_record_fxp(
                fxp_vec,
                Some(proof_bytes),
                tag,
                valori_kernel::types::id::DEFAULT_NS.0,
            )
            .map_err(|e| PyRuntimeError::new_err(format!("insert_with_proof failed: {:?}", e)))?;

        Ok((rid, proof_hex))
    }

    fn get_timeline(&self) -> PyResult<Vec<String>> {
        let engine = lock_engine!(self);
        let Some(committer) = engine.event_committer() else {
            return Ok(Vec::new());
        };

        let committed = committer.journal().committed();
        let mut events = Vec::with_capacity(committed.len());

        for (event_id, event) in committed.iter().enumerate() {
            let event_str = match event {
                KernelEvent::InsertRecord { id, tag, .. } => format!(
                    "Event ID {event_id}: InsertRecord (Record {}, Tag: {tag})",
                    id.0
                ),
                KernelEvent::DeleteRecord { id } => {
                    format!("Event ID {event_id}: DeleteRecord (Record {})", id.0)
                }
                KernelEvent::SoftDeleteRecord { id } => {
                    format!("Event ID {event_id}: SoftDeleteRecord (Record {})", id.0)
                }
                KernelEvent::CreateNode { id, kind, .. } => format!(
                    "Event ID {event_id}: CreateNode (Node {}, Kind: {kind:?})",
                    id.0
                ),
                KernelEvent::CreateEdge { id, from, to, kind } => format!(
                    "Event ID {event_id}: CreateEdge (Edge {}, {from:?} -> {to:?}, Kind: {kind:?})",
                    id.0
                ),
                KernelEvent::DeleteEdge { id } => {
                    format!("Event ID {event_id}: DeleteEdge (Edge {})", id.0)
                }
                KernelEvent::DeleteNode { id } => {
                    format!("Event ID {event_id}: DeleteNode (Node {})", id.0)
                }
                KernelEvent::InsertRecordEncrypted { id, key_id, .. } => format!(
                    "Event ID {event_id}: InsertRecordEncrypted (Record {}, key {})",
                    id.0,
                    key_id
                        .iter()
                        .take(4)
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                ),
                KernelEvent::ShredKey { key_id } => format!(
                    "Event ID {event_id}: ShredKey (key {})",
                    key_id
                        .iter()
                        .take(4)
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                ),
                KernelEvent::AutoInsertRecord { tag, .. } => {
                    format!("Event ID {event_id}: AutoInsertRecord (Tag: {tag})")
                }
                KernelEvent::AutoCreateNode { kind, .. } => {
                    format!("Event ID {event_id}: AutoCreateNode (Kind: {kind:?})")
                }
                KernelEvent::AutoCreateEdge { from, to, kind } => format!(
                    "Event ID {event_id}: AutoCreateEdge ({from:?} -> {to:?}, Kind: {kind:?})"
                ),
                KernelEvent::AutoInsertRecordEncrypted { key_id, tag, .. } => format!(
                    "Event ID {event_id}: AutoInsertRecordEncrypted (key {}, Tag: {tag})",
                    key_id
                        .iter()
                        .take(4)
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                ),
                KernelEvent::SetMeta { key, value } => {
                    format!("Event ID {event_id}: SetMeta ({key:?} = {value:?})")
                }
                KernelEvent::AutoCreateNamespace { name } => {
                    format!("Event ID {event_id}: AutoCreateNamespace (Name: {name:?})")
                }
                KernelEvent::DropNamespace { name } => {
                    format!("Event ID {event_id}: DropNamespace (Name: {name:?})")
                }
                KernelEvent::UpdateRecordMetadata { id, .. } => format!(
                    "Event ID {event_id}: UpdateRecordMetadata (Record {})",
                    id.0
                ),
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
        return Err(PyValueError::new_err(
            "cannot generate proof for empty vector",
        ));
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
        .map_err(PyRuntimeError::new_err)?;
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
