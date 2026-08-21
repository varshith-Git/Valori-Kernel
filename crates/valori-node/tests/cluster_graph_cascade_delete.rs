// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.3.1 — Record → GraphNode cascade semantics, CLUSTER path.
//!
//! Standalone/cluster parity for `POST /v1/delete` and `POST /v1/soft-delete`:
//! hard delete must cascade through Raft to every referencing node; soft
//! delete must not; cross-namespace hard delete must 404 (BUG-4). Pre-fix,
//! the cluster path did zero cascade of any kind (BUG-3).

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
        shard_count: 1,
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

async fn create_collection(handle: &ClusterHandle, name: &str) {
    let (st, w) = post(
        build_cluster_router(handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": name, "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
}

async fn insert_record(handle: &ClusterHandle, collection: Option<&str>) -> u32 {
    let mut body = serde_json::json!({"values": [0.1, 0.2, 0.3, 0.4]});
    if let Some(c) = collection {
        body["collection"] = serde_json::json!(c);
    }
    let (st, w) = post(build_cluster_router(handle, None), "/records", body).await;
    assert_eq!(st, StatusCode::OK, "{w}");
    w["id"].as_u64().unwrap() as u32
}

async fn create_node(handle: &ClusterHandle, record_id: u32, collection: Option<&str>) -> u64 {
    let mut body = serde_json::json!({"kind": 1, "record_id": record_id});
    if let Some(c) = collection {
        body["collection"] = serde_json::json!(c);
    }
    let (st, w) = post(build_cluster_router(handle, None), "/v1/graph/node", body).await;
    assert_eq!(st, StatusCode::OK, "{w}");
    w["node_id"].as_u64().unwrap()
}

#[tokio::test]
async fn cluster_hard_delete_cascades_to_all_referencing_nodes() {
    let handle = boot_leader().await;
    create_collection(&handle, "default").await;
    let rid = insert_record(&handle, Some("default")).await;
    let n1 = create_node(&handle, rid, Some("default")).await;
    let n2 = create_node(&handle, rid, Some("default")).await;

    let (st, w) = post(
        build_cluster_router(&handle, None),
        "/v1/delete",
        serde_json::json!({"id": rid, "collection": "default"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");

    for n in [n1, n2] {
        let (st, _) = get(
            build_cluster_router(&handle, None),
            &format!("/v1/graph/node/{n}?collection=default"),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::NOT_FOUND,
            "node {n} must be cascade-deleted"
        );
    }
}

#[tokio::test]
async fn cluster_soft_delete_leaves_referencing_nodes_intact() {
    let handle = boot_leader().await;
    create_collection(&handle, "default").await;
    let rid = insert_record(&handle, Some("default")).await;
    let node = create_node(&handle, rid, Some("default")).await;

    let (st, w) = post(
        build_cluster_router(&handle, None),
        "/v1/soft-delete",
        serde_json::json!({"id": rid, "collection": "default"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");

    let (st, _) = get(
        build_cluster_router(&handle, None),
        &format!("/v1/graph/node/{node}?collection=default"),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "soft delete must not cascade");
}

#[tokio::test]
async fn cluster_hard_delete_cannot_cross_namespaces() {
    let handle = boot_leader().await;
    let (st, _) = post(
        build_cluster_router(&handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-a", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = post(
        build_cluster_router(&handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-b", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let rid = insert_record(&handle, Some("ns-a")).await;

    let (st, _) = post(
        build_cluster_router(&handle, None),
        "/v1/delete",
        serde_json::json!({"id": rid, "collection": "ns-b"}),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    let (st, w) = post(
        build_cluster_router(&handle, None),
        "/v1/delete",
        serde_json::json!({"id": rid, "collection": "ns-a"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
}
