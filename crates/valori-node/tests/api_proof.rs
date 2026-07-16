// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! HTTP tests for proof endpoints:
//!   GET /v1/proof/state        — BLAKE3 state hash
//!   GET /v1/proof/event-log    — event-log hash + committed_height (requires event log)
//!   GET /v1/proof/receipt      — latest receipt (404 before any planner op)
//!   GET /v1/proof/receipt/:id  — receipt by id

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::{build_router, SharedEngine};
use valori_node::EngineFromNodeConfig;

fn engine_router(cfg: NodeConfig) -> (SharedEngine, axum::Router) {
    let engine = Engine::new(&cfg);
    let shared = Arc::new(RwLock::new(engine));
    let router = build_router(shared.clone(), None, None);
    (shared, router)
}

fn tiny_cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 100;
    cfg.max_nodes = 50;
    cfg.max_edges = 50;
    cfg
}

async fn get(router: axum::Router, uri: &str) -> (StatusCode, Value) {
    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!(null));
    (status, json)
}

async fn post_json(router: axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!(null));
    (status, json)
}

// ── /v1/proof/state ──────────────────────────────────────────────────────────

#[tokio::test]
async fn proof_state_returns_64_char_hex() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = get(router, "/v1/proof/state").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let hash = body["final_state_hash"]
        .as_str()
        .expect("missing final_state_hash");
    assert_eq!(hash.len(), 64, "expected 64-char hex, got '{hash}'");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "not hex: '{hash}'"
    );
}

#[tokio::test]
async fn proof_state_changes_after_insert() {
    let (_, router) = engine_router(tiny_cfg());

    let (_, before) = get(router.clone(), "/v1/proof/state").await;
    let hash_before = before["final_state_hash"].as_str().unwrap().to_string();

    let (status, _) = post_json(
        router.clone(),
        "/records",
        serde_json::json!({"values": [1.0f32, 0.0, 0.0, 0.0]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, after) = get(router, "/v1/proof/state").await;
    let hash_after = after["final_state_hash"].as_str().unwrap().to_string();

    assert_ne!(
        hash_before, hash_after,
        "state hash must change after insert"
    );
}

// ── /v1/proof/event-log ──────────────────────────────────────────────────────

#[tokio::test]
async fn proof_event_log_requires_event_log_enabled() {
    // Without an event log path, the endpoint returns an error.
    let (_, router) = engine_router(tiny_cfg());
    let (status, _) = get(router, "/v1/proof/event-log").await;
    // Returns 422/500 because event log not enabled — not 200.
    assert_ne!(
        status,
        StatusCode::OK,
        "expected error when event log not configured"
    );
}

#[tokio::test]
async fn proof_event_log_with_event_log_enabled() {
    // Use a non-existent path inside a tempdir so EventLogWriter creates it fresh
    // (an empty pre-existing file causes a parse failure → silent Ephemeral fallback).
    let tmp_dir = tempfile::tempdir().unwrap();
    let log_path = tmp_dir.path().join("events.log");
    let mut cfg = tiny_cfg();
    cfg.event_log_path = Some(log_path);
    let (_, router) = engine_router(cfg);

    // Insert something so there's a committed event.
    let (status, _) = post_json(
        router.clone(),
        "/records",
        serde_json::json!({"values": [1.0f32, 0.0, 0.0, 0.0]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get(router, "/v1/proof/event-log").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let hash = body["event_log_hash"]
        .as_str()
        .expect("missing event_log_hash");
    assert_eq!(hash.len(), 64, "event_log_hash must be 64-char hex");

    let state_hash = body["final_state_hash"]
        .as_str()
        .expect("missing final_state_hash");
    assert_eq!(state_hash.len(), 64, "final_state_hash must be 64-char hex");

    assert!(
        body["committed_height"].as_u64().unwrap_or(0) >= 1,
        "committed_height must be >= 1"
    );
}

// ── /v1/proof/receipt and /v1/proof/receipt/:id ───────────────────────────────

#[tokio::test]
async fn proof_receipt_returns_404_before_any_planner_op() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = get(router, "/v1/proof/receipt").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "expected 404 before any planner op: {body}"
    );
}

#[tokio::test]
async fn proof_receipt_by_id_returns_404_for_unknown_id() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, _) = get(router, "/v1/proof/receipt/no-such-id").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
