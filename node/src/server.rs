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
// Generic SharedEngine type alias
pub type SharedEngine<const M: usize, const D: usize, const N: usize, const E: usize> = 
    Arc<Mutex<Engine<M, D, N, E>>>;

use valori_kernel::types::enums::{NodeKind, EdgeKind};

// ... existing imports ...
use axum::extract::Query;

use axum::middleware::Next;
use axum::response::Response;
use axum::http::StatusCode;
use axum::extract::Request as AxumRequest;
use axum::http::header::AUTHORIZATION;


use axum::middleware::from_fn_with_state;

async fn auth_guard<const M: usize, const D: usize, const N: usize, const E: usize>(
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

pub fn build_router<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: SharedEngine<M, D, N, E>, 
    auth_token: Option<String>
) -> Router {
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
        // Proofs v1
        .route("/v1/proof/state", axum::routing::get(get_proof))
        .route("/v1/proof/event-log", axum::routing::get(get_event_proof)) // Phase 26
        // Replication v1
        .route("/v1/replication/wal", axum::routing::get(get_wal_stream))
        .route("/v1/replication/events", axum::routing::get(get_replication_events))
        .route("/v1/replication/state", axum::routing::get(get_replication_state))
        // Observability
        .route("/metrics", axum::routing::get(metrics_handler))
        .with_state(state);

    if let Some(token) = auth_token {
        tracing::info!("Auth Enabled: Bearer token required");
        let auth_state = Arc::new(Some(token));
        app = app.layer(from_fn_with_state(auth_state, auth_guard::<M, D, N, E>));
    } else {
        tracing::warn!("Auth Disabled: No token configured");
    }
    
    app
}

// ... existing handlers ...

async fn snapshot_save<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
    Json(req): Json<SnapshotSaveRequest>,
) -> Result<Json<SnapshotSaveResponse>, EngineError> {
    let mut engine = state.lock().await;
    let path = req.path.map(std::path::PathBuf::from);
    // Use engine default if path None
    let used_path = engine.save_snapshot(path.as_deref())?;
    
    Ok(Json(SnapshotSaveResponse {
        success: true,
        path: used_path.to_string_lossy().to_string(),
    }))
}

async fn snapshot_restore<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
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

async fn meta_set<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
    Json(payload): Json<MetadataSetRequest>,
) -> Result<Json<MetadataSetResponse>, EngineError> {
    let engine = state.lock().await;
    engine.metadata.set(payload.target_id, payload.metadata);
    Ok(Json(MetadataSetResponse { success: true }))
}

async fn meta_get<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
    Query(payload): Query<MetadataGetRequest>,
) -> Result<Json<MetadataGetResponse>, EngineError> {
    let engine = state.lock().await;
    let val = engine.metadata.get(&payload.target_id);
    Ok(Json(MetadataGetResponse {
        target_id: payload.target_id,
        metadata: val,
    }))
}


async fn insert_record<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
    Json(payload): Json<InsertRecordRequest>,
) -> Result<Json<InsertRecordResponse>, EngineError> {
    let mut engine = state.lock().await;
    let id = engine.insert_record_from_f32(&payload.values)?;
    Ok(Json(InsertRecordResponse { id }))
}

async fn search<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, EngineError> {
    let engine = state.lock().await;
    let hits = engine.search_l2(&payload.query, payload.k)?;
    
    let results = hits.into_iter().map(|(id, score)| SearchHit { id, score }).collect();
    Ok(Json(SearchResponse { results }))
}

async fn create_node<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
    Json(payload): Json<CreateNodeRequest>,
) -> Result<Json<CreateNodeResponse>, EngineError> {
    let mut engine = state.lock().await;
    let node_id = engine.create_node_for_record(payload.record_id, payload.kind)?;
    Ok(Json(CreateNodeResponse { node_id }))
}

async fn create_edge<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<Json<CreateEdgeResponse>, EngineError> {
    let mut engine = state.lock().await;
    let edge_id = engine.create_edge(payload.from, payload.to, payload.kind)?;
    Ok(Json(CreateEdgeResponse { edge_id }))
}

async fn snapshot<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
) -> Result<Vec<u8>, EngineError> {
    let engine = state.lock().await;
    engine.snapshot()
}

async fn restore<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
    body: axum::body::Bytes,
) -> Result<(), EngineError> {
    let mut engine = state.lock().await;
    engine.restore(&body)?;
    Ok(())
}

async fn memory_upsert_vector<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
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

async fn memory_search_vector<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
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

async fn get_proof<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
) -> Result<Json<valori_kernel::proof::DeterministicProof>, EngineError> {
    let engine = state.lock().await;
    let proof = engine.get_proof();
    Ok(Json(proof))
}

// Phase 26: Event log proof endpoint
async fn get_event_proof<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
) -> Result<Json<EventProofResponse>, EngineError> {
    let engine = state.lock().await;
    
    // Check if event committer is available
    if let Some(ref committer) = engine.event_committer {
        use valori_kernel::snapshot::blake3::hash_state_blake3;
        
        // Get current state and journal info
        let state_hash = hash_state_blake3(committer.live_state());
        let committed_height = committer.journal().committed_height();
        let event_count = committed_height; // Committed height == event count
        
        // TODO: Compute actual event log hash by reading the log file
        // For now, use a placeholder zeroed hash
        let event_log_hash = [0u8; 32];
        
        // Build response
        let response = EventProofResponse {
            kernel_version: 1,
            event_log_hash: format!("{:x}", u128::from_le_bytes(event_log_hash[..16].try_into().unwrap())),
            final_state_hash: format!("{:x}", u128::from_le_bytes(state_hash[..16].try_into().unwrap())),
            snapshot_hash: None, // TODO: Add snapshot hash if available
            event_count,
            committed_height,
        };
        
        Ok(Json(response))
    } else {
        Err(EngineError::InvalidInput(
            "Event log not enabled. Engine is running in WAL-only mode.".to_string()
        ))
    }
}

async fn get_wal_stream<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
) -> Result<Body, EngineError> {
    let path = {
        let engine = state.lock().await;
        engine.wal_path.clone()
    }.ok_or(EngineError::InvalidInput("No WAL configured for this node".into()))?;

    // Open file (async)
    let file = tokio::fs::File::open(&path).await
        .map_err(|e| EngineError::InvalidInput(format!("Failed to open WAL: {}", e)))?;
        
    // Create Stream
    let stream = ReaderStream::new(file);
    
    Ok(Body::from_stream(stream))
}

#[derive(Deserialize)]
struct ReplicationParams {
    start_offset: Option<u64>,
}

async fn get_replication_events<const M: usize, const D: usize, const N: usize, const E: usize>(
    State(state): State<SharedEngine<M, D, N, E>>,
    Query(params): Query<ReplicationParams>,
) -> Result<Body, EngineError> {
    let start_offset = params.start_offset.unwrap_or(0);

    // 1. Lock engine to get path and subscribe
    let (log_path, rx) = {
        let engine = state.lock().await;
        if let Some(ref committer) = engine.event_committer {
            (
                committer.event_log().path().to_path_buf(),
                committer.subscribe()
            )
        } else {
             return Err(EngineError::InvalidInput("Event log not enabled".to_string()));
        }
    };
    
    // 2. Spawn streaming task
    // Note: D is generic in helper, but Engine is typed with const D.
    // In server.rs, we use `ConcreteEngine` which has D=16 (hardcoded const).
    let rx_stream = crate::replication::spawn_replication_stream::<D>(
        log_path, 
        rx, 
        start_offset
    ).await?;
    
    // 3. Convert mpsc Receiver to Body Stream
    use tokio_util::io::ReaderStream;
    use futures::StreamExt;
    
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx_stream);
    
    // ReceiverStream yields Result<String, Error>.
    // Axum Body expects Result<Bytes, Error> or similar.
    let body_stream = stream.map(|res| {
        match res {
            Ok(json_line) => Ok(json_line),
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
        }
    });

    Ok(Body::from_stream(body_stream))
}

async fn get_replication_state() -> Json<serde_json::Value> {
    use crate::replication::{REPLICATION_STATUS, ReplicationState};
    use std::sync::atomic::Ordering;
    
    let status_u8 = REPLICATION_STATUS.load(Ordering::Relaxed);
    // 0=Synced, 1=Healing, 2=Diverged, 3=Unknown
    let status_str = match status_u8 {
        0 => "Synced",
        1 => "Healing",
        2 => "Diverged",
        _ => "Unknown",
    };
    
    Json(serde_json::json!({
        "status": status_str,
        "code": status_u8
    }))
}


async fn metrics_handler() -> String {
    crate::telemetry::get_metrics()
}
