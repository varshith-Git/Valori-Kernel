// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Cluster data plane v1: insert and search over HTTP against a real
//! Raft-backed node, follower redirect with Location, and the audit log
//! receiving exactly what was committed.

use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
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
    let handle = bootstrap_cluster(&cfg, None, None, 0).await.unwrap();
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();
    handle
}

async fn post_json(
    router: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, Option<String>, serde_json::Value) {
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
    let location = resp
        .headers()
        .get(header::LOCATION)
        .map(|v| v.to_str().unwrap().to_string());
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!(null));
    (status, location, json)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn insert_then_search_over_http() {
    let handle = boot_leader().await;
    let router = build_cluster_router(&handle, None);

    // Three points on a line; query nearest to the middle one.
    for v in [[0.0f32, 0.0], [1.0, 1.0], [5.0, 5.0]] {
        let (status, _, body) = post_json(
            router.clone(),
            "/records",
            serde_json::json!({ "values": v }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let (status, _, body) = post_json(
        router.clone(),
        "/search",
        serde_json::json!({ "query": [1.1, 0.9], "k": 2 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Wire-compatible with the standalone server: { results: [{ id, score }] }.
    let hits = body["results"].as_array().unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0]["id"], 1, "nearest must be (1,1): {body}");
    assert_eq!(hits[1]["id"], 0, "second nearest must be (0,0): {body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_inserts_all_land_despite_id_races() {
    let handle = boot_leader().await;
    let router = build_cluster_router(&handle, None);

    // Fire 16 inserts concurrently — they race on the sequential record id;
    // the retry loop must absorb the conflicts.
    let mut tasks = Vec::new();
    for i in 0..16u32 {
        let r = router.clone();
        tasks.push(tokio::spawn(async move {
            post_json(
                r,
                "/records",
                serde_json::json!({ "values": [i as f32, 0.0] }),
            )
            .await
            .0
        }));
    }
    for t in tasks {
        assert_eq!(t.await.unwrap(), StatusCode::OK);
    }
    assert_eq!(
        handle.state_machine.with_state(|s| s.record_count()).await,
        16,
        "every concurrent insert must land exactly once"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_id_deduplicates_an_http_retry() {
    let handle = boot_leader().await;
    let router = build_cluster_router(&handle, None);

    let rid: Vec<u8> = vec![9; 16];
    let body = serde_json::json!({ "values": [1.0, 2.0], "request_id": rid });
    let (s1, _, b1) = post_json(router.clone(), "/records", body.clone()).await;
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(b1["deduplicated"], false);

    let (s2, _, b2) = post_json(router, "/records", body).await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(b2["deduplicated"], true, "retry must be recognised: {b2}");
    assert_eq!(
        handle.state_machine.with_state(|s| s.record_count()).await,
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_to_a_follower_redirects_with_location() {
    // Two real nodes; find the follower and post an insert at it.
    let h1 = boot_leader().await;
    let cfg2 = ClusterConfig {
        node_id: 2,
        raft_bind: "127.0.0.1:0".into(),
        members: [(2, ValoriNode::default())].into_iter().collect(),
        init: false,
        raft_log_path: None,
        tls: None,
        shard_count: 1,
    };
    let h2 = bootstrap_cluster(&cfg2, None, None, 0).await.unwrap();

    h1.raft
        .add_learner(
            2,
            ValoriNode {
                api_addr: "10.0.0.2:3000".into(),
                raft_addr: h2.raft_addr.to_string(),
            },
            true,
        )
        .await
        .unwrap();
    h1.raft
        .change_membership(
            [1u64, 2]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            false,
        )
        .await
        .unwrap();

    // Wait until node 2 knows node 1 leads.
    h2.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "follower sees the leader")
        .await
        .unwrap();

    let follower_router = build_cluster_router(&h2, None);
    let (status, location, body) = post_json(
        follower_router,
        "/records",
        serde_json::json!({ "values": [1.0, 2.0] }),
    )
    .await;

    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT, "{body}");
    assert_eq!(
        location.as_deref(),
        Some("http://10.0.0.1:3000"),
        "Location must point at the leader's API: {body}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn health_and_metrics_are_served() {
    let handle = boot_leader().await;
    let router = build_cluster_router(&handle, None);

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
