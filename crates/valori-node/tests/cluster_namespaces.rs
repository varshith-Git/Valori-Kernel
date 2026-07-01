// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase S2 — Raft-replicated namespace/collection creation, end to end over
//! HTTP against real Raft-backed nodes.
//!
//! The consensus-layer tests (crates/valori-consensus/tests/state_machine.rs
//! and fault_tolerance.rs) already prove the apply()/registry logic in
//! isolation. This file proves the fix at the layer that actually matters to
//! a caller: a `POST /v1/namespaces` on one node must be visible, with the
//! SAME id, on a second node that never received the HTTP request directly
//! — the bug this phase fixes, made observable end to end.

use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use tower::ServiceExt;

use valori_consensus::types::ValoriNode;
use valori_consensus::NullAuditSink;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterHandle};
use valori_node::cluster_server::build_cluster_router;

async fn boot_leader() -> ClusterHandle {
    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(1, ValoriNode { api_addr: "10.0.0.1:3000".into(), raft_addr: String::new() })]
            .into_iter()
            .collect(),
        init: true,
        raft_log_path: None,
        tls: None,
        shard_count: 1,
    };
    let handle = bootstrap_cluster(&cfg, Box::new(NullAuditSink), 0).await.unwrap();
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();
    handle
}

/// A 2-node cluster: node 1 is the leader (with a real HTTP router), node 2
/// is a voter follower whose state machine we read directly (in-process, no
/// HTTP call to it) to prove replication reached it.
async fn two_node_cluster() -> (ClusterHandle, ClusterHandle) {
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
    let h2 = bootstrap_cluster(&cfg2, Box::new(NullAuditSink), 0).await.unwrap();

    h1.raft
        .add_learner(2, ValoriNode { api_addr: "10.0.0.2:3000".into(), raft_addr: h2.raft_addr.to_string() }, true)
        .await
        .unwrap();
    h1.raft
        .change_membership([1u64, 2].into_iter().collect::<std::collections::BTreeSet<_>>(), false)
        .await
        .unwrap();
    h2.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "follower sees the leader")
        .await
        .unwrap();

    (h1, h2)
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
    let location = resp.headers().get(header::LOCATION).map(|v| v.to_str().unwrap().to_string());
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!(null));
    (status, location, json)
}

async fn get_json(router: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let resp = router
        .oneshot(Request::builder().method(Method::GET).uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!(null));
    (status, json)
}

/// `raft.client_write()` returns once the LEADER applies an entry — it does
/// not wait for followers to replicate and apply it too. Polls a follower's
/// state machine until it agrees (or times out), matching the eventual
/// nature of async replication rather than assuming instantaneous convergence.
async fn wait_for_namespace(handle: &ClusterHandle, name: &str, expect: Option<u16>, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let got = handle.state_machine.resolve_namespace(Some(name)).await;
        if got == expect {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("namespace '{name}' did not converge to {expect:?} within {timeout:?}, last saw {got:?}");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn delete_uri(router: axum::Router, uri: &str) -> StatusCode {
    router
        .oneshot(Request::builder().method(Method::DELETE).uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn create_collection_via_http_replicates_to_all_nodes() {
    let (h1, h2) = two_node_cluster().await;
    let router = build_cluster_router(&h1, None);

    let (status, _, body) =
        post_json(router, "/v1/namespaces", serde_json::json!({ "name": "tenant-acme" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let allocated_id = body["id"].as_u64().unwrap() as u16;

    // Node 2 never received an HTTP request. Its state machine agreeing on
    // the same id, read in-process, IS the fix: pre-S2 this would have been
    // an empty, independent, node-local registry with no way to know "docs"
    // even exists.
    wait_for_namespace(&h2, "tenant-acme", Some(allocated_id), Duration::from_secs(5)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn create_collection_is_idempotent_across_retries() {
    let handle = boot_leader().await;
    let router = build_cluster_router(&handle, None);

    let (s1, _, b1) = post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "docs" })).await;
    let (s2, _, b2) = post_json(router, "/v1/namespaces", serde_json::json!({ "name": "docs" })).await;

    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(b1["id"], b2["id"], "same name must always resolve to the same id");
    assert_eq!(b1["created"], true);
    assert_eq!(b2["created"], false, "the second call must report it already existed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drop_collection_replicates_across_nodes() {
    let (h1, h2) = two_node_cluster().await;
    let router = build_cluster_router(&h1, None);

    let (_, _, create_body) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-acme" })).await;
    let allocated_id = create_body["id"].as_u64().unwrap() as u16;
    wait_for_namespace(&h2, "tenant-acme", Some(allocated_id), Duration::from_secs(5)).await;

    let status = delete_uri(router, "/v1/namespaces/tenant-acme").await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    wait_for_namespace(&h2, "tenant-acme", None, Duration::from_secs(5)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drop_default_collection_is_rejected() {
    let handle = boot_leader().await;
    let router = build_cluster_router(&handle, None);
    let status = delete_uri(router, "/v1/namespaces/default").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn list_collections_reflects_committed_creates() {
    let handle = boot_leader().await;
    let router = build_cluster_router(&handle, None);

    post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "docs" })).await;
    post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "images" })).await;

    let (status, body) = get_json(router, "/v1/namespaces").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<String> = body["collections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"default".to_string()));
    assert!(names.contains(&"docs".to_string()));
    assert!(names.contains(&"images".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_to_a_follower_redirects_for_namespace_create() {
    let (h1, h2) = two_node_cluster().await;
    let _ = h1; // keep the leader alive for the duration of the test
    let follower_router = build_cluster_router(&h2, None);

    let (status, location, body) =
        post_json(follower_router, "/v1/namespaces", serde_json::json!({ "name": "tenant-acme" })).await;

    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT, "{body}");
    assert_eq!(
        location.as_deref(),
        Some("http://10.0.0.1:3000"),
        "a follower must redirect a namespace create to the leader, not silently \
         mutate its own out-of-sync local copy — the exact bug this phase fixes"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shard_count_one_is_unaffected_by_namespace_replication() {
    // Regression guard: this whole file boots with shard_count=1 (S1's
    // default) via boot_leader()/two_node_cluster(). Confirm the ordinary
    // single-shard create/resolve/drop cycle behaves exactly as documented,
    // with no dependency on S1's multi-shard machinery.
    let handle = boot_leader().await;
    let router = build_cluster_router(&handle, None);

    let (status, _, body) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "docs" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(handle.state_machine.resolve_namespace(Some("docs")).await, Some(body["id"].as_u64().unwrap() as u16));

    let status = delete_uri(router, "/v1/namespaces/docs").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(handle.state_machine.resolve_namespace(Some("docs")).await, None);
}

// ── Phase S3b: real shard-routed writes ──────────────────────────────────────

async fn boot_leader_with_shards(shard_count: u32) -> ClusterHandle {
    let cfg = ClusterConfig {
        node_id: 1,
        raft_bind: "127.0.0.1:0".into(),
        members: [(1, ValoriNode { api_addr: "10.0.0.1:3000".into(), raft_addr: String::new() })]
            .into_iter()
            .collect(),
        init: true,
        raft_log_path: None,
        tls: None,
        shard_count,
    };
    let handle = bootstrap_cluster(&cfg, Box::new(NullAuditSink), 0).await.unwrap();
    handle
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();
    handle
}

/// The flagship end-to-end proof for S3a+S3b together: two different
/// collections, upserted via real HTTP, land in two DIFFERENT shards' Raft
/// groups — not just "correctly namespace-scoped" (S3a) but genuinely
/// routed to isolated Raft state machines (S3b). Before S3a's fix, every
/// one of these writes would have silently landed in namespace 0 on shard
/// 0, regardless of the collection requested.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_to_different_collections_route_to_different_shards() {
    let handle = boot_leader_with_shards(3).await;
    let router = build_cluster_router(&handle, None);

    // Namespace ids are assigned sequentially starting at 1, so the first
    // two collections created land on shard_for(1)=1 and shard_for(2)=2
    // with shard_count=3 — two distinct shards, deterministically.
    let (_, _, body_a) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-a" })).await;
    let (_, _, body_b) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-b" })).await;
    let ns_a = body_a["id"].as_u64().unwrap() as u16;
    let ns_b = body_b["id"].as_u64().unwrap() as u16;
    assert_ne!(ns_a, ns_b);

    let (status, _, body) = post_json(
        router.clone(),
        "/v1/memory/upsert",
        serde_json::json!({ "vector": [1.0, 2.0, 3.0, 4.0], "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, _, body) = post_json(
        router,
        "/v1/memory/upsert",
        serde_json::json!({ "vector": [5.0, 6.0, 7.0, 8.0], "collection": "tenant-b" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let shard_a = valori_consensus::types::ShardId((ns_a as u32) % 3);
    let shard_b = valori_consensus::types::ShardId((ns_b as u32) % 3);
    assert_ne!(shard_a, shard_b, "test setup must exercise two distinct shards");

    let count_a = handle.shards[&shard_a].state_machine.with_state(|s| s.record_count()).await;
    let count_b = handle.shards[&shard_b].state_machine.with_state(|s| s.record_count()).await;
    assert_eq!(count_a, 1, "tenant-a's record must land on its own shard ({shard_a:?})");
    assert_eq!(count_b, 1, "tenant-b's record must land on its own shard ({shard_b:?})");

    // And NOT cross-contaminate: tenant-a's shard holds exactly tenant-a's
    // data, nothing from tenant-b, and vice versa (already implied by
    // count == 1 on each independent shard, but assert explicitly for
    // shard 0 — the namespace-registry shard — holding zero DATA records,
    // since neither upsert targeted the default namespace).
    let count_shard0 = handle.shards[&valori_consensus::types::ShardId(0)]
        .state_machine
        .with_state(|s| s.record_count())
        .await;
    assert_eq!(count_shard0, 0, "shard 0 holds the namespace registry, not tenant-a/b's records");
}
