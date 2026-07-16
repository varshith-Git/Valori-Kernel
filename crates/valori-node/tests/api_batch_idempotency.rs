// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 3.12 — Batch insert per-item idempotency tests.

use valori_node::config::NodeConfig;
use valori_node::EngineFromNodeConfig;
use valori_node::server::build_router;
use valori_node::engine::Engine;
use valori_node::api::{BatchInsertRequest, BatchInsertResponse};
use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;
use std::sync::Arc;
use tokio::sync::RwLock;

const DIM: usize = 4;

fn make_shared() -> Arc<RwLock<Engine>> {
    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.max_records = 100;
    cfg.max_nodes = 64;
    cfg.max_edges = 64;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    Arc::new(RwLock::new(Engine::new(&cfg)))
}

async fn post_batch(
    shared: Arc<RwLock<Engine>>,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared, None, None);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/vectors/batch_insert")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// Submitting the same request_id twice returns the same record ID.
#[tokio::test]
async fn idempotent_insert_returns_same_id() {
    let shared = make_shared();
    let rid = "aabbccddeeff00112233445566778899";

    let body = serde_json::json!({
        "batch": [[0.1, 0.2, 0.3, 0.4]],
        "request_ids": [rid]
    });

    let (s1, r1) = post_batch(shared.clone(), body.clone()).await;
    assert_eq!(s1, StatusCode::OK);
    let id1 = r1["ids"][0].as_u64().unwrap();

    let (s2, r2) = post_batch(shared.clone(), body).await;
    assert_eq!(s2, StatusCode::OK);
    let id2 = r2["ids"][0].as_u64().unwrap();

    assert_eq!(id1, id2, "duplicate request_id must return the same record ID");
}

/// A duplicate item in a mixed batch is skipped; new items still get new IDs.
#[tokio::test]
async fn mixed_batch_dedup_and_new() {
    let shared = make_shared();
    let rid_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let rid_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    // First insert: two items with request_ids.
    let (_, r1) = post_batch(shared.clone(), serde_json::json!({
        "batch": [[0.1, 0.2, 0.3, 0.4], [0.5, 0.6, 0.7, 0.8]],
        "request_ids": [rid_a, rid_b]
    })).await;
    let id_a = r1["ids"][0].as_u64().unwrap();
    let id_b = r1["ids"][1].as_u64().unwrap();

    // Second insert: rid_a is duplicate, rid_b is new, third has no key.
    let (s2, r2) = post_batch(shared.clone(), serde_json::json!({
        "batch": [[0.1, 0.2, 0.3, 0.4], [0.9, 0.9, 0.9, 0.9]],
        "request_ids": [rid_a, null]
    })).await;
    assert_eq!(s2, StatusCode::OK);
    // rid_a deduped → same ID as before.
    assert_eq!(r2["ids"][0].as_u64().unwrap(), id_a);
    // new item gets a fresh ID.
    let id_new = r2["ids"][1].as_u64().unwrap();
    assert!(id_new > id_b, "new item should get a higher ID");
}

/// Omitting request_ids entirely still works (backward compat).
#[tokio::test]
async fn batch_without_request_ids_still_works() {
    let shared = make_shared();
    let body = serde_json::json!({
        "batch": [[0.1, 0.2, 0.3, 0.4], [0.5, 0.6, 0.7, 0.8]]
    });
    let (s, r) = post_batch(shared, body).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["ids"].as_array().unwrap().len(), 2);
}

/// A batch with all items deduped returns all previously assigned IDs and
/// does not mutate state (record count stays the same).
#[tokio::test]
async fn fully_deduped_batch_does_not_grow_record_count() {
    let shared = make_shared();
    let rids: Vec<String> = (0..3u8)
        .map(|i| format!("{:032x}", i))
        .collect();

    let batch_body = serde_json::json!({
        "batch": [[0.1, 0.2, 0.3, 0.4], [0.5, 0.6, 0.7, 0.8], [0.9, 0.1, 0.2, 0.3]],
        "request_ids": [rids[0], rids[1], rids[2]]
    });

    let (_, r1) = post_batch(shared.clone(), batch_body.clone()).await;
    let ids1: Vec<u64> = r1["ids"].as_array().unwrap()
        .iter().map(|v| v.as_u64().unwrap()).collect();

    // Replay entire batch — all three items are deduped.
    let (s2, r2) = post_batch(shared.clone(), batch_body).await;
    assert_eq!(s2, StatusCode::OK);
    let ids2: Vec<u64> = r2["ids"].as_array().unwrap()
        .iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(ids1, ids2);

    // State should not have grown — record count stays at 3.
    let count = shared.read().await.record_count();
    assert_eq!(count, 3, "fully deduped batch must not insert new records");
}
