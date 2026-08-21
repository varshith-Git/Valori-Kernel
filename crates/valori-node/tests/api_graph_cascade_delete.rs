// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.3.1 — Record → GraphNode cascade semantics, HTTP level (standalone).
//!
//! `POST /v1/delete` (hard) must cascade-delete every node referencing the
//! record; `POST /v1/soft-delete` must not touch the graph; cross-namespace
//! deletion must 404 (BUG-4), matching `GraphOps::delete_node`'s convention.

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

async fn create_default_collection(router: axum::Router) {
    let (status, body) = post_json(
        router,
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "collection create failed: {body}");
}

async fn insert_one(router: axum::Router, vec: [f32; 4]) -> u32 {
    let (status, body) = post_json(
        router,
        "/records",
        serde_json::json!({"values": vec, "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "insert failed: {body}");
    body["id"].as_u64().expect("missing id") as u32
}

async fn create_node(router: axum::Router, record: u32, kind: u8) -> u64 {
    let (status, body) = post_json(
        router,
        "/v1/graph/node",
        serde_json::json!({"kind": kind, "record_id": record, "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create node failed: {body}");
    body["node_id"].as_u64().unwrap()
}

// ── hard delete cascades to every referencing node ────────────────────────────

#[tokio::test]
async fn hard_delete_cascades_to_all_referencing_nodes() {
    let (_, router) = engine_router(tiny_cfg());
    create_default_collection(router.clone()).await;
    let rid = insert_one(router.clone(), [0.1, 0.2, 0.3, 0.4]).await;
    let n1 = create_node(router.clone(), rid, 1).await;
    let n2 = create_node(router.clone(), rid, 2).await;

    let (status, body) = post_json(
        router.clone(),
        "/v1/delete",
        serde_json::json!({"id": rid, "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    for n in [n1, n2] {
        let (status, _) = get(
            router.clone(),
            &format!("/v1/graph/node/{n}?collection=default"),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "node {n} must be cascade-deleted"
        );
    }
}

// ── soft delete leaves the graph alone ─────────────────────────────────────────

#[tokio::test]
async fn soft_delete_leaves_referencing_nodes_intact() {
    let (_, router) = engine_router(tiny_cfg());
    create_default_collection(router.clone()).await;
    let rid = insert_one(router.clone(), [0.1, 0.2, 0.3, 0.4]).await;
    let node = create_node(router.clone(), rid, 1).await;

    let (status, body) = post_json(
        router.clone(),
        "/v1/soft-delete",
        serde_json::json!({"id": rid, "collection": "default"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, _) = get(
        router.clone(),
        &format!("/v1/graph/node/{node}?collection=default"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "soft delete must not cascade to the graph"
    );
}

// ── cross-namespace hard delete 404s instead of succeeding (BUG-4) ────────────

#[tokio::test]
async fn hard_delete_cannot_cross_namespaces() {
    let (_, router) = engine_router(tiny_cfg());
    let (status, _) = post_json(
        router.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-a", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post_json(
        router.clone(),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-b", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post_json(
        router.clone(),
        "/records",
        serde_json::json!({"values": [0.1, 0.2, 0.3, 0.4], "collection": "ns-a"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rid = body["id"].as_u64().unwrap() as u32;

    // Deleting namespace A's record through namespace B's scope must 404,
    // never succeed and never delete the record.
    let (status, _) = post_json(
        router.clone(),
        "/v1/delete",
        serde_json::json!({"id": rid, "collection": "ns-b"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The record must still be deletable through its real namespace.
    let (status, body) = post_json(
        router.clone(),
        "/v1/delete",
        serde_json::json!({"id": rid, "collection": "ns-a"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
