// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.1 — `GET /v1/graph/query` HTTP-level integration tests.
//!
//! `crates/valori-rag/src/graph.rs`'s unit tests already prove `query_graph`
//! itself is correct and deterministic; these tests exercise the HTTP
//! plumbing on top of it (query-string parsing, error mapping, namespace
//! resolution via `collection`) that the unit tests can't reach.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt;
use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::Engine;
use valori_node::server::build_router;
use valori_node::EngineFromNodeConfig;

fn make_shared() -> Arc<RwLock<Engine>> {
    let mut cfg = NodeConfig::default();
    cfg.max_records = 100;
    cfg.max_nodes = 64;
    cfg.max_edges = 64;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    Arc::new(RwLock::new(Engine::new(&cfg)))
}

async fn post(
    shared: &Arc<RwLock<Engine>>,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared.clone(), None, None);
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    send(app, req).await
}

async fn get(shared: &Arc<RwLock<Engine>>, path: &str) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared.clone(), None, None);
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    send(app, req).await
}

async fn send(app: axum::Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// Alice(kind=1/Concept) --Follows(1)--> Bob(kind=3/User)
/// Alice --ByAgent(3)--> Acme(kind=2/Agent)
/// Returns (alice_id, bob_id, acme_id).
async fn alice_bob_acme(shared: &Arc<RwLock<Engine>>) -> (u64, u64, u64) {
    let (st, body) = post(
        shared,
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");

    let (st, w) = post(
        shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let alice = w["node_id"].as_u64().unwrap();

    let (st, w) = post(
        shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 3, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let bob = w["node_id"].as_u64().unwrap();

    let (st, w) = post(
        shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 2, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let acme = w["node_id"].as_u64().unwrap();

    let (st, _) = post(
        shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": alice, "to": bob, "kind": 1, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, _) = post(
        shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": alice, "to": acme, "kind": 3, "collection": "default" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    (alice, bob, acme)
}

#[tokio::test]
async fn query_returns_direct_outgoing_neighbors() {
    let shared = make_shared();
    let (alice, bob, acme) = alice_bob_acme(&shared).await;

    let (st, body) = get(
        &shared,
        &format!("/v1/graph/query?start={alice}&depth=1&collection=default"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ids: Vec<u64> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["node_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![bob.min(acme), bob.max(acme)]); // sorted ascending by id
    assert_eq!(body["count"].as_u64().unwrap(), 2);
}

#[tokio::test]
async fn query_filters_by_edge_kind() {
    let shared = make_shared();
    let (alice, bob, _acme) = alice_bob_acme(&shared).await;

    // kind=1 -> "Follows" edge only, matching the alice->bob edge.
    let (st, body) = get(
        &shared,
        &format!("/v1/graph/query?start={alice}&depth=1&edge_kind=1&collection=default"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ids: Vec<u64> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["node_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![bob]);
}

#[tokio::test]
async fn query_filters_by_node_kind() {
    let shared = make_shared();
    let (alice, bob, _acme) = alice_bob_acme(&shared).await;

    // node_kind=3 -> User (Bob) only.
    let (st, body) = get(
        &shared,
        &format!("/v1/graph/query?start={alice}&depth=1&node_kind=3&collection=default"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ids: Vec<u64> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["node_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![bob]);
}

#[tokio::test]
async fn query_missing_start_node_returns_404() {
    let shared = make_shared();
    let (st, _body) = get(&shared, "/v1/graph/query?start=9999").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn query_invalid_direction_returns_400() {
    let shared = make_shared();
    let (alice, _, _) = alice_bob_acme(&shared).await;
    let (st, _body) = get(
        &shared,
        &format!("/v1/graph/query?start={alice}&direction=sideways&collection=default"),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_invalid_edge_kind_returns_400() {
    let shared = make_shared();
    let (alice, _, _) = alice_bob_acme(&shared).await;
    let (st, _body) = get(
        &shared,
        &format!("/v1/graph/query?start={alice}&edge_kind=255&collection=default"),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_unknown_collection_returns_404() {
    let shared = make_shared();
    let (alice, _, _) = alice_bob_acme(&shared).await;
    let (st, _body) = get(
        &shared,
        &format!("/v1/graph/query?start={alice}&collection=nonexistent"),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn query_direction_incoming_and_both_work() {
    let shared = make_shared();
    let (alice, bob, _acme) = alice_bob_acme(&shared).await;

    // From Bob's perspective, incoming reaches Alice.
    let (st, body) = get(
        &shared,
        &format!("/v1/graph/query?start={bob}&direction=incoming&depth=1&collection=default"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ids: Vec<u64> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["node_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![alice]);

    let (st, body) = get(
        &shared,
        &format!("/v1/graph/query?start={bob}&direction=both&depth=1&collection=default"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ids: Vec<u64> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["node_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![alice], "Bob has no outgoing edges, one incoming");
}

#[tokio::test]
async fn repeated_identical_query_over_http_is_deterministic() {
    let shared = make_shared();
    let (alice, _, _) = alice_bob_acme(&shared).await;
    let path = format!("/v1/graph/query?start={alice}&depth=2&collection=default");
    let (st1, body1) = get(&shared, &path).await;
    let (st2, body2) = get(&shared, &path).await;
    assert_eq!(st1, StatusCode::OK);
    assert_eq!(st2, StatusCode::OK);
    assert_eq!(body1, body2);
}
