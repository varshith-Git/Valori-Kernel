// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.4.2 — Cluster Vector Search Namespace Isolation.
//!
//! BUG-6 (found in G1.4.1, fixed here): `POST /search` on the cluster path
//! called `KernelState::search_l2` (its own doc comment: "ALL records
//! regardless of namespace — backward-compat, single-tenant") and relied
//! entirely on `shard_for(ns)` routing for isolation, which enforces
//! nothing once more than one namespace maps to the same shard —
//! `shard_count=1` (the default) puts every namespace on shard 0. Fixed via
//! `shard_search_ns()` in `cluster_server.rs`, mirroring standalone's
//! existing `Engine::search_l2_ns` two-path split (exact `search_l2_ns` for
//! `BruteForce`, global-search-then-post-filter otherwise).
//!
//! This file proves isolation across every combination the design
//! discussion asked for: 1 shard / 2 namespaces (the exploitable case) and
//! N shards / N namespaces (the already-safe-by-routing case, confirmed
//! still safe), for every search mode: plain, decay, metadata_filter,
//! graph_rerank, and soft-deleted-record exclusion.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

use valori_consensus::types::ValoriNode;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterHandle};
use valori_node::cluster_server::build_cluster_router;

const DIM: usize = 4;

async fn boot(shard_count: u32) -> ClusterHandle {
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
        shard_count,
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

fn ids(resp: &serde_json::Value) -> Vec<u64> {
    resp["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_u64().unwrap())
        .collect()
}

/// Creates ns-a and ns-b, inserts one record with the SAME (colliding)
/// vector into each, and returns (handle, rec_a, rec_b). Namespace ids are
/// assigned sequentially starting at 1 (0 is default), so with
/// `shard_count > 1` these two collections land on different shards
/// (`shard_for(1) != shard_for(2)` whenever `shard_count >= 2`) — exactly
/// the "already safe by routing" case this file also verifies stays safe.
async fn two_namespace_fixture(handle: &ClusterHandle) -> (u64, u64) {
    post(
        build_cluster_router(handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-a", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    post(
        build_cluster_router(handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-b", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    let (st, w) = post(
        build_cluster_router(handle, None),
        "/records",
        serde_json::json!({"values": v(0.100), "collection": "ns-a"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let rec_a = w["id"].as_u64().unwrap();
    let (st, w) = post(
        build_cluster_router(handle, None),
        "/records",
        serde_json::json!({"values": v(0.100), "collection": "ns-b"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let rec_b = w["id"].as_u64().unwrap();
    (rec_a, rec_b)
}

// ── 1 shard / 2 namespaces — the exploitable configuration ───────────────────

#[tokio::test]
async fn one_shard_plain_search_does_not_cross_namespaces() {
    let handle = boot(1).await;
    let (rec_a, rec_b) = two_namespace_fixture(&handle).await;

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({"query": v(0.100), "k": 5, "collection": "ns-a"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    let found = ids(&resp);
    assert!(found.contains(&rec_a));
    assert!(
        !found.contains(&rec_b),
        "BUG-6: ns-b leaked into ns-a search"
    );
}

#[tokio::test]
async fn one_shard_decay_search_does_not_cross_namespaces() {
    let handle = boot(1).await;
    let (rec_a, rec_b) = two_namespace_fixture(&handle).await;

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({
            "query": v(0.100), "k": 5, "collection": "ns-a",
            "decay_half_life_secs": 86400
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    let found = ids(&resp);
    assert!(found.contains(&rec_a));
    assert!(!found.contains(&rec_b));
}

#[tokio::test]
async fn one_shard_metadata_filter_does_not_cross_namespaces() {
    let handle = boot(1).await;
    post(
        build_cluster_router(&handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-a", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    post(
        build_cluster_router(&handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-b", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    // Both records share the same metadata so a namespace-blind filter
    // pass would happily let ns-b's record through too.
    let (_, w) = post(
        build_cluster_router(&handle, None),
        "/records",
        serde_json::json!({"values": v(0.100), "collection": "ns-a"}),
    )
    .await;
    let rec_a = w["id"].as_u64().unwrap();
    let (st, w) = post(
        build_cluster_router(&handle, None),
        "/v1/memory/meta/set",
        serde_json::json!({"target_id": format!("rec:{rec_a}"), "metadata": {"tag": "x"}}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");

    let (_, w) = post(
        build_cluster_router(&handle, None),
        "/records",
        serde_json::json!({"values": v(0.100), "collection": "ns-b"}),
    )
    .await;
    let rec_b = w["id"].as_u64().unwrap();
    let (st, w) = post(
        build_cluster_router(&handle, None),
        "/v1/memory/meta/set",
        serde_json::json!({"target_id": format!("rec:{rec_b}"), "metadata": {"tag": "x"}}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({
            "query": v(0.100), "k": 5, "collection": "ns-a",
            "metadata_filter": {"tag": "x"}
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    let found = ids(&resp);
    assert!(found.contains(&rec_a));
    assert!(!found.contains(&rec_b));
}

#[tokio::test]
async fn one_shard_graph_rerank_does_not_cross_namespaces() {
    let handle = boot(1).await;
    let (rec_a, rec_b) = two_namespace_fixture(&handle).await;

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({
            "query": v(0.100), "k": 5, "collection": "ns-a",
            "graph_rerank": {"weight": 1.0}
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    let found = ids(&resp);
    assert!(found.contains(&rec_a));
    assert!(!found.contains(&rec_b));
}

#[tokio::test]
async fn one_shard_soft_deleted_record_stays_excluded_with_namespace_scoping() {
    let handle = boot(1).await;
    post(
        build_cluster_router(&handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": "ns-a", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    let (_, w) = post(
        build_cluster_router(&handle, None),
        "/records",
        serde_json::json!({"values": v(0.100), "collection": "ns-a"}),
    )
    .await;
    let rec_a = w["id"].as_u64().unwrap();
    let (st, w) = post(
        build_cluster_router(&handle, None),
        "/v1/soft-delete",
        serde_json::json!({"id": rec_a, "collection": "ns-a"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({"query": v(0.100), "k": 5, "collection": "ns-a"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert!(!ids(&resp).contains(&rec_a));
}

// ── N shards / N namespaces — already safe via routing, must stay safe ───────

#[tokio::test]
async fn three_shards_two_namespaces_on_different_shards_stay_isolated() {
    // NOTE: each shard runs an INDEPENDENT record-id counter (documented in
    // cluster_server.rs), so rec_a and rec_b can legitimately be the SAME
    // numeric id once they land on different shards — comparing raw ids
    // across shards is meaningless. The real assertion is result COUNT:
    // exactly one record per namespace-scoped search, never two.
    let handle = boot(3).await;
    let (rec_a, rec_b) = two_namespace_fixture(&handle).await;

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({"query": v(0.100), "k": 5, "collection": "ns-a"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    let found_a = ids(&resp);
    assert_eq!(
        found_a,
        vec![rec_a],
        "ns-a search must return exactly its own record"
    );

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({"query": v(0.100), "k": 5, "collection": "ns-b"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    let found_b = ids(&resp);
    assert_eq!(
        found_b,
        vec![rec_b],
        "ns-b search must return exactly its own record"
    );
}

// ── default namespace unaffected ──────────────────────────────────────────────

#[tokio::test]
async fn default_namespace_search_is_unaffected_by_the_fix() {
    let handle = boot(1).await;
    post(
        build_cluster_router(&handle, None),
        "/v1/namespaces",
        serde_json::json!({"name": "default", "dimension": 4, "metric": "squared_l2"}),
    )
    .await;
    let (st, w) = post(
        build_cluster_router(&handle, None),
        "/records",
        serde_json::json!({"values": v(0.5), "collection": "default"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let rec = w["id"].as_u64().unwrap();

    let (st, resp) = post(
        build_cluster_router(&handle, None),
        "/search",
        serde_json::json!({"query": v(0.5), "k": 1, "collection": "default"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{resp}");
    assert!(ids(&resp).contains(&rec));
}
