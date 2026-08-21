// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.1.1 — Graph read namespace isolation, CLUSTER path.
//!
//! Same matrix as `api_graph_namespace_isolation.rs` (the standalone test),
//! against a real single-node Raft cluster (self-elected leader,
//! `shard_count: 1`) so both namespaces genuinely share one physical shard
//! — the exact condition that made the pre-fix bug exploitable in cluster
//! mode (`shard_for(ns)` alone does not imply `node.namespace_id == ns`
//! once more than one namespace maps to the same shard).

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

use valori_consensus::types::ValoriNode;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterHandle};
use valori_node::cluster_server::build_cluster_router;

async fn boot_leader() -> ClusterHandle {
    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(
            1,
            ValoriNode {
                api_addr: "10.0.0.1:3000".into(),
                raft_addr: String::new(),
            },
        )]
        .into_iter()
        .collect(),
        init: true,
        raft_log_path: None,
        tls: None,
        shard_count: 1, // both namespaces land on the same shard — the risky case
    };
    let handle = bootstrap_cluster(&cfg, None, None).await.unwrap();
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();
    handle
}

async fn post(
    router: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
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

async fn get(router: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
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

async fn delete(router: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
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

/// Namespace "ns-a" gets Node 1 (with a self-loop edge); "ns-b" gets Node 2.
/// Returns (handle, node1_id, node2_id).
async fn two_namespace_fixture() -> (ClusterHandle, u64, u64) {
    let handle = boot_leader().await;

    let (st, _) = post(
        build_cluster_router(&handle, None),
        "/v1/namespaces",
        serde_json::json!({ "name": "ns-a", "dimension": 4, "metric": "squared_l2" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = post(
        build_cluster_router(&handle, None),
        "/v1/namespaces",
        serde_json::json!({ "name": "ns-b", "dimension": 4, "metric": "squared_l2" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, w) = post(
        build_cluster_router(&handle, None),
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "collection": "ns-a" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let node1 = w["node_id"].as_u64().unwrap();

    let (st, w) = post(
        build_cluster_router(&handle, None),
        "/v1/graph/node",
        serde_json::json!({ "kind": 1, "collection": "ns-b" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let node2 = w["node_id"].as_u64().unwrap();

    let (st, _) = post(
        build_cluster_router(&handle, None),
        "/v1/graph/edge",
        serde_json::json!({ "from": node1, "to": node1, "kind": 0, "collection": "ns-a" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    (handle, node1, node2)
}

#[tokio::test]
async fn cluster_get_node_matrix() {
    let (handle, node1, node2) = two_namespace_fixture().await;

    let (st, _) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/node/{node1}?collection=ns-a"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "A + Node 1 must succeed");

    let (st, _) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/node/{node2}?collection=ns-a"),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "A + Node 2 must be not found");

    let (st, _) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/node/{node1}?collection=ns-b"),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "B + Node 1 must be not found");

    let (st, _) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/node/{node2}?collection=ns-b"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "B + Node 2 must succeed");
}

#[tokio::test]
async fn cluster_node_edges_and_subgraph_do_not_leak() {
    let (handle, node1, node2) = two_namespace_fixture().await;

    // node_edges: A + Node 2 must not leak.
    let (st, _) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/edges/{node2}?collection=ns-a"),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // subgraph: A + Node 2 must return empty, not Node 2's real subgraph.
    let (st, body) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/subgraph?root={node2}&depth=2&collection=ns-a"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(body["nodes"].as_array().unwrap().is_empty());

    // subgraph: A + Node 1 (correct namespace) must return the real data.
    let (st, body) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/subgraph?root={node1}&depth=2&collection=ns-a"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["nodes"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn cluster_delete_node_cannot_cross_namespaces() {
    let (handle, node1, node2) = two_namespace_fixture().await;

    let (st, _) = delete(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/node/{node2}?collection=ns-a"),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "A must not be able to delete Node 2"
    );

    let (st, _) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/node/{node2}?collection=ns-b"),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "Node 2 must survive the rejected delete"
    );

    let (st, _) = delete(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/node/{node2}?collection=ns-b"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "B deleting its own Node 2 must succeed");

    let (st, _) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/node/{node1}?collection=ns-a"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "Node 1 must be unaffected");
}
