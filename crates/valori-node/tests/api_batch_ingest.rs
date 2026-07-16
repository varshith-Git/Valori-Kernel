// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_node::config::NodeConfig;
use valori_node::EngineFromNodeConfig;
use valori_node::server::build_router;
use valori_node::engine::Engine;
use valori_node::api::{BatchInsertRequest, BatchInsertResponse};
use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;
use std::sync::Arc;
use tokio::sync::RwLock;
use tempfile::tempdir;

const DIM: usize = 16;

fn make_cfg(dir: &std::path::Path) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.max_records = 100;
    cfg.max_nodes = 100;
    cfg.max_edges = 200;
    cfg.wal_path = Some(dir.join("valori.wal"));
    cfg.event_log_path = Some(dir.join("events.log"));
    cfg
}

#[tokio::test]
async fn test_batch_ingest_success() {
    let dir = tempdir().unwrap();
    let config = make_cfg(dir.path());

    let engine = Engine::new(&config);
    let shared_state = Arc::new(RwLock::new(engine));
    let app = build_router(shared_state, None, None);

    let batch = vec![vec![0.1; DIM], vec![0.2; DIM], vec![0.3; DIM]];
    let req = Request::builder()
        .method("POST")
        .uri("/v1/vectors/batch_insert")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&BatchInsertRequest { batch, collection: None, metadata: None, request_ids: None, texts: None }).unwrap()))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
    let resp: BatchInsertResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp.ids, vec![0, 1, 2]);
}

#[tokio::test]
async fn test_batch_ingest_wrong_dimension_is_rejected() {
    let dir = tempdir().unwrap();
    let config = make_cfg(dir.path());

    let engine = Engine::new(&config);
    let shared_state = Arc::new(RwLock::new(engine));
    let app = build_router(shared_state.clone(), None, None);

    // One vector has the wrong dimension — the whole batch must be rejected.
    let batch = vec![
        vec![0.1; DIM],
        vec![0.2; DIM + 1], // wrong dim
        vec![0.3; DIM],
    ];
    let req = Request::builder()
        .method("POST")
        .uri("/v1/vectors/batch_insert")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&BatchInsertRequest { batch, collection: None, metadata: None, request_ids: None, texts: None }).unwrap()))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert!(
        response.status().is_client_error() || response.status().is_server_error(),
        "Expected error status, got {}",
        response.status()
    );

    // Nothing should have been inserted.
    let engine = shared_state.read().await;
    assert!(
        engine.search_l2(&vec![0.1; DIM], 1).unwrap().is_empty(),
        "No records should have been inserted after a rejected batch"
    );
}
