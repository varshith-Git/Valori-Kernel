// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `k` bounds validation on the standalone `/search` endpoint. A client-
//! supplied `k` that's zero or absurdly large used to be passed straight
//! through to allocate a results buffer (amplified up to 20x on the rerank
//! path) — this pins the fix in place.

use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::Engine;
use valori_node::server::{build_router, SharedEngine};
use valori_node::EngineFromNodeConfig;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

fn cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 256;
    cfg.max_nodes = 64;
    cfg.max_edges = 64;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    cfg.snapshot_path = None;
    cfg
}

fn make_shared() -> SharedEngine {
    Arc::new(RwLock::new(Engine::new(&cfg())))
}

async fn post_json(
    shared: SharedEngine,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared, None, None);
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn search_rejects_k_zero() {
    let shared = make_shared();
    let (status, body) = post_json(
        shared,
        "/search",
        serde_json::json!({"query": [0.0, 0.0, 0.0, 0.0], "k": 0}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
}

#[tokio::test]
async fn search_rejects_k_above_ceiling() {
    let shared = make_shared();
    let (status, body) = post_json(
        shared,
        "/search",
        serde_json::json!({"query": [0.0, 0.0, 0.0, 0.0], "k": 1_000_000}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
}

#[tokio::test]
async fn search_accepts_k_within_bounds() {
    let shared = make_shared();
    post_json(
        shared.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    let (status, body) = post_json(
        shared,
        "/search",
        serde_json::json!({"query": [0.0, 0.0, 0.0, 0.0], "k": 10, "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
}

#[tokio::test]
async fn search_accepts_k_at_ceiling() {
    let shared = make_shared();
    post_json(
        shared.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    let (status, body) = post_json(
        shared,
        "/search",
        serde_json::json!({"query": [0.0, 0.0, 0.0, 0.0], "k": 5000, "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
}
