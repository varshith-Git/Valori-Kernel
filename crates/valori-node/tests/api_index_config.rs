// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 3 — Index configuration endpoint tests.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;
use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::build_router;
use valori_node::EngineFromNodeConfig;

async fn get_index_config(shared: Arc<RwLock<Engine>>) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared, None, None);
    let req = Request::builder()
        .method("GET")
        .uri("/v1/index/config")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1 << 16)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

#[tokio::test]
async fn index_config_returns_collection_scoped() {
    let cfg = NodeConfig::default();
    let engine = Arc::new(RwLock::new(Engine::new(&cfg)));
    let (status, json) = get_index_config(engine).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["index_type"], "collection_scoped");
}
