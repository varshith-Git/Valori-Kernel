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

pub fn build_router(state: SharedEngine) -> Router {
    Router::new()
        .route("/records", post(insert_record))
        .route("/search", post(search))
        .route("/graph/node", post(create_node))
        .route("/graph/edge", post(create_edge))
        .route("/snapshot", post(snapshot))
        .route("/restore", post(restore))
        .with_state(state)
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
