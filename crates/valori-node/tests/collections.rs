// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Collections seam (multi-node roadmap Phase 1.4).
//!
//! Exactly one collection ("default") exists today, but every data-path
//! request accepts the field NOW so clients written today survive the
//! arrival of multi-collection (shard-by-collection, Phase 4) unchanged.

use valori_node::api::{validate_collection, DEFAULT_COLLECTION};
use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::Engine;
use valori_node::server::{build_router, SharedEngine};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 64;
    cfg.max_nodes = 8;
    cfg.max_edges = 16;
    cfg.index_kind = IndexKind::BruteForce;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    cfg.snapshot_path = None;
    cfg
}

fn make_shared() -> SharedEngine {
    Arc::new(Mutex::new(Engine::new(&cfg())))
}

async fn post(app: axum::Router, uri: &str, body: serde_json::Value) -> StatusCode {
    app.oneshot(
        Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    )
    .await
    .unwrap()
    .status()
}

// ── Unit-level validation ─────────────────────────────────────────────────────

#[test]
fn default_collection_is_named_default() {
    assert_eq!(DEFAULT_COLLECTION, "default");
}

#[test]
fn validation_accepts_none_and_default() {
    assert!(validate_collection(None).is_ok());
    assert!(validate_collection(Some("default")).is_ok());
}

#[test]
fn validation_rejects_unknown_collections() {
    let err = validate_collection(Some("tenant-42")).unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("tenant-42"), "error must name the collection: {msg}");
}

// ── HTTP-level behavior ───────────────────────────────────────────────────────

#[tokio::test]
async fn insert_without_collection_works() {
    let app = build_router(make_shared(), None);
    let status = post(app, "/records", serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4] })).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn insert_with_default_collection_works() {
    let app = build_router(make_shared(), None);
    let status = post(
        app,
        "/records",
        serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4], "collection": "default" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn insert_with_unknown_collection_is_400() {
    let app = build_router(make_shared(), None);
    let status = post(
        app,
        "/records",
        serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4], "collection": "tenant-42" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_respects_collection_field() {
    let shared = make_shared();
    {
        let mut engine = shared.lock().await;
        engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
    }
    let app = build_router(shared.clone(), None);
    let ok = post(
        app,
        "/search",
        serde_json::json!({ "query": [0.1, 0.2, 0.3, 0.4], "k": 1, "collection": "default" }),
    )
    .await;
    assert_eq!(ok, StatusCode::OK);

    let app = build_router(shared, None);
    let bad = post(
        app,
        "/search",
        serde_json::json!({ "query": [0.1, 0.2, 0.3, 0.4], "k": 1, "collection": "other" }),
    )
    .await;
    assert_eq!(bad, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rejected_collection_does_not_mutate_state() {
    let shared = make_shared();
    let app = build_router(shared.clone(), None);
    let status = post(
        app,
        "/records",
        serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4], "collection": "nope" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let engine = shared.lock().await;
    assert_eq!(engine.state.record_count(), 0, "rejected request must not insert");
}
