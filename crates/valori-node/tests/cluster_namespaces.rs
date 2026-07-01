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
use valori_node::cluster_server::{build_cluster_router, build_cluster_router_with_keys};

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

/// Phase S4: cluster_memory_consolidate (soft-delete old + insert new +
/// graph nodes + Supersedes edge) must route its entire write sequence to
/// the shard that owns the target collection — not just the insert.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consolidate_routes_to_the_collections_shard() {
    let handle = boot_leader_with_shards(3).await;
    let router = build_cluster_router(&handle, None);

    // First collection created gets namespace id 1 -> shard 1 (1 % 3 = 1).
    let (_, _, body) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-a" })).await;
    let ns_a = body["id"].as_u64().unwrap() as u16;
    let shard_a = valori_consensus::types::ShardId((ns_a as u32) % 3);
    assert_ne!(shard_a, valori_consensus::types::ShardId(0));

    let (status, _, body) = post_json(
        router.clone(),
        "/v1/memory/upsert",
        serde_json::json!({ "vector": [1.0, 2.0, 3.0, 4.0], "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let old_record_id = body["record_id"].as_u64().unwrap() as u32;

    let (status, _, body) = post_json(
        router,
        "/v1/memory/consolidate",
        serde_json::json!({
            "old_record_id": old_record_id,
            "new_vector": [5.0, 6.0, 7.0, 8.0],
            "collection": "tenant-a",
        }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // record_count() excludes soft-deleted slots, so after consolidate
    // (old soft-deleted, new inserted) exactly 1 LIVE record remains — and
    // it must be on tenant-a's OWN shard, proving the fix routes the whole
    // sequence there, not just to shard 0.
    let count = handle.shards[&shard_a].state_machine.with_state(|s| s.record_count()).await;
    assert_eq!(count, 1, "the new (live) record must be on tenant-a's shard");

    let count_shard0 = handle.shards[&valori_consensus::types::ShardId(0)]
        .state_machine
        .with_state(|s| s.record_count())
        .await;
    assert_eq!(count_shard0, 0, "none of tenant-a's consolidate traffic should touch shard 0's data");
}

/// Phase S5: cluster_insert_encrypted now routes ciphertext to the target
/// namespace's shard, so a single key_id's records can legitimately land
/// on different shards (one per collection it was used in). shred_key must
/// reach every shard, not just shard 0, for FLAG_SHREDDED to be a true
/// audit record of erasure everywhere the ciphertext actually lives.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shred_key_reaches_records_on_every_shard() {
    use base64::Engine as _;

    let handle = boot_leader_with_shards(3).await;
    let router = build_cluster_router(&handle, None);

    let (_, _, body_a) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-a" })).await;
    let (_, _, body_b) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-b" })).await;
    let ns_a = body_a["id"].as_u64().unwrap() as u16;
    let ns_b = body_b["id"].as_u64().unwrap() as u16;
    let shard_a = valori_consensus::types::ShardId((ns_a as u32) % 3);
    let shard_b = valori_consensus::types::ShardId((ns_b as u32) % 3);
    assert_ne!(shard_a, shard_b, "test setup must exercise two distinct shards");

    // Encrypted inserts store a zero-vector placeholder sized to the
    // kernel's already-locked dim — they cannot lock it themselves. Each
    // shard needs one plain insert first (a pre-existing kernel
    // constraint, unrelated to this phase).
    post_json(router.clone(), "/v1/memory/upsert",
        serde_json::json!({ "vector": [0.0, 0.0, 0.0, 0.0], "collection": "tenant-a" })).await;
    post_json(router.clone(), "/v1/memory/upsert",
        serde_json::json!({ "vector": [0.0, 0.0, 0.0, 0.0], "collection": "tenant-b" })).await;

    let key_id_hex = "aa".repeat(16); // 32 hex chars = 16 bytes
    let payload = base64::engine::general_purpose::STANDARD.encode(b"secret-a");

    let (status, _, body_ins_a) = post_json(
        router.clone(),
        "/v1/records/encrypted",
        serde_json::json!({ "payload": payload, "collection": "tenant-a", "key_id": key_id_hex }),
    ).await;
    assert_eq!(status, StatusCode::CREATED, "{body_ins_a}");
    let record_a = body_ins_a["id"].as_u64().unwrap() as u32;

    let (status, _, body_ins_b) = post_json(
        router.clone(),
        "/v1/records/encrypted",
        serde_json::json!({ "payload": payload, "collection": "tenant-b", "key_id": key_id_hex }),
    ).await;
    assert_eq!(status, StatusCode::CREATED, "{body_ins_b}");
    let record_b = body_ins_b["id"].as_u64().unwrap() as u32;

    // Before shredding: the record is not flagged.
    const FLAG_SHREDDED: u8 = 0x04;
    let flags_before_a = handle.shards[&shard_a].state_machine
        .with_state(move |s| s.get_record(valori_kernel::types::id::RecordId(record_a)).map(|r| r.flags))
        .await
        .expect("record_a must exist before shredding");
    assert_eq!(flags_before_a & FLAG_SHREDDED, 0, "record must not be pre-flagged shredded");

    let resp = router
        .oneshot(
            axum::http::Request::builder()
                .method(Method::DELETE)
                .uri(format!("/v1/crypto/shred/{key_id_hex}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["shredded"], true, "{body}");

    // The real proof: FLAG_SHREDDED must be set on BOTH records, on their
    // OWN independent shards — not just whichever shard used to be the
    // only one shred_key touched (shard 0, which neither record is on).
    let flags_a = handle.shards[&shard_a].state_machine
        .with_state(move |s| s.get_record(valori_kernel::types::id::RecordId(record_a)).map(|r| r.flags))
        .await
        .expect("record_a must still exist (shredded, not deleted)");
    let flags_b = handle.shards[&shard_b].state_machine
        .with_state(move |s| s.get_record(valori_kernel::types::id::RecordId(record_b)).map(|r| r.flags))
        .await
        .expect("record_b must still exist (shredded, not deleted)");
    assert_eq!(flags_a & FLAG_SHREDDED, FLAG_SHREDDED, "tenant-a's record must be shredded on its own shard");
    assert_eq!(flags_b & FLAG_SHREDDED, FLAG_SHREDDED, "tenant-b's record must be shredded on its own shard");
}

/// Phase S6: the read-index protocol (`GET /v1/cluster/read-index`) is now
/// shard-aware, and cluster_memory_search uses it (linearizable by
/// default) for whichever shard its collection resolves to — not just
/// shard 0. This is a single-node cluster so the leader path is exercised
/// (no actual follower round trip), but it proves a non-zero shard's read
/// index resolves correctly and doesn't hang/error, and that both the
/// default linearizable path and the explicit "local" opt-out both work.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_search_is_linearizable_on_a_non_zero_shard() {
    let handle = boot_leader_with_shards(3).await;
    let router = build_cluster_router(&handle, None);

    let (_, _, body) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-a" })).await;
    let ns_a = body["id"].as_u64().unwrap() as u16;
    assert_ne!(valori_consensus::types::ShardId((ns_a as u32) % 3), valori_consensus::types::ShardId(0));

    post_json(router.clone(), "/v1/memory/upsert",
        serde_json::json!({ "vector": [1.0, 2.0, 3.0, 4.0], "collection": "tenant-a" })).await;

    // Default (linearizable) consistency.
    let (status, _, body) = post_json(
        router.clone(),
        "/v1/memory/search",
        serde_json::json!({ "query_vector": [1.0, 2.0, 3.0, 4.0], "k": 5, "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["results"].as_array().unwrap().len(), 1, "{body}");

    // Explicit local (eventually consistent) opt-out must also work.
    let (status, _, body) = post_json(
        router,
        "/v1/memory/search",
        serde_json::json!({ "query_vector": [1.0, 2.0, 3.0, 4.0], "k": 5, "collection": "tenant-a", "consistency": "local" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["results"].as_array().unwrap().len(), 1, "{body}");

    // The read-index endpoint itself, queried directly for tenant-a's
    // shard, must succeed (proves the ?shard= query param routes to a
    // real, correctly-elected shard, not just shard 0 by accident).
    let shard_a = (ns_a as u32) % 3;
    let (status, body) = get_json(
        build_cluster_router(&handle, None),
        &format!("/v1/cluster/read-index?shard={shard_a}"),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["shard"], shard_a);
}

/// Phase S7: the core CRUD surface (`/v1/records`, `/v1/search`,
/// `/v1/soft-delete`, `/v1/vectors/batch-insert`) gained a `collection`
/// field and must route to that collection's data shard, not always shard
/// 0 — the same treatment already given to `/v1/memory/*` in S3b/S4.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn core_crud_routes_to_the_collections_shard() {
    let handle = boot_leader_with_shards(3).await;
    let router = build_cluster_router(&handle, None);

    let (_, _, body) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-a" })).await;
    let ns_a = body["id"].as_u64().unwrap() as u16;
    let shard_a = valori_consensus::types::ShardId((ns_a as u32) % 3);
    assert_ne!(shard_a, valori_consensus::types::ShardId(0));

    // Insert via /v1/records lands on tenant-a's own shard.
    let (status, _, body) = post_json(
        router.clone(),
        "/v1/records",
        serde_json::json!({ "values": [1.0, 2.0, 3.0, 4.0], "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id_a = body["id"].as_u64().unwrap() as u32;

    let count = handle.shards[&shard_a].state_machine.with_state(|s| s.record_count()).await;
    assert_eq!(count, 1, "insert_record must land on tenant-a's own shard");

    // Search via /v1/search finds it, scoped to tenant-a.
    let (status, _, body) = post_json(
        router.clone(),
        "/v1/search",
        serde_json::json!({ "query": [1.0, 2.0, 3.0, 4.0], "k": 5, "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["results"].as_array().unwrap().len(), 1, "{body}");

    // Batch insert also routes to tenant-a's shard.
    let (status, _, body) = post_json(
        router.clone(),
        "/v1/vectors/batch-insert",
        serde_json::json!({ "batch": [[5.0, 6.0, 7.0, 8.0]], "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let count = handle.shards[&shard_a].state_machine.with_state(|s| s.record_count()).await;
    assert_eq!(count, 2, "batch_insert must land on tenant-a's own shard too");

    // Soft-delete resolves via the collection field, not shard 0, and
    // mutates the right shard's kernel state.
    let (status, _, body) = post_json(
        router.clone(),
        "/v1/soft-delete",
        serde_json::json!({ "id": id_a, "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let count = handle.shards[&shard_a].state_machine.with_state(|s| s.record_count()).await;
    assert_eq!(count, 1, "soft-delete excludes the record from record_count on tenant-a's shard");

    let count_shard0 = handle.shards[&valori_consensus::types::ShardId(0)]
        .state_machine
        .with_state(|s| s.record_count())
        .await;
    assert_eq!(count_shard0, 0, "none of tenant-a's core CRUD traffic should touch shard 0's data");
}

/// Phase S8: graph node/edge create + read (`/v1/graph/node`,
/// `/v1/graph/node/:id`, `/v1/graph/edge`, `/v1/graph/edges/:id`,
/// `/v1/graph/subgraph`) must resolve `collection` and route to that
/// namespace's own data shard — node/edge ids are only unique within a
/// shard's own kernel state, same reasoning as core CRUD record ids (S7).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn graph_endpoints_route_to_the_collections_shard() {
    let handle = boot_leader_with_shards(3).await;
    let router = build_cluster_router(&handle, None);

    let (_, _, body) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-a" })).await;
    let ns_a = body["id"].as_u64().unwrap() as u16;
    let shard_a = valori_consensus::types::ShardId((ns_a as u32) % 3);
    assert_ne!(shard_a, valori_consensus::types::ShardId(0));

    let (status, _, body) = post_json(
        router.clone(),
        "/v1/graph/node",
        serde_json::json!({ "kind": 0, "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let node_1 = body["node_id"].as_u64().unwrap() as u32;

    let (status, _, body) = post_json(
        router.clone(),
        "/v1/graph/node",
        serde_json::json!({ "kind": 0, "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let node_2 = body["node_id"].as_u64().unwrap() as u32;

    let (status, _, body) = post_json(
        router.clone(),
        "/v1/graph/edge",
        serde_json::json!({ "from": node_1, "to": node_2, "kind": 0, "collection": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Both nodes and the edge must actually be on tenant-a's own shard.
    let (node_count, edge_count) = handle.shards[&shard_a]
        .state_machine
        .with_state(|s| (s.node_count(), s.edge_count()))
        .await;
    assert_eq!(node_count, 2, "both graph nodes must land on tenant-a's own shard");
    assert_eq!(edge_count, 1, "the edge must land on tenant-a's own shard");

    let count_shard0_nodes = handle.shards[&valori_consensus::types::ShardId(0)]
        .state_machine
        .with_state(|s| s.node_count())
        .await;
    assert_eq!(count_shard0_nodes, 0, "none of tenant-a's graph traffic should touch shard 0");

    // GET reads, scoped by ?collection=, must find them on the right shard.
    // Response shape is wire-compatible with the standalone server's
    // GetNodeResponse ({"kind","record_id","namespace_id"}, no "id" — the
    // caller already knows the id from the path) — see the fix in
    // get_graph_node's doc comment for why this matters (Python SDK
    // walk()/expand() read record_id specifically).
    let (status, body) = get_json(
        router.clone(),
        &format!("/v1/graph/node/{node_1}?collection=tenant-a"),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], 0);
    assert_eq!(body["namespace_id"], ns_a);

    let (status, body) = get_json(
        router.clone(),
        &format!("/v1/graph/edges/{node_1}?collection=tenant-a"),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let edges = body["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1, "{body}");
    // edge_id/to_node — matching standalone's EdgeData shape, not id/from/to.
    assert_eq!(edges[0]["to_node"], node_2);

    let (status, body) = get_json(
        router,
        &format!("/v1/graph/subgraph?root={node_1}&depth=2&collection=tenant-a"),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["nodes"].as_array().unwrap().len(), 2, "{body}");
}

/// Phase S8: `/v1/community/detect` with a named namespace must route the
/// scan to that namespace's own data shard and filter to it, instead of
/// always scanning shard 0 regardless of which shard the namespace's nodes
/// actually live on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn community_detect_scoped_to_namespace_scans_the_right_shard() {
    let handle = boot_leader_with_shards(3).await;
    let router = build_cluster_router(&handle, None);

    let (_, _, body) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-a" })).await;
    let ns_a = body["id"].as_u64().unwrap() as u16;
    assert_ne!(valori_consensus::types::ShardId((ns_a as u32) % 3), valori_consensus::types::ShardId(0));

    // Two nodes on tenant-a's shard.
    post_json(router.clone(), "/v1/graph/node",
        serde_json::json!({ "kind": 0, "collection": "tenant-a" })).await;
    post_json(router.clone(), "/v1/graph/node",
        serde_json::json!({ "kind": 0, "collection": "tenant-a" })).await;
    // One node on the default namespace, shard 0.
    post_json(router.clone(), "/v1/graph/node", serde_json::json!({ "kind": 0 })).await;

    let (status, _, body) = post_json(
        router.clone(),
        "/v1/community/detect",
        serde_json::json!({ "namespace": "tenant-a" }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["node_count"], 2, "detect scoped to tenant-a must find only tenant-a's 2 nodes: {body}");

    // Omitting namespace keeps the pre-S8 default: scans shard 0 only.
    let (status, _, body) = post_json(
        router,
        "/v1/community/detect",
        serde_json::json!({}),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["node_count"], 1, "unscoped detect must still default to shard 0 only: {body}");
}

/// Minimal OpenAI-compatible embed server for `cluster_ingest` coverage —
/// no real Ollama/OpenAI dependency needed to test the chunk+embed+insert
/// pipeline's shard routing. Echoes back one fixed 4-dim vector per input
/// text, which is all `cluster_ingest`'s routing logic needs to be provable.
async fn spawn_mock_embed_server() -> String {
    let router = axum::Router::new().route(
        "/v1/embeddings",
        axum::routing::post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
            let n = body["input"].as_array().map(|a| a.len()).unwrap_or(1);
            let data: Vec<serde_json::Value> = (0..n)
                .map(|_| serde_json::json!({ "embedding": [1.0, 2.0, 3.0, 4.0] }))
                .collect();
            axum::Json(serde_json::json!({ "data": data }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.ok();
    });
    format!("http://{addr}")
}

/// Phase S9: `/v1/ingest` (chunk + embed + insert + graph nodes) must route
/// its entire write sequence to the target collection's own shard — the
/// same treatment S4 already gave `cluster_memory_consolidate`/
/// `cluster_extract_entities`, but never had automated coverage because it
/// requires a configured embed provider. A minimal in-process mock embed
/// server (no external Ollama/OpenAI dependency) makes that coverage
/// possible.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ingest_routes_to_the_collections_shard() {
    let embed_url = spawn_mock_embed_server().await;

    let handle = boot_leader_with_shards(3).await;

    let mut node_cfg = valori_node::config::NodeConfig::default();
    node_cfg.embed_provider = Some("custom".into());
    node_cfg.embed_url = Some(embed_url);

    let router = build_cluster_router_with_keys(
        &handle,
        None,
        None,
        std::sync::Arc::new(valori_node::api_keys::KeyStore::new(None)),
        &node_cfg,
    );

    let (_, _, body) =
        post_json(router.clone(), "/v1/namespaces", serde_json::json!({ "name": "tenant-a" })).await;
    let ns_a = body["id"].as_u64().unwrap() as u16;
    let shard_a = valori_consensus::types::ShardId((ns_a as u32) % 3);
    assert_ne!(shard_a, valori_consensus::types::ShardId(0));

    let (status, _, body) = post_json(
        router,
        "/v1/ingest",
        serde_json::json!({
            "text": "Paragraph one has enough words to form a chunk on its own.\n\nParagraph two also has enough words to form a second, separate chunk.",
            "collection": "tenant-a",
        }),
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let chunk_count = body["chunk_count"].as_u64().unwrap();
    assert!(chunk_count >= 1, "{body}");

    // Every inserted chunk record + the document/chunk graph nodes must land
    // on tenant-a's own shard, not shard 0.
    let (record_count, node_count) = handle.shards[&shard_a]
        .state_machine
        .with_state(|s| (s.record_count(), s.node_count()))
        .await;
    assert_eq!(record_count as u64, chunk_count, "every ingested chunk must land on tenant-a's shard");
    assert!(node_count >= 1 + chunk_count as usize, "document node + one chunk node per chunk, all on tenant-a's shard");

    let shard0_records = handle.shards[&valori_consensus::types::ShardId(0)]
        .state_machine
        .with_state(|s| s.record_count())
        .await;
    assert_eq!(shard0_records, 0, "none of tenant-a's ingest traffic should touch shard 0's data");
}
