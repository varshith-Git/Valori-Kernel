use valori_node::config::NodeConfig;
use valori_node::server::build_router;
use valori_node::engine::Engine;
use valori_node::api::{BatchInsertRequest, BatchInsertResponse, InsertRecordRequest};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt; // for oneshot
use std::sync::Arc;
use tokio::sync::Mutex;
use tempfile::tempdir;

// Define concrete types matching server.rs
const M: usize = 100;
const D: usize = 16;
const N: usize = 100;
const E: usize = 200;

#[tokio::test]
async fn test_batch_ingest_success() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("valori.wal");
    let event_log_path = dir.path().join("events.log");

    let mut config = NodeConfig::default();
    config.max_records = M;
    config.dim = D;
    config.max_nodes = N;
    config.max_edges = E;
    config.wal_path = Some(db_path.clone());
    config.event_log_path = Some(event_log_path.clone()); // Enable Event Log for Batching

    let engine = Engine::<M, D, N, E>::new(&config);
    let shared_state = Arc::new(Mutex::new(engine));
    let app = build_router(shared_state, None);

    // Prepare Batch
    let batch = vec![
        vec![0.1; D],
        vec![0.2; D],
        vec![0.3; D],
    ];

    let req = Request::builder()
        .method("POST")
        .uri("/v1/vectors/batch_insert")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&BatchInsertRequest { batch }).unwrap()))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
    let resp: BatchInsertResponse = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(resp.ids.len(), 3);
    assert_eq!(resp.ids, vec![0, 1, 2]); // First batch should get 0, 1, 2
}

#[tokio::test]
async fn test_batch_ingest_atomicity_failure() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("valori.wal");
    let event_log_path = dir.path().join("events.log");

    let mut config = NodeConfig::default();
    config.max_records = M;
    config.dim = D;
    config.max_nodes = N;
    config.max_edges = E;
    config.wal_path = Some(db_path.clone());
    config.event_log_path = Some(event_log_path.clone());

    let engine = Engine::<M, D, N, E>::new(&config);
    let shared_state = Arc::new(Mutex::new(engine));
    let app = build_router(shared_state.clone(), None);

    // Invalid payload (one vector has wrong dim)
    let batch = vec![
        vec![0.1; D],
        vec![0.2; D + 1], // INVALID DIM
        vec![0.3; D],
    ];

    let req = Request::builder()
        .method("POST")
        .uri("/v1/vectors/batch_insert")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&BatchInsertRequest { batch }).unwrap()))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    // Should fail validation before commit
    // Since insert_batch validates strictly before commit, this should return 500 or 400 depending on error mapping
    // EngineError::InvalidInput maps to INTERNAL_SERVER_ERROR currently? or BAD_REQUEST?
    // Let's check api.rs/errors.rs mapping. Usually InvalidInput -> 400?
    // Actually, Axum doesn't auto-map EngineError. 
    // Wait, EngineError needs IntoResponse.
    // Assuming standard error handling returns error code.
    assert!(response.status().is_client_error() || response.status().is_server_error());

    // Verify NOTHING was inserted
    let engine = shared_state.lock().await;
    // Check ID 0 is empty
    assert!(engine.search_l2(&vec![0.1; D], 1).unwrap().is_empty());
}
