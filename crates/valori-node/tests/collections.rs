// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Collection (namespace) management — create, scope, isolate, drop, snapshot.

use valori_node::api::{validate_collection, DEFAULT_COLLECTION};
use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::Engine;
use valori_node::server::{build_router, SharedEngine};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;

fn cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 256;
    cfg.max_nodes = 64;
    cfg.max_edges = 64;
    cfg.index_kind = IndexKind::BruteForce;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    cfg.snapshot_path = None;
    cfg
}

fn make_shared() -> SharedEngine {
    Arc::new(RwLock::new(Engine::new(&cfg())))
}

fn vec4(x: f32) -> serde_json::Value {
    serde_json::json!([x, x * 0.1, x * 0.01, x * 0.001])
}

// ── oneshot helpers ───────────────────────────────────────────────────────────

async fn http(
    app: axum::Router,
    method: Method,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let req_body = if let Some(b) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(serde_json::to_vec(&b).unwrap())
    } else {
        Body::empty()
    };
    let resp = app.oneshot(builder.body(req_body).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn post_json(
    shared: SharedEngine,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared, None, None);
    http(app, Method::POST, uri, Some(body)).await
}

async fn get_json(shared: SharedEngine, uri: &str) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared, None, None);
    http(app, Method::GET, uri, None).await
}

async fn delete_req(shared: SharedEngine, uri: &str) -> StatusCode {
    let app = build_router(shared, None, None);
    http(app, Method::DELETE, uri, None).await.0
}

// ── unit: backward-compat validate_collection ────────────────────────────────

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

// ── HTTP: legacy behavior unchanged ──────────────────────────────────────────

#[tokio::test]
async fn insert_without_collection_works() {
    let (status, _) = post_json(
        make_shared(),
        "/records",
        serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn insert_with_default_collection_works() {
    let (status, _) = post_json(
        make_shared(),
        "/records",
        serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4], "collection": "default" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn insert_with_unknown_collection_is_400() {
    let (status, _) = post_json(
        make_shared(),
        "/records",
        serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4], "collection": "tenant-42" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rejected_collection_does_not_mutate_state() {
    let shared = make_shared();
    let (status, _) = post_json(
        shared.clone(),
        "/records",
        serde_json::json!({ "values": [0.1, 0.2, 0.3, 0.4], "collection": "nope" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(shared.read().await.state.record_count(), 0);
}

// ── HTTP: collection CRUD ─────────────────────────────────────────────────────

#[tokio::test]
async fn default_collection_appears_in_list() {
    let (status, body) = get_json(make_shared(), "/v1/namespaces").await;
    assert_eq!(status, StatusCode::OK);
    let cols = body["collections"].as_array().unwrap();
    assert!(cols.iter().any(|c| c["name"] == "default" && c["id"] == 0));
}

#[tokio::test]
async fn create_and_list_collections() {
    let shared = make_shared();

    let (s, r) = post_json(shared.clone(), "/v1/namespaces",
        serde_json::json!({"name": "tenantA"})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["name"], "tenantA");
    assert_eq!(r["id"], 1);
    assert_eq!(r["created"], true);

    let (_, r2) = post_json(shared.clone(), "/v1/namespaces",
        serde_json::json!({"name": "tenantB"})).await;
    assert_eq!(r2["id"], 2);

    let (_, list) = get_json(shared, "/v1/namespaces").await;
    let names: Vec<&str> = list["collections"].as_array().unwrap()
        .iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"default"));
    assert!(names.contains(&"tenantA"));
    assert!(names.contains(&"tenantB"));
}

#[tokio::test]
async fn create_collection_is_idempotent() {
    let shared = make_shared();
    let (_, r1) = post_json(shared.clone(), "/v1/namespaces",
        serde_json::json!({"name": "dup"})).await;
    let (_, r2) = post_json(shared, "/v1/namespaces",
        serde_json::json!({"name": "dup"})).await;
    assert_eq!(r1["id"], r2["id"]);
    assert_eq!(r2["created"], false);
}

#[tokio::test]
async fn cannot_drop_default_collection() {
    let status = delete_req(make_shared(), "/v1/namespaces/default").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn drop_unknown_collection_is_404() {
    // Unified behavior (routes::collections): unknown collection is 404 on
    // BOTH paths. Standalone previously returned 400 while cluster returned
    // 404 — a silent dual-path divergence.
    let status = delete_req(make_shared(), "/v1/namespaces/ghost").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── HTTP: namespace isolation ────────────────────────────────────────────────

#[tokio::test]
async fn search_is_scoped_to_collection() {
    let shared = make_shared();

    // create tenantA
    post_json(shared.clone(), "/v1/namespaces", serde_json::json!({"name": "tenantA"})).await;

    // two records in default
    post_json(shared.clone(), "/records", serde_json::json!({"values": vec4(1.0)})).await;
    post_json(shared.clone(), "/records", serde_json::json!({"values": vec4(2.0)})).await;

    // one record in tenantA
    let (_, ins) = post_json(shared.clone(), "/records",
        serde_json::json!({"values": vec4(100.0), "collection": "tenantA"})).await;
    let tenant_id = ins["id"].as_u64().unwrap();

    // search tenantA — only tenantA record
    let (_, hits) = post_json(shared.clone(), "/search",
        serde_json::json!({"query": vec4(100.0), "k": 10, "collection": "tenantA"})).await;
    let results = hits["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"].as_u64().unwrap(), tenant_id);

    // search default — does NOT include tenantA record
    let (_, hits_default) = post_json(shared, "/search",
        serde_json::json!({"query": vec4(100.0), "k": 10})).await;
    let default_ids: Vec<u64> = hits_default["results"].as_array().unwrap()
        .iter().map(|h| h["id"].as_u64().unwrap()).collect();
    assert!(!default_ids.contains(&tenant_id));
}

#[tokio::test]
async fn batch_insert_scoped_to_collection() {
    let shared = make_shared();
    post_json(shared.clone(), "/v1/namespaces", serde_json::json!({"name": "batch_ns"})).await;

    let (_, resp) = post_json(shared.clone(), "/v1/vectors/batch_insert",
        serde_json::json!({"batch": [vec4(1.0), vec4(2.0), vec4(3.0)], "collection": "batch_ns"})).await;
    assert_eq!(resp["ids"].as_array().unwrap().len(), 3);

    // default search must not see them
    let (_, hits) = post_json(shared, "/search",
        serde_json::json!({"query": vec4(2.0), "k": 10})).await;
    assert_eq!(hits["results"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn drop_collection_removes_records_from_search() {
    let shared = make_shared();
    post_json(shared.clone(), "/v1/namespaces", serde_json::json!({"name": "drop_me"})).await;
    post_json(shared.clone(), "/records",
        serde_json::json!({"values": vec4(5.0), "collection": "drop_me"})).await;

    // verify present before drop
    let (_, hits) = post_json(shared.clone(), "/search",
        serde_json::json!({"query": vec4(5.0), "k": 5, "collection": "drop_me"})).await;
    assert_eq!(hits["results"].as_array().unwrap().len(), 1);

    // drop
    let status = delete_req(shared.clone(), "/v1/namespaces/drop_me").await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // collection gone from list
    let (_, list) = get_json(shared.clone(), "/v1/namespaces").await;
    let names: Vec<&str> = list["collections"].as_array().unwrap()
        .iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&"drop_me"));

    // searching dropped collection is now 400
    let (s, _) = post_json(shared, "/search",
        serde_json::json!({"query": vec4(5.0), "k": 5, "collection": "drop_me"})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

// ── Snapshot persistence ──────────────────────────────────────────────────────

#[tokio::test]
async fn snapshot_preserves_collections_and_data() {
    let shared = make_shared();

    post_json(shared.clone(), "/v1/namespaces",
        serde_json::json!({"name": "persist_me"})).await;
    let (_, ins) = post_json(shared.clone(), "/records",
        serde_json::json!({"values": vec4(42.0), "collection": "persist_me"})).await;
    let expected_id = ins["id"].as_u64().unwrap();

    // round-trip through snapshot
    let snap_bytes = shared.read().await.snapshot().unwrap();
    shared.write().await.restore(&snap_bytes).unwrap();

    // after restore: collection still exists and record still found
    let (s, hits) = post_json(shared, "/search",
        serde_json::json!({"query": vec4(42.0), "k": 5, "collection": "persist_me"})).await;
    assert_eq!(s, StatusCode::OK);
    let ids: Vec<u64> = hits["results"].as_array().unwrap()
        .iter().map(|h| h["id"].as_u64().unwrap()).collect();
    assert!(ids.contains(&expected_id));
}
