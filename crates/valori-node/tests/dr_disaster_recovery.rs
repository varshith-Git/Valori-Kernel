// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Mandatory disaster-recovery test (Layer 2.16 follow-up — object storage
//! is wired into provisioning and verified at startup; this proves the loop
//! it exists to close actually works):
//!
//!   insert 10,000 vectors -> delete container -> deploy new one -> restore
//!   -> search works -> checksums identical
//!
//! "Delete container" is simulated by dropping the original `Engine` (no
//! `snapshot_path`/`event_log_path` configured, so nothing survives on
//! "local disk" either — the ONLY thing that outlives it is whatever got
//! uploaded to the object store, exactly like a real container getting
//! destroyed and only its object-store durability tier surviving).
//! "Deploy new one" is a brand new `Engine` from scratch, pointed at the
//! same bucket (a fresh `file://` dir standing in for S3 — see
//! valori-storage::object_store's own module doc comment for that
//! equivalence). "Restore" calls the exact `POST /v1/storage/snapshots/
//! restore` endpoint the control plane's `backup/mod.rs` calls for a real
//! recovery.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

use valori_node::config::NodeConfig;
use valori_node::engine::Engine;
use valori_node::server::{build_router, SharedEngine};
use valori_node::EngineFromNodeConfig;

const DIM: usize = 8;
const TOTAL: usize = 10_000;
const BATCH: usize = 1_000;

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
    let bytes = axum::body::to_bytes(resp.into_body(), 8 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, json)
}

async fn get_json(router: axum::Router, uri: &str) -> (StatusCode, Value) {
    let resp = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 8 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, json)
}

/// Deterministic vector for index `i`: first component is `i` itself (so a
/// query for the exact vector has a unique, obvious nearest neighbour),
/// remaining components are a fixed fingerprint of `i` — enough entropy that
/// two different indices never collide, without needing real embeddings.
fn vector_for(i: usize) -> Vec<f32> {
    let mut v = vec![i as f32];
    for c in 1..DIM {
        v.push(((i * (c + 7)) % 997) as f32);
    }
    v
}

fn dr_cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.max_records = TOTAL + 1_000;
    cfg.max_nodes = 100;
    cfg.max_edges = 100;
    // Deliberately no snapshot_path / event_log_path / wal_path: "delete
    // container" must actually lose everything except what made it to the
    // object store, or this test would prove nothing.
    cfg
}

#[tokio::test]
async fn ten_thousand_vectors_survive_container_loss_via_object_store() {
    let bucket_dir = tempfile::tempdir().unwrap();
    // SAFETY: sole test in this binary touching this env var.
    unsafe {
        std::env::set_var(
            "VALORI_OBJECT_STORE_URL",
            format!("file://{}", bucket_dir.path().display()),
        );
    }

    // ── "Insert 10,000 vectors" ─────────────────────────────────────────
    let original_engine = Engine::new(&dr_cfg());
    assert!(original_engine.object_store.is_some());
    let original: SharedEngine = Arc::new(RwLock::new(original_engine));

    for chunk_start in (0..TOTAL).step_by(BATCH) {
        let batch: Vec<Vec<f32>> = (chunk_start..chunk_start + BATCH).map(vector_for).collect();
        let router = build_router(original.clone(), None, None);
        let (status, body) = post_json(
            router,
            "/v1/vectors/batch_insert",
            json!({ "batch": batch }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "batch insert at {chunk_start} failed: {body}");
    }

    let router = build_router(original.clone(), None, None);
    let (status, health) = get_json(router, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["records"]["live"].as_u64(), Some(TOTAL as u64));

    // Checksum BEFORE destruction.
    let router = build_router(original.clone(), None, None);
    let (status, proof) = get_json(router, "/v1/proof/state").await;
    assert_eq!(status, StatusCode::OK);
    let checksum_before = proof["final_state_hash"].as_str().unwrap().to_string();
    assert_eq!(checksum_before.len(), 64, "expected a 32-byte hex BLAKE3 hash");

    // Snapshot to object store — this is what the scheduled backup sweep
    // (and any pre-destroy final-snapshot step) calls in production.
    let router = build_router(original.clone(), None, None);
    let (status, upload) = post_json(router, "/v1/storage/snapshots/upload", json!({})).await;
    assert_eq!(status, StatusCode::OK, "snapshot upload failed: {upload}");
    let snapshot_key = upload["key"].as_str().unwrap().to_string();
    assert_eq!(upload["state_hash"].as_str().unwrap(), checksum_before);

    // ── "Delete container" ──────────────────────────────────────────────
    // Drop every reference to the original engine. No snapshot_path/
    // event_log_path was configured, so this is the whole story — there is
    // nothing left anywhere except bucket_dir (the object store).
    drop(original);

    // ── "Deploy new one" ────────────────────────────────────────────────
    // Fresh engine, empty state, same config a redeployed container would
    // get (same VALORI_DIM/VALORI_MAX_RECORDS, same VALORI_OBJECT_STORE_URL
    // still set from above — same bucket, new container).
    let restored_engine = Engine::new(&dr_cfg());
    let restored: SharedEngine = Arc::new(RwLock::new(restored_engine));

    let router = build_router(restored.clone(), None, None);
    let (status, health) = get_json(router, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        health["records"]["live"].as_u64(),
        Some(0),
        "fresh container must start empty before restore"
    );

    // ── "Restore" ────────────────────────────────────────────────────────
    let router = build_router(restored.clone(), None, None);
    let (status, restore_resp) = post_json(
        router,
        "/v1/storage/snapshots/restore",
        json!({ "key": snapshot_key }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "restore failed: {restore_resp}");
    let checksum_after = restore_resp["state_hash"].as_str().unwrap().to_string();

    let router = build_router(restored.clone(), None, None);
    let (status, health) = get_json(router, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["records"]["live"].as_u64(), Some(TOTAL as u64));

    // ── "Search works" ──────────────────────────────────────────────────
    for &i in &[0usize, 1, 4999, 9999] {
        let router = build_router(restored.clone(), None, None);
        let (status, search) = post_json(
            router,
            "/search",
            json!({ "query": vector_for(i), "k": 1, "rerank": false }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "search for index {i} failed: {search}");
        let hits = search["results"].as_array().unwrap();
        assert_eq!(hits.len(), 1, "expected exactly one hit for index {i}");
        assert_eq!(
            hits[0]["score"].as_f64().unwrap(),
            0.0,
            "exact match for index {i} should have zero distance"
        );
    }

    // ── "Checksums identical" ───────────────────────────────────────────
    assert_eq!(
        checksum_before, checksum_after,
        "post-restore BLAKE3 state hash must exactly match the pre-destruction hash"
    );

    unsafe {
        std::env::remove_var("VALORI_OBJECT_STORE_URL");
    }
}
