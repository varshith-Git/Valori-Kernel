// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Integration tests for Phase 3.6 — Crypto-shredding (GDPR erasure).

use axum::{body::Body, http::{Request, StatusCode}};
use tower::ServiceExt;
use serde_json::{json, Value};
use base64::Engine as _;
use std::sync::Arc;
use tokio::sync::RwLock;

use valori_node::{config::NodeConfig, engine::Engine, server::build_router};
use valori_node::EngineFromNodeConfig;

fn make_cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 100;
    cfg
}

fn make_app() -> axum::Router {
    let cfg = make_cfg();
    let engine = Arc::new(RwLock::new(Engine::new(&cfg)));
    build_router(engine, None, None)
}

async fn post_json(app: &axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn delete_json(app: &axum::Router, path: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("DELETE")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn get_json(app: &axum::Router, path: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

// Helper: insert a plain record to prime the dim
async fn prime_dim(app: &axum::Router) {
    post_json(app, "/records", json!({"values": [0.1, 0.2, 0.3, 0.4]})).await;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_insert_encrypted_returns_key_id() {
    let app = make_app();
    prime_dim(&app).await;

    let payload = base64::engine::general_purpose::STANDARD.encode(b"PII: john.doe@example.com");
    let (status, body) = post_json(&app, "/v1/records/encrypted", json!({ "payload": payload, "tag": 1 })).await;

    assert_eq!(status, StatusCode::CREATED, "encrypted insert should return 201, got: {body}");
    assert!(body["id"].as_u64().is_some(), "response must contain id");
    let key_id = body["key_id"].as_str().expect("response must contain key_id");
    assert_eq!(key_id.len(), 32, "key_id must be 32 hex chars, got: {key_id}");
}

#[tokio::test]
async fn test_shred_key_makes_status_return_false() {
    let app = make_app();
    prime_dim(&app).await;

    let payload = base64::engine::general_purpose::STANDARD.encode(b"secret data");
    let (ins_status, ins_body) = post_json(&app, "/v1/records/encrypted", json!({ "payload": payload })).await;
    assert_eq!(ins_status, StatusCode::CREATED, "insert_encrypted: {ins_body}");
    let key_id = ins_body["key_id"].as_str().unwrap().to_owned();

    // Key should exist before shredding
    let (s, b) = get_json(&app, &format!("/v1/crypto/status/{key_id}")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b["exists"].as_bool().unwrap_or(false), "key should exist before shred");

    // Shred the key
    let (shred_status, shred_body) = delete_json(&app, &format!("/v1/crypto/shred/{key_id}")).await;
    assert_eq!(shred_status, StatusCode::OK, "shred: {shred_body}");
    assert!(shred_body["shredded"].as_bool().unwrap_or(false));

    // Key should no longer exist
    let (s2, b2) = get_json(&app, &format!("/v1/crypto/status/{key_id}")).await;
    assert_eq!(s2, StatusCode::OK);
    assert!(!b2["exists"].as_bool().unwrap_or(true), "key should be gone after shred, got: {b2}");
}

#[tokio::test]
async fn test_encrypted_record_not_in_search_results() {
    let app = make_app();

    // Insert a searchable record at [1,0,0,0]
    let (_, plain_body) = post_json(&app, "/records", json!({"values": [1.0, 0.0, 0.0, 0.0]})).await;
    let plain_id = plain_body["id"].as_u64().unwrap_or(99);

    // Insert an encrypted record (stored as zero vector internally)
    let payload = base64::engine::general_purpose::STANDARD.encode(b"PII data");
    post_json(&app, "/v1/records/encrypted", json!({ "payload": payload })).await;

    // Search with k=5 — zero vector (encrypted record) must not rank #1
    let (s, results) = post_json(&app, "/search", json!({"query": [1.0, 0.0, 0.0, 0.0], "k": 5})).await;
    assert_eq!(s, StatusCode::OK);
    let hits = results["results"].as_array().expect("results must be array");
    assert!(!hits.is_empty(), "search returned no hits");
    assert_eq!(
        hits[0]["id"].as_u64().unwrap_or(99), plain_id,
        "non-encrypted record must rank first, got: {hits:?}"
    );
}

#[tokio::test]
async fn test_bad_key_id_format_returns_400() {
    let app = make_app();
    let (status, _) = delete_json(&app, "/v1/crypto/shred/not-a-valid-hex-key").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_encrypt_two_records_under_same_key_then_shred() {
    let app = make_app();
    prime_dim(&app).await;

    let key_id = "aabbccddeeff00112233445566778899";
    let payload = base64::engine::general_purpose::STANDARD.encode(b"grouped PII");

    let (r1_s, r1_b) = post_json(&app, "/v1/records/encrypted", json!({ "payload": payload, "key_id": key_id })).await;
    assert_eq!(r1_s, StatusCode::CREATED, "r1: {r1_b}");
    assert_eq!(r1_b["key_id"].as_str().unwrap(), key_id);

    let (r2_s, r2_b) = post_json(&app, "/v1/records/encrypted", json!({ "payload": payload, "key_id": key_id })).await;
    assert_eq!(r2_s, StatusCode::CREATED, "r2: {r2_b}");

    // Shred the shared key
    let (shred_s, shred_b) = delete_json(&app, &format!("/v1/crypto/shred/{key_id}")).await;
    assert_eq!(shred_s, StatusCode::OK, "shred: {shred_b}");

    // Key gone
    let (check_s, check_b) = get_json(&app, &format!("/v1/crypto/status/{key_id}")).await;
    assert_eq!(check_s, StatusCode::OK);
    assert!(!check_b["exists"].as_bool().unwrap_or(true));

    // Both record ids were allocated
    let _ = r1_b["id"].as_u64().unwrap();
    let _ = r2_b["id"].as_u64().unwrap();
}
