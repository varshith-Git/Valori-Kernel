// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! HTTP tests for Tree-RAG endpoints:
//!   POST /v1/tree/build         — parse markdown → TreeIndex + cache_key
//!   POST /v1/tree/query         — query a tree (by value or cache_key)
//!   POST /v1/tree/verify        — replay a receipt against a tree
//!   POST /v1/tree/chain-verify  — verify an ordered receipt chain
//!   POST /v1/tree/hybrid        — tree + vector combined search

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

const SAMPLE_MD: &str = r#"# Introduction

This is the intro paragraph with some content.

## Section One

Details about section one.

### Subsection A

More details here.

## Section Two

Details about section two.
"#;

// ── /v1/tree/build ────────────────────────────────────────────────────────────

#[tokio::test]
async fn tree_build_returns_cache_key_and_node_count() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = post_json(
        router,
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let cache_key = body["cache_key"].as_str().expect("missing cache_key");
    assert_eq!(cache_key.len(), 64, "cache_key must be 64-char BLAKE3 hex");
    assert!(
        body["node_count"].as_u64().unwrap_or(0) > 0,
        "node_count must be positive"
    );
    assert!(
        body["tree"].is_object(),
        "response must include tree object"
    );
}

#[tokio::test]
async fn tree_build_with_doc_name() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = post_json(
        router,
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD, "doc_name": "my-doc"}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["doc_name"].as_str().unwrap(), "my-doc");
}

// ── /v1/tree/query ────────────────────────────────────────────────────────────

#[tokio::test]
async fn tree_query_with_inline_tree() {
    let (_, router) = engine_router(tiny_cfg());

    // First build to get a tree
    let (_, build_body) = post_json(
        router.clone(),
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD}),
    )
    .await;
    let tree = &build_body["tree"];

    let (status, body) = post_json(
        router,
        "/v1/tree/query",
        serde_json::json!({"tree": tree, "query": "section one details", "k": 2}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["answer"].is_string() || body["citations"].is_array(),
        "response must include answer or citations: {body}"
    );
}

#[tokio::test]
async fn tree_query_with_cache_key() {
    let (_, router) = engine_router(tiny_cfg());

    let (_, build_body) = post_json(
        router.clone(),
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD}),
    )
    .await;
    let cache_key = build_body["cache_key"].as_str().unwrap().to_string();

    let (status, body) = post_json(
        router,
        "/v1/tree/query",
        serde_json::json!({"cache_key": cache_key, "query": "subsection details"}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn tree_query_chained_receipts_have_prev_hash() {
    let (_, router) = engine_router(tiny_cfg());

    let (_, build_body) = post_json(
        router.clone(),
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD}),
    )
    .await;
    let cache_key = build_body["cache_key"].as_str().unwrap().to_string();

    let (_, first) = post_json(
        router.clone(),
        "/v1/tree/query",
        serde_json::json!({"cache_key": cache_key, "query": "introduction"}),
    )
    .await;

    let first_receipt_hash = first["receipt"]["receipt_hash"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        !first_receipt_hash.is_empty(),
        "first query must return a receipt hash"
    );

    // Second query passes prev_hash — chain links
    let (status, second) = post_json(
        router,
        "/v1/tree/query",
        serde_json::json!({
            "cache_key": cache_key,
            "query": "section two",
            "prev_hash": first_receipt_hash
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let prev = second["receipt"]["prev_hash"].as_str().unwrap_or("");
    assert_eq!(
        prev, first_receipt_hash,
        "receipt.prev_hash must equal the hash we passed"
    );
}

// ── /v1/tree/verify ───────────────────────────────────────────────────────────

#[tokio::test]
async fn tree_verify_valid_receipt() {
    let (_, router) = engine_router(tiny_cfg());

    let (_, build_body) = post_json(
        router.clone(),
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD}),
    )
    .await;
    let tree = &build_body["tree"];

    // Query to get a receipt
    let (_, query_body) = post_json(
        router.clone(),
        "/v1/tree/query",
        serde_json::json!({"tree": tree, "query": "section one"}),
    )
    .await;
    let receipt = &query_body["receipt"];
    assert!(receipt.is_object(), "query must return a receipt");

    let (status, body) = post_json(
        router,
        "/v1/tree/verify",
        serde_json::json!({"tree": tree, "receipt": receipt}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["valid"].as_bool().unwrap(),
        true,
        "receipt must be valid"
    );
}

#[tokio::test]
async fn tree_verify_tampered_receipt_is_invalid() {
    let (_, router) = engine_router(tiny_cfg());

    let (_, build_body) = post_json(
        router.clone(),
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD}),
    )
    .await;
    let tree = &build_body["tree"];

    let (_, query_body) = post_json(
        router.clone(),
        "/v1/tree/query",
        serde_json::json!({"tree": tree, "query": "introduction"}),
    )
    .await;

    // Tamper: corrupt the evidence_hash (this is what verify_receipt checks)
    let mut receipt = query_body["receipt"].clone();
    receipt["evidence_hash"] =
        serde_json::json!("0000000000000000000000000000000000000000000000000000000000000000");

    let (status, body) = post_json(
        router,
        "/v1/tree/verify",
        serde_json::json!({"tree": tree, "receipt": receipt}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["valid"].as_bool().unwrap(),
        false,
        "tampered receipt must be invalid"
    );
}

// ── /v1/tree/chain-verify ─────────────────────────────────────────────────────

#[tokio::test]
async fn tree_chain_verify_empty_is_valid() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = post_json(
        router,
        "/v1/tree/chain-verify",
        serde_json::json!({"receipts": []}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["valid"].as_bool().unwrap(), true);
    assert!(body["broken_at"].is_null());
}

#[tokio::test]
async fn tree_chain_verify_linked_chain_is_valid() {
    let (_, router) = engine_router(tiny_cfg());

    let (_, build_body) = post_json(
        router.clone(),
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD}),
    )
    .await;
    let cache_key = build_body["cache_key"].as_str().unwrap().to_string();

    let (_, r1) = post_json(
        router.clone(),
        "/v1/tree/query",
        serde_json::json!({"cache_key": cache_key, "query": "intro"}),
    )
    .await;
    let h1 = r1["receipt"]["receipt_hash"].as_str().unwrap().to_string();

    let (_, r2) = post_json(
        router.clone(),
        "/v1/tree/query",
        serde_json::json!({"cache_key": cache_key, "query": "section", "prev_hash": h1}),
    )
    .await;

    let receipt1 = &r1["receipt"];
    let receipt2 = &r2["receipt"];

    let (status, body) = post_json(
        router,
        "/v1/tree/chain-verify",
        serde_json::json!({"receipts": [receipt1, receipt2]}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["valid"].as_bool().unwrap(),
        true,
        "linked chain must be valid: {body}"
    );
}

#[tokio::test]
async fn tree_chain_verify_broken_chain_reports_index() {
    let (_, router) = engine_router(tiny_cfg());

    let (_, build_body) = post_json(
        router.clone(),
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD}),
    )
    .await;
    let cache_key = build_body["cache_key"].as_str().unwrap().to_string();

    let (_, r1) = post_json(
        router.clone(),
        "/v1/tree/query",
        serde_json::json!({"cache_key": cache_key, "query": "intro"}),
    )
    .await;
    let (_, r2) = post_json(
        router.clone(),
        "/v1/tree/query",
        serde_json::json!({"cache_key": cache_key, "query": "section one"}),
    )
    .await;

    // r2 was NOT linked to r1 (no prev_hash), so the chain is broken at index 1
    let receipt1 = &r1["receipt"];
    let receipt2 = &r2["receipt"];

    let (status, body) = post_json(
        router,
        "/v1/tree/chain-verify",
        serde_json::json!({"receipts": [receipt1, receipt2]}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["valid"].as_bool().unwrap(), false);
    assert_eq!(body["broken_at"].as_u64().unwrap(), 1);
}

// ── /v1/tree/hybrid ───────────────────────────────────────────────────────────

#[tokio::test]
async fn tree_hybrid_with_text_returns_hits() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = post_json(
        router,
        "/v1/tree/hybrid",
        serde_json::json!({
            "text": SAMPLE_MD,
            "query": "section one",
            "k": 2
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let hits = body["hits"].as_array().expect("missing hits");
    assert!(!hits.is_empty(), "hybrid must return at least one hit");
    assert!(body["tree_hit_count"].as_u64().is_some());
}

#[tokio::test]
async fn tree_hybrid_with_cache_key() {
    let (_, router) = engine_router(tiny_cfg());

    let (_, build_body) = post_json(
        router.clone(),
        "/v1/tree/build",
        serde_json::json!({"text": SAMPLE_MD}),
    )
    .await;
    let cache_key = build_body["cache_key"].as_str().unwrap().to_string();

    let (status, body) = post_json(
        router,
        "/v1/tree/hybrid",
        serde_json::json!({
            "cache_key": cache_key,
            "query": "subsection details",
            "k": 3
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["hits"].is_array());
}
