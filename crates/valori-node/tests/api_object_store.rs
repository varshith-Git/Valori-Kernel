// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! End-to-end proof that automatic snapshots work once VALORI_OBJECT_STORE_URL
//! is set — insert vectors, trigger the same POST the scheduled backup sweep
//! calls, and confirm the snapshot shows up in the object store. Uses a
//! file:// backend as the S3 stand-in (same equivalence
//! valori-storage::object_store's own module doc comment draws) — no
//! network/credentials needed, but it's the identical opendal code path a
//! real s3:// backend would take.
//!
//! Sets VALORI_OBJECT_STORE_URL as a process env var, which `Engine::new()`
//! reads at construction time (see valori-node's engine.rs). Kept to a
//! single test in this file to avoid racing that global with itself.

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
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!(null));
    (status, json)
}

async fn get_json(router: axum::Router, uri: &str) -> (StatusCode, Value) {
    let resp = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!(null));
    (status, json)
}

#[tokio::test]
async fn insert_vectors_then_upload_lands_a_snapshot_in_object_store() {
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: single-threaded w.r.t. this env var — no other test in this
    // binary reads/writes VALORI_OBJECT_STORE_URL.
    unsafe {
        std::env::set_var("VALORI_OBJECT_STORE_URL", format!("file://{}", dir.path().display()));
    }

    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 100;
    cfg.max_nodes = 50;
    cfg.max_edges = 50;

    let engine = Engine::new(&cfg);
    assert!(
        engine.object_store.is_some(),
        "Engine::new should have picked up VALORI_OBJECT_STORE_URL"
    );
    let shared: SharedEngine = Arc::new(RwLock::new(engine));

    // Insert a few vectors — the "insert vectors" step of the requested test.
    for i in 0..5u32 {
        let router = build_router(shared.clone(), None, None);
        let (status, body) = post_json(
            router,
            "/records",
            serde_json::json!({"values": [i as f32, 1.0, 2.0, 3.0]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "insert {i} failed: {body}");
    }

    // Trigger the same endpoint the scheduled backup sweep calls.
    let router = build_router(shared.clone(), None, None);
    let (status, body) = post_json(router, "/v1/storage/snapshots/upload", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK, "snapshot upload failed: {body}");
    let key = body["key"].as_str().expect("missing key in response").to_string();
    assert!(body["size_bytes"].as_u64().unwrap_or(0) > 0);

    // "Snapshot appears in S3" — confirmed two ways: via the API...
    let router = build_router(shared.clone(), None, None);
    let (status, list_body) = get_json(router, "/v1/storage/snapshots").await;
    assert_eq!(status, StatusCode::OK);
    let snapshots = list_body["snapshots"].as_array().expect("missing snapshots array");
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0]["key"], key);

    // ...and directly on disk (the file:// stand-in for the S3 bucket).
    let snap_path = dir.path().join(&key);
    assert!(snap_path.exists(), "expected snapshot file at {snap_path:?}");
    assert!(std::fs::metadata(&snap_path).unwrap().len() > 0);

    // manifest.json is the new entry point — the upload above must have
    // written it, naming this exact snapshot as current.
    let router = build_router(shared.clone(), None, None);
    let (status, manifest_body) = get_json(router, "/v1/storage/manifest").await;
    assert_eq!(status, StatusCode::OK);
    let manifest = &manifest_body["manifest"];
    assert!(!manifest.is_null(), "manifest.json should exist after an upload");
    assert_eq!(manifest["current_snapshot"]["key"], key);
    assert_eq!(manifest["schema_version"].as_u64(), Some(1));
    assert!(manifest["node_version"].as_str().unwrap_or("").len() > 0);

    let manifest_path = dir.path().join("manifest.json");
    assert!(manifest_path.exists(), "expected manifest.json at {manifest_path:?}");

    // Restore with NO key given — must resolve via manifest.json alone.
    let router = build_router(shared.clone(), None, None);
    let (status, restore_body) = post_json(router, "/v1/storage/snapshots/restore", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK, "manifest-driven restore failed: {restore_body}");
    assert_eq!(restore_body["key"].as_str(), Some(key.as_str()));

    unsafe {
        std::env::remove_var("VALORI_OBJECT_STORE_URL");
    }
}
