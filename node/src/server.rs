// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use axum::{
    routing::post,
    Router,
    extract::State,
    Json,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::engine::Engine;
use crate::api::*;
use crate::errors::EngineError;

// Hardcoded consts matching Engine def in main.rs
// const MAX_RECORDS: usize = 1024;
// const D: usize = 16;
// ...
// To allow flexibility, we can define a trait or alias.
// But Engine implementation is generic.
// We need a specific type for the shared state.
// In main.rs we will decide the concrete type.
// Here we can define `SharedEngine` as a generic alias if we want, OR simple type alias if we fix dimensions in this crate.
// Given strict determinism, fixed dimensions are likely.
// Let's use the defaults from config.rs: 1024, 16, 1024, 2048.
pub const MAX_RECORDS: usize = 1024;
pub const D: usize = 16;
pub const MAX_NODES: usize = 1024;
pub const MAX_EDGES: usize = 2048;

pub type ConcreteEngine = Engine<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>;
pub type SharedEngine = Arc<Mutex<ConcreteEngine>>;

use valori_kernel::types::enums::{NodeKind, EdgeKind};

// ... existing imports ...
use axum::extract::Query;

use axum::middleware::{self, Next};
use axum::response::Response;
use axum::http::{Request, StatusCode};
use axum::extract::Request as AxumRequest;
use axum::http::header::AUTHORIZATION;
use axum::Extension;

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
    // No token configured implies no auth required? 
    // Logic in build_router conditionally adds middleware.
    // So if middleware is present, token is Some.
    // But passing Option allows flexibility. 
    // Re-reading build_router logic below.
    Ok(next.run(req).await)
}

pub fn build_router(state: SharedEngine, auth_token: Option<String>) -> Router {
    let mut app = Router::new()
        .route("/records", post(insert_record))
        .route("/search", post(search))
        .route("/graph/node", post(create_node))
        .route("/graph/edge", post(create_edge))
        .route("/v1/snapshot/download", axum::routing::get(snapshot)) 
        .route("/v1/snapshot/upload", post(restore))
        // Admin V1
        .route("/v1/snapshot/save", post(snapshot_save))
        .route("/v1/snapshot/restore", post(snapshot_restore))
        // Memory Protocol v0
        .route("/v1/memory/upsert_vector", post(memory_upsert_vector))
        .route("/v1/memory/search_vector", post(memory_search_vector))
        // Metadata v1
        .route("/v1/memory/meta/set", post(meta_set))
        .route("/v1/memory/meta/get", axum::routing::get(meta_get))
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

// ... existing handlers ...

async fn snapshot_save(
    State(state): State<SharedEngine>,
    Json(req): Json<SnapshotSaveRequest>,
) -> Result<Json<SnapshotSaveResponse>, EngineError> {
    let engine = state.lock().await;
    let path = req.path.map(std::path::PathBuf::from);
    // Use engine default if path None
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
    
    // We must read the file into bytes
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

    // 1. Insert vector as a record
    let record_id = engine.insert_record_from_f32(&payload.vector)?;

    // 2. Create or reuse document node
    let doc_node_id = if let Some(existing) = payload.attach_to_document_node {
        existing
    } else {
        let kind_val: u8 = NodeKind::Document as u8;
        engine.create_node_for_record(None, kind_val)?
    };

    // 3. Create chunk node attached to this record
    let chunk_kind_val: u8 = NodeKind::Chunk as u8;
    let chunk_node_id = engine.create_node_for_record(Some(record_id), chunk_kind_val)?;

    // 4. Link doc -> chunk as ParentOf
    let parent_edge_kind_val: u8 = EdgeKind::ParentOf as u8;
    engine.create_edge(doc_node_id, chunk_node_id, parent_edge_kind_val)?;

    let memory_id = format!("rec:{}", record_id);

    // 5. Store Metadata if provided
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

    // Engine already has search_l2 via KernelState
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
