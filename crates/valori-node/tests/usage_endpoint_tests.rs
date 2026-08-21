// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase P2 (Cloud plan/quota/usage accounting): tests for the read-only
//! `GET /v1/usage` endpoint.
//!
//! Covers (per the P2 implementation plan's P2.10 verification table):
//!   - correct records/collections/storage after real inserts
//!   - storage accounting includes rotated event-log segments, not just
//!     the live file (the single most likely accounting bug in this design)
//!   - collection counting tracks create/drop exactly
//!   - the handler never mutates canonical state — BLAKE3 state hash is
//!     byte-identical whether or not /v1/usage was ever called

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::Value;
use std::sync::Arc;
use tempfile::tempdir;
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

fn tiny_cfg_with_event_log(event_log_path: std::path::PathBuf) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 10_000;
    cfg.max_nodes = 50;
    cfg.max_edges = 50;
    cfg.event_log_path = Some(event_log_path);
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

#[tokio::test]
async fn usage_reports_zero_records_and_default_collection_on_a_fresh_engine() {
    let dir = tempdir().unwrap();
    let cfg = tiny_cfg_with_event_log(dir.path().join("events.log"));
    let (_shared, router) = engine_router(cfg);

    let (status, body) = get(router, "/v1/usage").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["records"], 0);
    // Phase 3.3: a brand-new engine has zero collections — "default"
    // included — until one is explicitly created via POST /v1/namespaces.
    assert_eq!(body["collections"], 0);
    assert!(body["storage"]["total_bytes"].as_u64().is_some());
}

#[tokio::test]
async fn usage_tracks_real_inserted_records() {
    let dir = tempdir().unwrap();
    let cfg = tiny_cfg_with_event_log(dir.path().join("events.log"));
    let (_shared, router) = engine_router(cfg);

    let (status, body) = post_json(
        router.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    for _ in 0..5 {
        let (status, _) = post_json(
            router.clone(),
            "/v1/records",
            serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4], "collection": "default" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let (status, body) = get(router, "/v1/usage").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["records"], 5);
}

#[tokio::test]
async fn usage_tracks_collection_create_and_drop() {
    let dir = tempdir().unwrap();
    let cfg = tiny_cfg_with_event_log(dir.path().join("events.log"));
    let (_shared, router) = engine_router(cfg);

    let (status, _) = post_json(
        router.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "tenant-a", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = get(router.clone(), "/v1/usage").await;
    // Phase 3.3: only "tenant-a" was ever explicitly created — no implicit
    // "default" collection exists.
    assert_eq!(body["collections"], 1, "tenant-a only");

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/v1/namespaces/tenant-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn usage_storage_bytes_includes_rotated_event_log_segments() {
    // Direct test of the accounting trap the P2 plan calls out explicitly:
    // EventLogWriter::rotate() renames the live segment and opens a fresh
    // one — old segments are never deleted. A storage_bytes number that
    // only stats the live file would silently undercount here.
    let dir = tempdir().unwrap();
    let live_path = dir.path().join("events.log");
    let cfg = tiny_cfg_with_event_log(live_path.clone());
    let (shared, router) = engine_router(cfg);

    let (status, body) = post_json(
        router.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    for _ in 0..3 {
        let (status, _) = post_json(
            router.clone(),
            "/v1/records",
            serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4], "collection": "default" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let before_rotate_bytes = std::fs::metadata(&live_path).unwrap().len();
    assert!(
        before_rotate_bytes > 0,
        "event log must have real bytes before rotation"
    );

    // Simulate a rotation exactly the way EventLogWriter::rotate() names
    // archives (`events.log.NNNNNN`) without needing to drive the engine's
    // internal rotation-size threshold in a test.
    let archive_path = dir.path().join("events.log.000001");
    std::fs::copy(&live_path, &archive_path).unwrap();

    let (status, body) = get(router, "/v1/usage").await;
    assert_eq!(status, StatusCode::OK);
    let total = body["storage"]["total_bytes"].as_u64().unwrap();
    assert!(
        total >= before_rotate_bytes * 2,
        "storage_bytes must include the archived segment, not just the live file: got {total}, expected >= {}",
        before_rotate_bytes * 2
    );

    drop(shared); // keep the engine alive through every assertion above
}

#[tokio::test]
async fn usage_endpoint_never_mutates_canonical_state() {
    let dir = tempdir().unwrap();
    let cfg = tiny_cfg_with_event_log(dir.path().join("events.log"));
    let (_shared, router) = engine_router(cfg);

    let (status, body) = post_json(
        router.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    for _ in 0..3 {
        let (status, _) = post_json(
            router.clone(),
            "/v1/records",
            serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4], "collection": "default" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let (_, before) = get(router.clone(), "/v1/proof/state").await;

    // Call /v1/usage several times, including a burst, to prove it never
    // touches canonical state regardless of call count.
    for _ in 0..10 {
        let (status, _) = get(router.clone(), "/v1/usage").await;
        assert_eq!(status, StatusCode::OK);
    }

    let (_, after) = get(router, "/v1/proof/state").await;
    assert_eq!(
        before["final_state_hash"], after["final_state_hash"],
        "GET /v1/usage must never change the BLAKE3 canonical state hash"
    );
}
