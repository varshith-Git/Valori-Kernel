// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use axum::{
    routing::{post, delete},
    Router,
    extract::{State, Path as AxumPath},
    Json,
    body::Body,
};
use tokio_util::io::ReaderStream;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::engine::Engine;
use crate::api::*;
use crate::errors::EngineError;
use serde::Deserialize;

pub type SharedEngine = Arc<Mutex<Engine>>;

use valori_kernel::types::enums::{NodeKind, EdgeKind};
use axum::extract::Query;
use axum::middleware::Next;
use axum::response::{Response, IntoResponse};
use axum::http::StatusCode;
use axum::extract::Request as AxumRequest;
use axum::http::header::AUTHORIZATION;
use axum::middleware::from_fn_with_state;

async fn auth_guard(
    State(token): State<Arc<Option<String>>>,
    req: AxumRequest,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Some(token_str) = &*token {
        let auth_header = req.headers().get(AUTHORIZATION)
            .and_then(|val| val.to_str().ok())
            .filter(|val| val.starts_with("Bearer "));
            
        if let Some(val) = auth_header {
             let provided = val.trim_start_matches("Bearer ");
             if provided == token_str {
                 return Ok(next.run(req).await);
             }
        }
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}

pub fn build_router(
    state: SharedEngine,
    auth_token: Option<String>,
) -> Router {
    // ── Public routes — no auth required ─────────────────────────────────────
    // Load balancers (health probes) and Prometheus scrapers must reach these
    // without a bearer token, even when VALORI_AUTH_TOKEN is configured.
    let public = Router::new()
        .route("/health",  axum::routing::get(health_check))
        .route("/metrics", axum::routing::get(metrics_handler))
        .with_state(state.clone());

    // ── Protected routes — require auth when a token is configured ────────────
    let mut protected = Router::new()
        .route("/version", axum::routing::get(version_handler))
        .route("/records", post(insert_record))
        .route("/v1/delete", post(delete_record))
        .route("/v1/vectors/batch_insert", post(batch_insert))
        .route("/search", post(search))
        .route("/graph/node", post(create_node))
        .route("/graph/node/:id", axum::routing::get(get_node))
        .route("/graph/edge", post(create_edge))
        .route("/graph/edges/:id", axum::routing::get(get_edges))
        .route("/v1/snapshot/download", axum::routing::get(snapshot))
        .route("/v1/snapshot/upload", post(restore))
        .route("/v1/snapshot/save", post(snapshot_save))
        .route("/v1/snapshot/restore", post(snapshot_restore))
        .route("/v1/memory/upsert_vector", post(memory_upsert_vector))
        .route("/v1/memory/search_vector", post(memory_search_vector))
        .route("/v1/memory/meta/set", post(meta_set))
        .route("/v1/memory/meta/get", axum::routing::get(meta_get))
        .route("/v1/proof/state", axum::routing::get(get_proof))
        .route("/v1/proof/event-log", axum::routing::get(get_event_proof))
        .route("/v1/replication/wal", axum::routing::get(get_wal_stream))
        .route("/v1/replication/events", axum::routing::get(get_replication_events))
        .route("/v1/replication/state", axum::routing::get(get_replication_state))
        .route("/timeline", axum::routing::get(get_timeline))
        .route("/v1/namespaces", post(create_collection_handler).get(list_collections_handler))
        .route("/v1/namespaces/:name", delete(drop_collection_handler))
        // Phase 3.1: object-store endpoints
        .route("/v1/storage/snapshots", axum::routing::get(list_remote_snapshots))
        .route("/v1/storage/snapshots/upload", post(upload_snapshot_to_store))
        .route("/v1/storage/snapshots/restore", post(restore_from_store))
        .route("/v1/storage/wal", axum::routing::get(list_remote_wal))
        .route("/v1/storage/wal/archive", post(archive_wal_segment))
        .with_state(state);

    if let Some(token) = auth_token {
        tracing::info!("Auth Enabled: Bearer token required");
        let auth_state = Arc::new(Some(token));
        protected = protected.layer(from_fn_with_state(auth_state, auth_guard));
    } else {
        tracing::warn!("Auth Disabled: No token configured");
    }

    Router::new().merge(public).merge(protected)
}

/// `GET /health` — structured health report for load balancers and operators.
///
/// HTTP status codes:
/// * **200** `"ok"`       — all pools below 90 % capacity
/// * **200** `"degraded"` — at least one pool ≥ 90 %; still serving all requests
/// * **503** `"full"`     — at least one pool at 100 %; inserts are being rejected
///
/// This endpoint is **always unauthenticated** so that load-balancer health
/// probes and liveness checks work without a bearer token.
async fn health_check(
    State(state): State<SharedEngine>,
) -> impl IntoResponse {
    let engine = state.lock().await;
    let h = engine.health();

    // Refresh Prometheus gauges on every health probe — cheap, and it means
    // the /metrics endpoint always reflects the latest state even between
    // heavy write bursts.
    engine.update_prometheus_metrics();

    let status_code = if h.status == "full" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (status_code, Json(h))
}

async fn version_handler() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

async fn delete_record(
    State(state): State<SharedEngine>,
    Json(payload): Json<DeleteRecordRequest>,
) -> Result<Json<DeleteRecordResponse>, EngineError> {
    let mut engine = state.lock().await;
    engine.resolve_collection(payload.collection.as_deref())?;
    engine.delete_record(payload.id)?;

    Ok(Json(DeleteRecordResponse { success: true }))
}

async fn snapshot_save(
    State(state): State<SharedEngine>,
    Json(req): Json<SnapshotSaveRequest>,
) -> Result<Json<SnapshotSaveResponse>, EngineError> {
    let engine = state.lock().await;
    let path = req.path.map(std::path::PathBuf::from);
    let used_path = engine.save_snapshot(path.as_deref())?;
    
    Ok(Json(SnapshotSaveResponse {
        success: true,
        path: used_path.to_string_lossy().to_string(),
    }))
}

async fn snapshot_restore(
    State(state): State<SharedEngine>,
    Json(req): Json<SnapshotRestoreRequest>,
) -> Result<Json<SnapshotRestoreResponse>, EngineError> {
    let mut engine = state.lock().await;
    let path = std::path::PathBuf::from(req.path);
    
    if !path.exists() {
        return Err(EngineError::InvalidInput(format!("Snapshot not found at {:?}", path)));
    }
    
    let data = tokio::fs::read(&path).await.map_err(|e| EngineError::InvalidInput(e.to_string()))?;
    engine.restore(&data)?;
    
    Ok(Json(SnapshotRestoreResponse { success: true }))
}

async fn meta_set(
    State(state): State<SharedEngine>,
    Json(payload): Json<MetadataSetRequest>,
) -> Result<Json<MetadataSetResponse>, EngineError> {
    let engine = state.lock().await;
    engine.metadata.set(payload.target_id, payload.metadata);
    if let Err(e) = engine.flush_metadata() {
        tracing::warn!("meta_set: failed to persist metadata sidecar: {:?}", e);
    }
    Ok(Json(MetadataSetResponse { success: true }))
}

async fn meta_get(
    State(state): State<SharedEngine>,
    Query(payload): Query<MetadataGetRequest>,
) -> Result<Json<MetadataGetResponse>, EngineError> {
    let engine = state.lock().await;
    let val = engine.metadata.get(&payload.target_id);
    Ok(Json(MetadataGetResponse {
        target_id: payload.target_id,
        metadata: val,
    }))
}

async fn insert_record(
    State(state): State<SharedEngine>,
    Json(payload): Json<InsertRecordRequest>,
) -> Result<Json<InsertRecordResponse>, EngineError> {
    let mut engine = state.lock().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let id = engine.insert_record_from_f32_ns(&payload.values, ns)?;
    Ok(Json(InsertRecordResponse { id }))
}

async fn batch_insert(
    State(state): State<SharedEngine>,
    Json(payload): Json<BatchInsertRequest>,
) -> Result<Json<BatchInsertResponse>, EngineError> {
    let mut engine = state.lock().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let ids = engine.insert_batch_ns(&payload.batch, ns)?;
    Ok(Json(BatchInsertResponse { ids }))
}

async fn search(
    State(state): State<SharedEngine>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, EngineError> {
    let engine = state.lock().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let hits = if ns == 0 {
        engine.search_l2(&payload.query, payload.k)?
    } else {
        engine.search_l2_ns(&payload.query, payload.k, ns)?
    };
    let results = hits.into_iter().map(|(id, score)| SearchHit { id, score }).collect();
    Ok(Json(SearchResponse { results }))
}

async fn create_node(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateNodeRequest>,
) -> Result<Json<CreateNodeResponse>, EngineError> {
    let mut engine = state.lock().await;
    engine.resolve_collection(payload.collection.as_deref())?;
    let node_id = engine.create_node_for_record(payload.record_id, payload.kind)?;
    Ok(Json(CreateNodeResponse { node_id }))
}

async fn create_edge(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<Json<CreateEdgeResponse>, EngineError> {
    let mut engine = state.lock().await;
    engine.resolve_collection(payload.collection.as_deref())?;
    let edge_id = engine.create_edge(payload.from, payload.to, payload.kind)?;
    Ok(Json(CreateEdgeResponse { edge_id }))
}

async fn get_node(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
) -> Result<Json<GetNodeResponse>, EngineError> {
    let engine = state.lock().await;
    use valori_kernel::types::id::NodeId;
    match engine.state.get_node(NodeId(id)) {
        Some(node) => {
            Ok(Json(GetNodeResponse {
                kind: node.kind as u8,
                record_id: node.record.map(|r| r.0),
            }))
        },
        None => Err(EngineError::Kernel(valori_kernel::error::KernelError::NotFound)),
    }
}

async fn get_edges(
    State(state): State<SharedEngine>,
    axum::extract::Path(id): axum::extract::Path<u32>,
) -> Result<Json<GetEdgesResponse>, EngineError> {
    let engine = state.lock().await;
    use valori_kernel::types::id::NodeId;
    
    let mut edges = Vec::new();
    if let Some(iter) = engine.state.outgoing_edges(NodeId(id)) {
        for edge in iter {
            edges.push(EdgeData {
                edge_id: edge.id.0,
                to_node: edge.to.0,
                kind: edge.kind as u8,
            });
        }
    }
    Ok(Json(GetEdgesResponse { edges }))
}

async fn snapshot(
    State(state): State<SharedEngine>,
) -> Result<Vec<u8>, EngineError> {
    let engine = state.lock().await;
    engine.snapshot()
}

async fn restore(
    State(state): State<SharedEngine>,
    body: axum::body::Bytes,
) -> Result<(), EngineError> {
    let mut engine = state.lock().await;
    engine.restore(&body)?;
    Ok(())
}

async fn memory_upsert_vector(
    State(state): State<SharedEngine>,
    Json(payload): Json<MemoryUpsertVectorRequest>,
) -> Result<Json<MemoryUpsertResponse>, EngineError> {
    let mut engine = state.lock().await;
    let ns = engine.resolve_collection(payload.collection.as_deref())?;
    let record_id = engine.insert_record_from_f32_ns(&payload.vector, ns)?;

    let doc_node_id = if let Some(existing) = payload.attach_to_document_node {
        existing
    } else {
        engine.create_node_for_record(None, NodeKind::Document as u8)?
    };

    let chunk_node_id = engine.create_node_for_record(Some(record_id), NodeKind::Chunk as u8)?;
    engine.create_edge(doc_node_id, chunk_node_id, EdgeKind::ParentOf as u8)?;

    let memory_id = format!("rec:{}", record_id);
    if let Some(meta) = payload.metadata {
        engine.metadata.set(memory_id.clone(), meta);
        if let Err(e) = engine.flush_metadata() {
            tracing::warn!("memory_upsert: failed to persist metadata sidecar: {:?}", e);
        }
    }

    Ok(Json(MemoryUpsertResponse {
        memory_id,
        record_id,
        document_node_id: doc_node_id,
        chunk_node_id,
    }))
}

async fn memory_search_vector(
    State(state): State<SharedEngine>,
    Json(payload): Json<MemorySearchVectorRequest>,
) -> Result<Json<MemorySearchResponse>, EngineError> {
    let engine = state.lock().await;
    let hits = engine.search_l2(&payload.query_vector, payload.k)?;

    let results = hits
        .into_iter()
        .map(|(record_id, score)| {
            let memory_id = format!("rec:{}", record_id);
            let metadata = engine.metadata.get(&memory_id);
            MemorySearchHit {
                memory_id,
                record_id,
                score,
                metadata,
            }
        })
        .collect();

    Ok(Json(MemorySearchResponse { results }))
}

async fn get_proof(
    State(state): State<SharedEngine>,
) -> impl IntoResponse {
    let engine = state.lock().await;
    let proof = engine.get_proof();
    // Encode all 32 bytes as lowercase hex — same wire format as the cluster's
    // state_proof handler so external clients see an identical response shape.
    let hex: String = proof.final_state_hash.iter().map(|b| format!("{b:02x}")).collect();
    Json(serde_json::json!({ "final_state_hash": hex }))
}

async fn get_event_proof(
    State(state): State<SharedEngine>,
) -> Result<Json<EventProofResponse>, EngineError> {
    let engine = state.lock().await;
    
    if let Some(ref committer) = engine.event_committer {
        let proof = engine.get_proof();
        let committed_height = committer.journal().committed_height();

        // Hash the actual event-log file with BLAKE3 (full 32 bytes → 64 hex chars).
        // Previously this was incorrectly set to the final_state_hash value, and both
        // hashes were truncated to 16 bytes then formatted without zero-padding,
        // yielding ≤32 hex chars instead of the correct 64.
        let event_log_path = committer.event_log().path().to_path_buf();
        let event_log_hash_bytes =
            crate::events::event_proof::compute_event_log_hash(&event_log_path)
                .unwrap_or([0u8; 32]);

        let response = EventProofResponse {
            kernel_version: 1,
            event_log_hash: event_log_hash_bytes.iter().map(|b| format!("{b:02x}")).collect(),
            final_state_hash: proof.final_state_hash.iter().map(|b| format!("{b:02x}")).collect(),
            snapshot_hash: None,
            event_count: committed_height,
            committed_height,
        };

        Ok(Json(response))
    } else {
        Err(EngineError::InvalidInput("Event log not enabled".to_string()))
    }
}

async fn get_wal_stream(
    State(state): State<SharedEngine>,
) -> Result<Body, EngineError> {
    let path = {
        let engine = state.lock().await;
        engine.wal_path.clone()
    }.ok_or(EngineError::InvalidInput("No WAL configured".into()))?;

    let file = tokio::fs::File::open(&path).await
        .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
    Ok(Body::from_stream(ReaderStream::new(file)))
}

#[derive(Deserialize)]
struct ReplicationParams {
    start_offset: Option<u64>,
}

async fn get_replication_events(
    State(state): State<SharedEngine>,
    Query(params): Query<ReplicationParams>,
) -> Result<Body, EngineError> {
    let start_offset = params.start_offset.unwrap_or(0);

    let (log_path, rx) = {
        let mut engine = state.lock().await;
        if let Some(ref mut committer) = engine.event_committer {
            if let Err(e) = committer.flush_log() {
                tracing::error!("Failed to flush event log for replication: {}", e);
            }
            (committer.event_log().path().to_path_buf(), committer.subscribe())
        } else {
             return Err(EngineError::InvalidInput("Event log not enabled".to_string()));
        }
    };
    
    let rx_stream = crate::replication::spawn_replication_stream(log_path, rx, start_offset).await?;
    
    use futures::StreamExt;
    let body_stream = tokio_stream::wrappers::ReceiverStream::new(rx_stream).map(|res| {
        match res {
            Ok(json_line) => Ok(json_line),
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
        }
    });

    Ok(Body::from_stream(body_stream))
}

async fn get_replication_state() -> Json<serde_json::Value> {
    let status_str = crate::replication::replication_display_state();
    Json(serde_json::json!({ "status": status_str }))
}

/// `GET /metrics` — Prometheus text exposition format.
///
/// Refreshes all KernelState gauges synchronously before rendering so that
/// the scrape always reflects the live pool sizes regardless of write
/// activity between scrapes.
///
/// This endpoint is **always unauthenticated** so that Prometheus can scrape
/// without a bearer token.
async fn metrics_handler(
    State(state): State<SharedEngine>,
) -> String {
    // Update kernel gauges from live state before rendering.
    {
        let engine = state.lock().await;
        engine.update_prometheus_metrics();
    }
    crate::telemetry::get_metrics()
}

async fn get_timeline(
    State(state): State<SharedEngine>,
) -> Result<Json<Vec<String>>, EngineError> {
    // Read from the in-memory EventJournal rather than re-parsing the
    // on-disk file: the journal.committed() slice is always current because
    // commit_buffer() runs synchronously inside commit_event().
    use valori_kernel::event::KernelEvent;

    let engine = state.lock().await;
    let Some(ref committer) = engine.event_committer else {
        return Err(EngineError::InvalidInput("Event log not enabled".to_string()));
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
                format!("Event ID {event_id}: AutoCreateEdge ({} → {}, Kind: {kind:?})", from.0, to.0),
        };
        events.push(event_str);
    }

    Ok(Json(events))
}

// ── Collection (namespace) management endpoints ───────────────────────────────

async fn create_collection_handler(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateCollectionRequest>,
) -> Result<Json<CreateCollectionResponse>, EngineError> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(EngineError::InvalidInput("collection name cannot be empty".into()));
    }
    let mut engine = state.lock().await;
    let already_exists = engine.namespaces.map.contains_key(&name) || name == "default";
    let id = engine.create_collection(&name)?;
    Ok(Json(CreateCollectionResponse {
        name,
        id,
        created: !already_exists,
    }))
}

async fn list_collections_handler(
    State(state): State<SharedEngine>,
) -> Json<ListCollectionsResponse> {
    let engine = state.lock().await;
    let collections = engine
        .list_collections()
        .into_iter()
        .map(|(name, id)| CollectionInfo { name, id })
        .collect();
    Json(ListCollectionsResponse { collections })
}

async fn drop_collection_handler(
    State(state): State<SharedEngine>,
    AxumPath(name): AxumPath<String>,
) -> Result<axum::http::StatusCode, EngineError> {
    let mut engine = state.lock().await;
    engine.drop_collection(&name)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ── Phase 3.1: object-store handlers ─────────────────────────────────────────

#[derive(serde::Serialize)]
struct StorageSnapshotUploadResponse {
    key: String,
    state_hash: String,
    size_bytes: usize,
    pruned: usize,
}

#[derive(serde::Serialize)]
struct ListRemoteSnapshotsResponse {
    snapshots: Vec<crate::object_store::SnapshotEntry>,
    count: usize,
}

#[derive(serde::Deserialize)]
struct RestoreFromStoreRequest {
    /// Object key returned by a previous upload or list call.
    key: String,
}

#[derive(serde::Serialize)]
struct RestoreFromStoreResponse {
    key: String,
    state_hash: String,
    size_bytes: usize,
}

#[derive(serde::Serialize)]
struct ListRemoteWalResponse {
    segments: Vec<crate::object_store::WalEntry>,
    count: usize,
}

#[derive(serde::Deserialize)]
struct ArchiveWalRequest {
    /// Absolute path on this node's local filesystem to the sealed segment.
    path: String,
}

#[derive(serde::Serialize)]
struct ArchiveWalResponse {
    key: String,
    size_bytes: u64,
}

/// `GET /v1/storage/snapshots` — list snapshots in the object store.
async fn list_remote_snapshots(
    State(state): State<SharedEngine>,
) -> Result<Json<ListRemoteSnapshotsResponse>, EngineError> {
    let object_store = {
        let engine = state.lock().await;
        engine.object_store.clone()
    };
    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;
    let snapshots = os.list_snapshots().await.map_err(|e| {
        EngineError::InvalidInput(format!("object store list failed: {e}"))
    })?;
    let count = snapshots.len();
    Ok(Json(ListRemoteSnapshotsResponse { snapshots, count }))
}

/// `POST /v1/storage/snapshots/upload` — snapshot current state and push to object store.
///
/// Automatically prunes old snapshots according to `VALORI_OBJECT_STORE_KEEP` (default 7).
async fn upload_snapshot_to_store(
    State(state): State<SharedEngine>,
) -> Result<Json<StorageSnapshotUploadResponse>, EngineError> {
    // Capture snapshot data and object store handle while holding the lock,
    // then release before any async I/O so we don't hold the mutex across awaits.
    let (snap_bytes, state_hash, object_store, keep) = {
        let engine = state.lock().await;
        let snap = engine.snapshot()?;
        let proof = engine.get_proof();
        let hash = proof
            .final_state_hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let os = engine.object_store.clone();
        let keep = engine.object_store_keep as usize;
        (snap, hash, os, keep)
    };

    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;

    let size_bytes = snap_bytes.len();
    let key = os
        .upload_snapshot(&snap_bytes, &state_hash)
        .await
        .map_err(|e| EngineError::InvalidInput(format!("upload failed: {e}")))?;

    let pruned = os
        .prune_snapshots(keep)
        .await
        .unwrap_or(0);

    Ok(Json(StorageSnapshotUploadResponse {
        key,
        state_hash,
        size_bytes,
        pruned,
    }))
}

/// `POST /v1/storage/snapshots/restore` — pull a snapshot from the object store and restore.
///
/// Body: `{ "key": "snapshots/00000001750000000_abc12345.snap" }`
async fn restore_from_store(
    State(state): State<SharedEngine>,
    Json(req): Json<RestoreFromStoreRequest>,
) -> Result<Json<RestoreFromStoreResponse>, EngineError> {
    let object_store = {
        let engine = state.lock().await;
        engine.object_store.clone()
    };
    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;

    let data = os
        .download_snapshot(&req.key)
        .await
        .map_err(|e| EngineError::InvalidInput(format!("download failed: {e}")))?;
    let size_bytes = data.len();

    {
        let mut engine = state.lock().await;
        engine.restore(&data)?;
    }

    // Compute hash of the just-restored state.
    let state_hash = {
        let engine = state.lock().await;
        engine
            .get_proof()
            .final_state_hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };

    tracing::info!(
        key = %req.key,
        state_hash = %state_hash,
        "restored from object store"
    );
    Ok(Json(RestoreFromStoreResponse {
        key: req.key,
        state_hash,
        size_bytes,
    }))
}

/// `GET /v1/storage/wal` — list archived WAL segments in the object store.
async fn list_remote_wal(
    State(state): State<SharedEngine>,
) -> Result<Json<ListRemoteWalResponse>, EngineError> {
    let object_store = {
        let engine = state.lock().await;
        engine.object_store.clone()
    };
    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;
    let segments = os.list_wal_segments().await.map_err(|e| {
        EngineError::InvalidInput(format!("object store list failed: {e}"))
    })?;
    let count = segments.len();
    Ok(Json(ListRemoteWalResponse { segments, count }))
}

/// `POST /v1/storage/wal/archive` — archive a sealed WAL segment to the object store.
///
/// Body: `{ "path": "/data/events.log.000001" }`
///
/// The segment must already be sealed (rotated away from the live log path).
/// Auto-archival on rotation is wired in Phase 3.2.
async fn archive_wal_segment(
    State(state): State<SharedEngine>,
    Json(req): Json<ArchiveWalRequest>,
) -> Result<Json<ArchiveWalResponse>, EngineError> {
    let object_store = {
        let engine = state.lock().await;
        engine.object_store.clone()
    };
    let os = object_store.ok_or_else(|| {
        EngineError::InvalidInput(
            "object store not configured — set VALORI_OBJECT_STORE_URL".into(),
        )
    })?;

    let local_path = std::path::Path::new(&req.path);
    if !local_path.exists() {
        return Err(EngineError::InvalidInput(format!(
            "segment not found: {}",
            req.path
        )));
    }
    let size_bytes = std::fs::metadata(local_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let key = os
        .archive_wal_segment(local_path)
        .await
        .map_err(|e| EngineError::InvalidInput(format!("archive failed: {e}")))?;

    Ok(Json(ArchiveWalResponse { key, size_bytes }))
}
