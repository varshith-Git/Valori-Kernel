// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.4.1 — graph-aware vector reranking, CLUSTER path.
//!
//! Standalone/cluster parity for the `graph_rerank` search parameter, over
//! a real single-node Raft cluster, reusing the `boot_leader()` pattern
//! established by the G1.1.1/G1.3.1 cluster test files.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

use valori_consensus::types::ValoriNode;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterHandle};
use valori_node::cluster_server::build_cluster_router;

const DIM: usize = 4;

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

fn v(seed: f32) -> Vec<f32> {
    (0..DIM).map(|i| seed + i as f32 * 0.001).collect()
}

async fn create_default_collection(handle: &ClusterHandle) {
    let (st, w) = post(
        build_cluster_router(handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": DIM, "metric": "squared_l2"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
}

async fn insert(handle: &ClusterHandle, seed: f32) -> u32 {
    let (st, w) = post(
        build_cluster_router(handle, None),
        "/records",
        serde_json::json!({"values": v(seed), "collection": "default"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    w["id"].as_u64().unwrap() as u32
}

async fn create_node(handle: &ClusterHandle, record_id: u32) -> u64 {
    let (st, w) = post(
        build_cluster_router(handle, None),
        "/v1/graph/node",
        serde_json::json!({"kind": 1, "record_id": record_id, "collection": "default"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    w["node_id"].as_u64().unwrap()
}

async fn create_edge(handle: &ClusterHandle, from: u64, to: u64) -> u64 {
    let (st, w) = post(
        build_cluster_router(handle, None),
        "/v1/graph/edge",
        serde_json::json!({"from": from, "to": to, "kind": 0, "collection": "default"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    w["edge_id"].as_u64().unwrap()
}

fn ids(resp: &serde_json::Value) -> Vec<u64> {
    resp["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_u64().unwrap())
        .collect()
}

#[tokio::test]
async fn cluster_graph_rerank_reports_hop_distance() {
    let handle = boot_leader().await;
    create_default_collection(&handle).await;
    let seed_rec = insert(&handle, 0.100).await;
    let connected_rec = insert(&handle, 0.150).await;
    let seed_node = create_node(&handle, seed_rec).await;
    let connected_node = create_node(&handle, connected_rec).await;
    create_edge(&handle, seed_node, connected_node).await;

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({
            "query": v(0.100), "k": 2, "collection": "default",
            "graph_rerank": {"weight": 0.5}
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    let hits = resp["results"].as_array().unwrap();
    let connected_hit = hits.iter().find(|h| h["id"] == connected_rec).unwrap();
    assert_eq!(connected_hit["graph_distance"], 1);
}

#[tokio::test]
async fn cluster_absent_graph_rerank_omits_the_field() {
    let handle = boot_leader().await;
    create_default_collection(&handle).await;
    insert(&handle, 0.100).await;

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({"query": v(0.100), "k": 1, "collection": "default"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    for hit in resp["results"].as_array().unwrap() {
        assert!(hit.get("graph_distance").is_none());
    }
}

// NOTE (found during G1.4.1, NOT introduced by it, NOT fixed here — out of
// scope): cluster's `/search` ignores namespace entirely when
// `shard_count=1` — `cluster_server.rs::search()` calls `s.search_l2(...)`
// (no namespace filter), relying solely on shard routing for isolation,
// which does nothing when every namespace maps to the same shard. Proven
// with `graph_rerank` entirely absent, so this is a pre-existing gap in
// plain vector search, not something graph-aware reranking causes. Already
// flagged as discrepancy #4 in
// docs/reviews/graph-g1.4-hybrid-retrieval-design.md §1. No standalone
// equivalent — `server.rs::search()` correctly calls `search_l2_ns`. A
// namespace-isolation test for `graph_rerank` on the cluster path is
// therefore not meaningful until that pre-existing bug is fixed elsewhere;
// see `graph_aware_reranking.rs::graph_rerank_never_crosses_namespaces_for_seeds_or_candidates`
// for the equivalent, currently-passing, standalone-path proof.

#[tokio::test]
async fn cluster_soft_deleted_candidate_never_appears() {
    let handle = boot_leader().await;
    create_default_collection(&handle).await;
    let seed_rec = insert(&handle, 0.100).await;
    let victim_rec = insert(&handle, 0.101).await;
    create_node(&handle, seed_rec).await;
    create_node(&handle, victim_rec).await;

    let (st, _) = post(
        build_cluster_router(&handle, None),
        "/v1/soft-delete",
        serde_json::json!({"id": victim_rec, "collection": "default"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({
            "query": v(0.100), "k": 5, "collection": "default",
            "graph_rerank": {"weight": 1.0}
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert!(!ids(&resp).contains(&(victim_rec as u64)));
}
