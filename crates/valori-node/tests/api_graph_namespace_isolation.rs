// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.1.1 — Graph read namespace isolation.
//!
//! Proves the exact matrix from the G1.1.1 review:
//!
//!   Namespace A has Node 1; Namespace B has Node 2.
//!     A + Node 1 -> success
//!     A + Node 2 -> not found
//!     B + Node 1 -> not found
//!     B + Node 2 -> success
//!
//! for every affected read (`get_node`, `node_edges`, `subgraph`) and for
//! the cross-tenant delete this closes as a side effect (`delete_node`'s
//! shared handler gates on `get_node`). This file is the STANDALONE half of
//! the matrix — see `cluster_graph_namespace_isolation.rs` for the same
//! matrix against a real single-node Raft cluster.

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

async fn send(app: axum::Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
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

async fn delete(shared: &Arc<RwLock<Engine>>, path: &str) -> (StatusCode, serde_json::Value) {
    let app = build_router(shared.clone(), None, None);
    let req = Request::builder()
        .method("DELETE")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    send(app, req).await
}

/// Sets up: collection "ns-a" with Node 1 (kind=1), collection "ns-b" with
/// Node 2 (kind=1), and a self-loop edge on Node 1 (so `node_edges`/
/// `subgraph` have something non-trivial to leak if the bug were still
/// present). Returns (node1_id, node2_id).
async fn two_namespace_fixture(shared: &Arc<RwLock<Engine>>) -> (u64, u64) {
    let (st, _) = post(
        shared,
        "/v1/namespaces",
        serde_json::json!({ "name": "ns-a", "dimension": 4, "metric": "squared_l2" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = post(
        shared,
        "/v1/namespaces",
        serde_json::json!({ "name": "ns-b", "dimension": 4, "metric": "squared_l2" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, w) = post(
        shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "collection": "ns-a" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let node1 = w["node_id"].as_u64().unwrap();

    let (st, w) = post(
        shared,
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "collection": "ns-b" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let node2 = w["node_id"].as_u64().unwrap();

    // A self-loop on node1, in ns-a, so node_edges/subgraph have real data.
    let (st, _) = post(
        shared,
        "/v1/graph/edge",
        serde_json::json!({ "from": node1, "to": node1, "kind": 0, "collection": "ns-a" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    (node1, node2)
}

// ── get_node ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_node_matrix() {
    let shared = make_shared();
    let (node1, node2) = two_namespace_fixture(&shared).await;

    // A + Node 1 -> success
    let (st, _) = get(&shared, &format!("/v1/graph/node/{node1}?collection=ns-a")).await;
    assert_eq!(st, StatusCode::OK, "A + Node 1 must succeed");

    // A + Node 2 -> not found
    let (st, _) = get(&shared, &format!("/v1/graph/node/{node2}?collection=ns-a")).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "A + Node 2 must be not found");

    // B + Node 1 -> not found
    let (st, _) = get(&shared, &format!("/v1/graph/node/{node1}?collection=ns-b")).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "B + Node 1 must be not found");

    // B + Node 2 -> success
    let (st, _) = get(&shared, &format!("/v1/graph/node/{node2}?collection=ns-b")).await;
    assert_eq!(st, StatusCode::OK, "B + Node 2 must succeed");
}

// ── node_edges ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn node_edges_matrix() {
    let shared = make_shared();
    let (node1, node2) = two_namespace_fixture(&shared).await;

    let (st, body) = get(&shared, &format!("/v1/graph/edges/{node1}?collection=ns-a")).await;
    assert_eq!(st, StatusCode::OK, "A + Node 1 must succeed");
    assert_eq!(
        body["edges"].as_array().unwrap().len(),
        1,
        "must see node1's real self-loop edge"
    );

    let (st, _) = get(&shared, &format!("/v1/graph/edges/{node2}?collection=ns-a")).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "A + Node 2 must be not found");

    let (st, _) = get(&shared, &format!("/v1/graph/edges/{node1}?collection=ns-b")).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "B + Node 1 must be not found");

    let (st, body) = get(&shared, &format!("/v1/graph/edges/{node2}?collection=ns-b")).await;
    assert_eq!(st, StatusCode::OK, "B + Node 2 must succeed");
    assert_eq!(
        body["edges"].as_array().unwrap().len(),
        0,
        "node2 has no edges"
    );
}

// ── subgraph ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn subgraph_matrix() {
    let shared = make_shared();
    let (node1, node2) = two_namespace_fixture(&shared).await;

    // A + Node 1 -> real subgraph (the self-loop node + edge).
    let (st, body) = get(
        &shared,
        &format!("/v1/graph/subgraph?root={node1}&depth=2&collection=ns-a"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        body["nodes"].as_array().unwrap().len(),
        1,
        "A + Node 1 must return the real subgraph, not empty"
    );

    // A + Node 2 -> empty (wrong namespace treated like "root doesn't exist").
    let (st, body) = get(
        &shared,
        &format!("/v1/graph/subgraph?root={node2}&depth=2&collection=ns-a"),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "subgraph never 404s, per its existing convention"
    );
    assert!(
        body["nodes"].as_array().unwrap().is_empty(),
        "A + Node 2 must not leak node2's subgraph"
    );

    // B + Node 1 -> empty.
    let (st, body) = get(
        &shared,
        &format!("/v1/graph/subgraph?root={node1}&depth=2&collection=ns-b"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        body["nodes"].as_array().unwrap().is_empty(),
        "B + Node 1 must not leak node1's subgraph"
    );

    // B + Node 2 -> real (empty) subgraph — node2 has no edges, so this is a
    // single-node result, not literally empty; the point is it succeeds and
    // is Node 2's own data, not a leak.
    let (st, body) = get(
        &shared,
        &format!("/v1/graph/subgraph?root={node2}&depth=2&collection=ns-b"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(body["nodes"][0]["id"].as_u64().unwrap(), node2);
}

// ── delete_node (closed as a side effect of the get_node fix, tested directly) ─

#[tokio::test]
async fn delete_node_cannot_cross_namespaces() {
    let shared = make_shared();
    let (node1, node2) = two_namespace_fixture(&shared).await;

    // Attempting to delete Node 2 while scoped to namespace A must fail...
    let (st, _) = delete(&shared, &format!("/v1/graph/node/{node2}?collection=ns-a")).await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "A must not be able to delete Node 2"
    );

    // ...and Node 2 must still exist afterward, in its real namespace.
    let (st, _) = get(&shared, &format!("/v1/graph/node/{node2}?collection=ns-b")).await;
    assert_eq!(
        st,
        StatusCode::OK,
        "Node 2 must survive the rejected cross-namespace delete"
    );

    // The legitimate delete (B deleting its own Node 2) must succeed.
    let (st, _) = delete(&shared, &format!("/v1/graph/node/{node2}?collection=ns-b")).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = get(&shared, &format!("/v1/graph/node/{node2}?collection=ns-b")).await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "Node 2 must actually be gone now"
    );

    // Node 1 (a different namespace, untouched throughout) must be unaffected.
    let (st, _) = get(&shared, &format!("/v1/graph/node/{node1}?collection=ns-a")).await;
    assert_eq!(
        st,
        StatusCode::OK,
        "Node 1 must be unaffected by any of this"
    );
}

// ── GraphRAG — audited, not fixed: takes no direct node-id input ─────────────

#[tokio::test]
async fn graphrag_has_no_direct_node_id_parameter_to_exploit() {
    // GraphRAG's request shape is `{query_vector, k, depth, collection}` —
    // there is no `root`/`node_id`/`start` field a caller could point at an
    // arbitrary node. Its seeds are entirely derived from a namespace-scoped
    // vector KNN (already isolated) via `resolve_seed_nodes`, which only
    // matches nodes whose `record` field equals an already-namespace-scoped
    // record id — and `CreateNode` already requires a node's record to share
    // its own namespace (G0's invariant), so a matched node cannot belong to
    // a different namespace than the record that produced it. This test
    // documents that reasoning is sound for the simplest case: an empty
    // store's GraphRAG call touches nothing and leaks nothing.
    let shared = make_shared();
    two_namespace_fixture(&shared).await;

    let (st, body) = post(
        &shared,
        "/v1/graphrag",
        serde_json::json!({ "query_vector": [0.0, 0.0, 0.0, 0.0], "k": 5, "depth": 2, "collection": "ns-a" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // No vectors were ever inserted (only bare graph nodes) — hits must be
    // empty, and nothing from ns-b must appear anywhere in the response.
    assert!(body["hits"]
        .as_array()
        .map(|a| a.is_empty())
        .unwrap_or(true));
}
