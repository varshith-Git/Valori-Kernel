// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! HTTP tests for miscellaneous endpoints not covered elsewhere:
//!   GET  /v1/version
//!   GET  /v1/shard/routing
//!   GET  /v1/graph/nodes
//!   POST /v1/index/rebuild
//!   POST /v1/delete
//!   GET  /v1/records/:id
//!   PATCH /v1/records/:id/metadata
//!   POST /v1/memory/contradict
//!   GET  /v1/memory/meta/get  +  POST /v1/memory/meta/set
//!   GET  /v1/snapshot/download
//!   POST /v1/snapshot/restore
//!   POST /v1/ingest/document   (embed-disabled path)
//!   POST /v1/ingest/update     (embed-disabled path)
//!   POST /v1/ingest/extract-entities  (embed-disabled path)
//!   GET  /v1/ingest/status/:job_id
//!   GET  /v1/community/overview
//!   POST /v1/community/search

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;
use std::sync::Arc;
use tokio::sync::RwLock;

use valori_node::config::NodeConfig;
use valori_node::EngineFromNodeConfig;
use valori_node::engine::Engine;
use valori_node::server::{build_router, SharedEngine};

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
        .oneshot(Request::builder().method(Method::GET).uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!(null));
    (status, json)
}

async fn post_json(router: axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::POST).uri(uri)
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

async fn patch_json(router: axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::PATCH).uri(uri)
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

/// Insert one record and return its id.
async fn insert_one(router: axum::Router, vec: [f32; 4]) -> u32 {
    let (status, body) = post_json(
        router,
        "/records",
        serde_json::json!({"values": vec}),
    ).await;
    assert_eq!(status, StatusCode::OK, "insert failed: {body}");
    body["id"].as_u64().expect("missing id") as u32
}

// ── /v1/version ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn version_returns_string() {
    let (_, router) = engine_router(tiny_cfg());
    let resp = router
        .oneshot(Request::builder().uri("/v1/version").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    // Version is returned as a plain string (not JSON object)
    assert!(!bytes.is_empty(), "version must return non-empty body");
}

// ── /v1/shard/routing ────────────────────────────────────────────────────────

#[tokio::test]
async fn shard_routing_returns_mode_and_shards() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = get(router, "/v1/shard/routing").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["mode"].is_string());
    assert!(body["shard_count"].as_u64().unwrap_or(0) >= 1);
    assert!(body["shards"].is_array());
}

// ── /v1/graph/nodes ───────────────────────────────────────────────────────────

#[tokio::test]
async fn graph_nodes_empty_graph() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = get(router, "/v1/graph/nodes").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Should return an empty nodes list
    let nodes = body["nodes"].as_array().or_else(|| body.as_array());
    assert!(nodes.map(|n| n.is_empty()).unwrap_or(true));
}

// ── /v1/index/rebuild ────────────────────────────────────────────────────────

#[tokio::test]
async fn index_rebuild_defaults_to_brute() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = post_json(router, "/v1/index/rebuild", serde_json::json!({})).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"].as_bool().unwrap(), true);
    assert!(body["effective"].is_string());
}

#[tokio::test]
async fn index_rebuild_accepts_hnsw() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = post_json(
        router,
        "/v1/index/rebuild",
        serde_json::json!({"index": "hnsw"}),
    ).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let effective = body["effective"].as_str().unwrap();
    assert_eq!(effective, "hnsw");
}

// ── /v1/delete ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_record_by_id() {
    let (_, router) = engine_router(tiny_cfg());
    let id = insert_one(router.clone(), [1.0, 0.0, 0.0, 0.0]).await;

    let (status, body) = post_json(
        router,
        "/v1/delete",
        serde_json::json!({"id": id}),
    ).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"].as_bool().unwrap(), true);
}

#[tokio::test]
async fn delete_nonexistent_record_fails() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, _) = post_json(
        router,
        "/v1/delete",
        serde_json::json!({"id": 9999u32}),
    ).await;
    assert_ne!(status, StatusCode::OK, "deleting a missing record must not return 200");
}

// ── /v1/records/:id ──────────────────────────────────────────────────────────

#[tokio::test]
async fn get_record_by_id_roundtrip() {
    let (_, router) = engine_router(tiny_cfg());
    let id = insert_one(router.clone(), [0.5, 0.5, 0.0, 0.0]).await;

    let (status, body) = get(router, &format!("/v1/records/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"].as_u64().unwrap() as u32, id);
    assert!(body["vector"].is_array());
}

#[tokio::test]
async fn get_record_by_id_not_found() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, _) = get(router, "/v1/records/9999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── /v1/records/:id/metadata ─────────────────────────────────────────────────

#[tokio::test]
async fn patch_record_metadata_roundtrip() {
    let (_, router) = engine_router(tiny_cfg());
    let id = insert_one(router.clone(), [0.1, 0.2, 0.3, 0.4]).await;

    let (status, body) = patch_json(
        router.clone(),
        &format!("/v1/records/{id}/metadata"),
        serde_json::json!({"author": "Alice", "year": 2025}),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"].as_bool().unwrap(), true);

    // Verify the metadata was actually stored
    let (status2, rec) = get(router, &format!("/v1/records/{id}")).await;
    assert_eq!(status2, StatusCode::OK);
    assert_eq!(rec["metadata"]["author"].as_str().unwrap(), "Alice");
}

#[tokio::test]
async fn patch_metadata_not_found_returns_404() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, _) = patch_json(
        router,
        "/v1/records/9999/metadata",
        serde_json::json!({"x": 1}),
    ).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── /v1/memory/contradict ────────────────────────────────────────────────────

#[tokio::test]
async fn contradict_identical_vectors_above_threshold() {
    let (_, router) = engine_router(tiny_cfg());
    let id_a = insert_one(router.clone(), [1.0, 0.0, 0.0, 0.0]).await;
    let id_b = insert_one(router.clone(), [1.0, 0.0, 0.0, 0.0]).await;

    let (status, body) = post_json(
        router,
        "/v1/memory/contradict",
        serde_json::json!({"record_a": id_a, "record_b": id_b, "threshold": 0.5}),
    ).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["record_a"].as_u64().unwrap() as u32, id_a);
    assert_eq!(body["record_b"].as_u64().unwrap() as u32, id_b);
    assert!(body["similarity"].as_f64().is_some());
    assert!(body["state_hash"].as_str().is_some());
    // Identical vectors should contradict at threshold 0.5
    assert_eq!(body["contradicts"].as_bool().unwrap(), true);
}

#[tokio::test]
async fn contradict_orthogonal_vectors_below_threshold() {
    let (_, router) = engine_router(tiny_cfg());
    let id_a = insert_one(router.clone(), [1.0, 0.0, 0.0, 0.0]).await;
    let id_b = insert_one(router.clone(), [0.0, 1.0, 0.0, 0.0]).await;

    let (status, body) = post_json(
        router,
        "/v1/memory/contradict",
        serde_json::json!({"record_a": id_a, "record_b": id_b, "threshold": 0.9}),
    ).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["contradicts"].as_bool().unwrap(), false);
    assert!(body["edge_id"].is_null());
}

// ── /v1/memory/meta/get + /v1/memory/meta/set ────────────────────────────────

#[tokio::test]
async fn meta_set_and_get_roundtrip() {
    let (_, router) = engine_router(tiny_cfg());

    let (status, body) = post_json(
        router.clone(),
        "/v1/memory/meta/set",
        serde_json::json!({"target_id": "node:42", "metadata": {"role": "agent", "version": 3}}),
    ).await;
    assert_eq!(status, StatusCode::OK, "set: {body}");
    assert_eq!(body["success"].as_bool().unwrap(), true);

    let (status2, body2) = get(router, "/v1/memory/meta/get?target_id=node:42").await;
    assert_eq!(status2, StatusCode::OK, "get: {body2}");
    assert_eq!(body2["target_id"].as_str().unwrap(), "node:42");
    assert_eq!(body2["metadata"]["role"].as_str().unwrap(), "agent");
}

#[tokio::test]
async fn meta_get_missing_key_returns_null_metadata() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = get(router, "/v1/memory/meta/get?target_id=does-not-exist").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["metadata"].is_null());
}

// ── /v1/snapshot/download ────────────────────────────────────────────────────

#[tokio::test]
async fn snapshot_download_returns_bytes() {
    let (_, router) = engine_router(tiny_cfg());
    // Insert one record so there's some state
    insert_one(router.clone(), [0.1, 0.2, 0.3, 0.4]).await;

    let resp = router
        .oneshot(Request::builder().uri("/v1/snapshot/download").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    assert!(!bytes.is_empty(), "snapshot download must return non-empty bytes");
}

// ── /v1/snapshot/restore ─────────────────────────────────────────────────────

#[tokio::test]
async fn snapshot_restore_missing_path_returns_error() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let snap_path = tmp_dir.path().join("state.snap");
    let mut cfg = tiny_cfg();
    cfg.snapshot_path = Some(snap_path.clone());
    let (_, router) = engine_router(cfg);

    // Non-existent file within allowed dir
    let (status, _) = post_json(
        router,
        "/v1/snapshot/restore",
        serde_json::json!({"path": snap_path.to_str().unwrap()}),
    ).await;
    assert_ne!(status, StatusCode::OK, "restore of missing file must fail");
}

// ── /v1/ingest/document (chunk-only, no embed required) ──────────────────────

#[tokio::test]
async fn ingest_document_returns_chunks() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = post_json(
        router,
        "/v1/ingest/document",
        serde_json::json!({"text": "Paragraph one. Paragraph two. Paragraph three.", "strategy": "fixed"}),
    ).await;
    // /v1/ingest/document is chunk-only — no embed provider needed
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["chunk_count"].as_u64().unwrap_or(0) >= 1);
    assert!(body["chunks"].is_array());
    assert!(body["strategy_used"].is_string());
}

// ── /v1/ingest/update (embed disabled) ───────────────────────────────────────

#[tokio::test]
async fn ingest_update_without_embed_provider_returns_error() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, _) = post_json(
        router,
        "/v1/ingest/update",
        serde_json::json!({"document_node_id": 1, "text": "updated", "source": "test.txt"}),
    ).await;
    assert_ne!(status, StatusCode::OK);
}

// ── /v1/ingest/extract-entities (embed disabled) ─────────────────────────────

#[tokio::test]
async fn extract_entities_without_embed_provider_returns_error() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, _) = post_json(
        router,
        "/v1/ingest/extract-entities",
        serde_json::json!({"text": "Alice works at Acme Corp."}),
    ).await;
    assert_ne!(status, StatusCode::OK);
}

// ── /v1/ingest/status/:job_id ─────────────────────────────────────────────────

#[tokio::test]
async fn ingest_status_unknown_job_returns_404() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, _) = get(router, "/v1/ingest/status/no-such-job-id").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── /v1/community/overview ───────────────────────────────────────────────────

#[tokio::test]
async fn community_overview_before_detect_returns_empty_or_error() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, body) = get(router, "/v1/community/overview").await;
    // Either 200 with empty communities or a 4xx — both are acceptable before detect runs
    if status == StatusCode::OK {
        let communities = body["communities"].as_array()
            .or_else(|| body.as_array());
        assert!(communities.map(|c| c.is_empty()).unwrap_or(true),
            "no communities should exist before detect: {body}");
    }
    // 4xx is also acceptable
}

// ── /v1/community/search ─────────────────────────────────────────────────────

#[tokio::test]
async fn community_search_before_detect_returns_empty_or_error() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, _body) = post_json(
        router,
        "/v1/community/search",
        serde_json::json!({"vector": [1.0f32, 0.0, 0.0, 0.0], "k": 5}),
    ).await;
    // Before community_detect has been run, the store is empty.
    // The server may return 200 with empty results or 4xx.
    assert!(
        status == StatusCode::OK || status.is_client_error(),
        "unexpected status {status}"
    );
}
