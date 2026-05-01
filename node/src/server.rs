// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use axum::{
    routing::post,
    Router,
    extract::State,
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
use axum::response::Response;
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
    auth_token: Option<String>
) -> Router {
    let mut app = Router::new()
        .route("/health", axum::routing::get(health_check))
        .route("/version", axum::routing::get(version_handler))
        .route("/records", post(insert_record))
        .route("/v1/delete", post(delete_record))
        .route("/v1/vectors/batch_insert", post(batch_insert))
        .route("/search", post(search))
        .route("/graph/node", post(create_node))
        .route("/graph/edge", post(create_edge))
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
        .route("/metrics", axum::routing::get(metrics_handler))
        .with_state(state);

    if let Some(token) = auth_token {
        tracing::info!("Auth Enabled: Bearer token required");
        let auth_state = Arc::new(Some(token));
        app = app.layer(from_fn_with_state(auth_state, auth_guard));
    } else {
        tracing::warn!("Auth Disabled: No token configured");
    }
    
    app
}

async fn health_check() -> &'static str {
    "OK"
}

async fn version_handler() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

async fn delete_record(
    State(state): State<SharedEngine>,
    Json(payload): Json<DeleteRecordRequest>,
) -> Result<Json<DeleteRecordResponse>, EngineError> {
    let mut engine = state.lock().await;
    use valori_kernel::types::id::RecordId;
    
    if let Some(ref mut committer) = engine.event_committer {
         use valori_kernel::event::KernelEvent;
         let rid = RecordId(payload.id);
         let event = KernelEvent::DeleteRecord { id: rid };
         
         match committer.commit_event(event.clone()) {
             Ok(_) => {
                 engine.apply_committed_event(&event)?;
             }
             Err(e) => return Err(EngineError::InvalidInput(format!("Commit failed: {:?}", e))),
         }
    } else {
         engine.state.apply(&valori_kernel::state::command::Command::DeleteRecord { id: RecordId(payload.id) })
            .map_err(|e| EngineError::InvalidInput(e.to_string()))?;
    }

    Ok(Json(DeleteRecordResponse { success: true }))
}

async fn snapshot_save(
    State(state): State<SharedEngine>,
    Json(req): Json<SnapshotSaveRequest>,
) -> Result<Json<SnapshotSaveResponse>, EngineError> {
    let mut engine = state.lock().await;
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
    let id = engine.insert_record_from_f32(&payload.values)?;
    Ok(Json(InsertRecordResponse { id }))
}

async fn batch_insert(
    State(state): State<SharedEngine>,
    Json(payload): Json<BatchInsertRequest>,
) -> Result<Json<BatchInsertResponse>, EngineError> {
    let mut engine = state.lock().await;
    let ids = engine.insert_batch(&payload.batch)?;
    Ok(Json(BatchInsertResponse { ids }))
}

async fn search(
    State(state): State<SharedEngine>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, EngineError> {
    let engine = state.lock().await;
    let hits = engine.search_l2(&payload.query, payload.k)?;
    let results = hits.into_iter().map(|(id, score)| SearchHit { id, score }).collect();
    Ok(Json(SearchResponse { results }))
}

async fn create_node(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateNodeRequest>,
) -> Result<Json<CreateNodeResponse>, EngineError> {
    let mut engine = state.lock().await;
    let node_id = engine.create_node_for_record(payload.record_id, payload.kind)?;
    Ok(Json(CreateNodeResponse { node_id }))
}

async fn create_edge(
    State(state): State<SharedEngine>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<Json<CreateEdgeResponse>, EngineError> {
    let mut engine = state.lock().await;
    let edge_id = engine.create_edge(payload.from, payload.to, payload.kind)?;
    Ok(Json(CreateEdgeResponse { edge_id }))
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
    let record_id = engine.insert_record_from_f32(&payload.vector)?;

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
) -> Result<Json<valori_kernel::proof::DeterministicProof>, EngineError> {
    let engine = state.lock().await;
    Ok(Json(engine.get_proof()))
}

async fn get_event_proof(
    State(state): State<SharedEngine>,
) -> Result<Json<EventProofResponse>, EngineError> {
    let engine = state.lock().await;
    
    if let Some(ref committer) = engine.event_committer {
        let proof = engine.get_proof();
        let committed_height = committer.journal().committed_height();
        
        let response = EventProofResponse {
            kernel_version: 1,
            event_log_hash: format!("{:x}", u128::from_le_bytes(proof.final_state_hash[..16].try_into().unwrap())),
            final_state_hash: format!("{:x}", u128::from_le_bytes(proof.final_state_hash[..16].try_into().unwrap())),
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
        let engine = state.lock().await;
        if let Some(ref committer) = engine.event_committer {
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
    use crate::replication::REPLICATION_STATUS;
    use std::sync::atomic::Ordering;
    
    let status_u8 = REPLICATION_STATUS.load(Ordering::Relaxed);
    let status_str = match status_u8 {
        1 => "Synced",
        2 => "Diverged",
        3 => "Healing",
        _ => "Unknown",
    };
    
    Json(serde_json::json!({ "status": status_str, "code": status_u8 }))
}

async fn metrics_handler() -> String {
    crate::telemetry::get_metrics()
}
