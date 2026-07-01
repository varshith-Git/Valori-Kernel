// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.6 — the cluster management API over a real running cluster.
//!
//! Boots actual nodes (gRPC servers, elections, the lot), then drives the
//! axum endpoints with tower::oneshot: status, health, add-node (grow 1→2),
//! remove-node (shrink back), last-voter protection, and follower rejection.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

use valori_consensus::types::ValoriNode;
use valori_consensus::NullAuditSink;
use valori_node::cluster::{bootstrap_cluster, ClusterConfig, ClusterHandle};
use valori_node::cluster_api::cluster_router;
use valori_node::commit::Committer;

use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;

async fn boot(node_id: u64, init: bool) -> ClusterHandle {
    let cfg = ClusterConfig {
        node_id,
        raft_bind: "127.0.0.1:0".into(),
        members: [(node_id, ValoriNode::default())].into_iter().collect(),
        init,
        raft_log_path: None,
        tls: None,
        shard_count: 1,
    };
    bootstrap_cluster(&cfg, Box::new(NullAuditSink), 0).await.unwrap()
}

async fn wait_for_leader(h: &ClusterHandle, id: u64) {
    h.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(id), "leader elected")
        .await
        .unwrap();
}

async fn get_json(
    router: axum::Router,
    uri: &str,
) -> (StatusCode, serde_json::Value) {
    let resp = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn post_json(
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
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

// ── Status & health ───────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_reports_leadership_and_membership() {
    let h = boot(1, true).await;
    wait_for_leader(&h, 1).await;

    let router = cluster_router(Arc::new(h.raft.clone()), Arc::new(std::collections::BTreeMap::from([(valori_consensus::types::ShardId(0), h.raft.clone())])), None);
    let (status, body) = get_json(router, "/v1/cluster/status").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["node_id"], 1);
    assert_eq!(body["current_leader"], 1);
    assert_eq!(body["is_leader"], true);
    assert_eq!(body["members"].as_array().unwrap().len(), 1);
    assert_eq!(body["members"][0]["voter"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn health_is_503_without_a_leader_and_200_with_one() {
    // Booted but NOT initialized: no membership, no election, no leader.
    let h = boot(1, false).await;
    let router = cluster_router(Arc::new(h.raft.clone()), Arc::new(std::collections::BTreeMap::from([(valori_consensus::types::ShardId(0), h.raft.clone())])), None);
    let (status, body) = get_json(router.clone(), "/v1/cluster/health").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "no-leader");

    // Initialize → leader emerges → healthy.
    h.raft
        .initialize(
            [(1u64, ValoriNode::default())]
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
        .await
        .unwrap();
    wait_for_leader(&h, 1).await;

    let (status, body) = get_json(router, "/v1/cluster/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

// ── Membership changes ────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn add_node_grows_the_cluster_and_replicates_state() {
    // Node 1 starts alone and leads.
    let h1 = boot(1, true).await;
    wait_for_leader(&h1, 1).await;

    // Write one record before node 2 exists.
    let mut committer = h1.committer();
    committer
        .commit(KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::new_zeros(4),
            metadata: None,
            tag: 0,
        })
        .unwrap();

    // Node 2 boots empty (no init — it joins, never initializes).
    let h2 = boot(2, false).await;

    let router = cluster_router(Arc::new(h1.raft.clone()), Arc::new(std::collections::BTreeMap::from([(valori_consensus::types::ShardId(0), h1.raft.clone())])), None);
    let (status, body) = post_json(
        router.clone(),
        "/v1/cluster/add-node",
        serde_json::json!({ "node_id": 2, "raft_addr": h2.raft_addr.to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "add-node failed: {body}");
    assert_eq!(body["status"], "added");

    // The new member appears in status as a voter.
    let (_, status_body) = get_json(router, "/v1/cluster/status").await;
    let members = status_body["members"].as_array().unwrap();
    assert_eq!(members.len(), 2, "both nodes in membership: {status_body}");

    // And node 2 catches up to the pre-join write — same kernel state.
    h2.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.last_applied.is_some(), "node 2 applied the log")
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if h2.state_machine.with_state(|s| s.record_count()).await == 1 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "node 2 never caught up");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        h1.state_machine.state_hash().await,
        h2.state_machine.state_hash().await,
        "joined node must converge to the leader's exact state hash"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_node_shrinks_the_cluster() {
    let h1 = boot(1, true).await;
    wait_for_leader(&h1, 1).await;
    let h2 = boot(2, false).await;

    let router = cluster_router(Arc::new(h1.raft.clone()), Arc::new(std::collections::BTreeMap::from([(valori_consensus::types::ShardId(0), h1.raft.clone())])), None);
    let (status, _) = post_json(
        router.clone(),
        "/v1/cluster/add-node",
        serde_json::json!({ "node_id": 2, "raft_addr": h2.raft_addr.to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post_json(
        router.clone(),
        "/v1/cluster/remove-node",
        serde_json::json!({ "node_id": 2 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "remove failed: {body}");
    assert_eq!(body["status"], "removed");

    let (_, status_body) = get_json(router, "/v1/cluster/status").await;
    let voters: Vec<_> = status_body["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["voter"] == true)
        .collect();
    assert_eq!(voters.len(), 1, "only node 1 remains a voter: {status_body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_the_last_voter_is_refused() {
    let h = boot(1, true).await;
    wait_for_leader(&h, 1).await;

    let router = cluster_router(Arc::new(h.raft.clone()), Arc::new(std::collections::BTreeMap::from([(valori_consensus::types::ShardId(0), h.raft.clone())])), None);
    let (status, body) = post_json(
        router,
        "/v1/cluster/remove-node",
        serde_json::json!({ "node_id": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "cannot-remove-last-voter");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn membership_change_on_an_uninitialized_node_is_forbidden() {
    // Never initialized: not a leader, can't change membership.
    let h = boot(1, false).await;
    let router = cluster_router(Arc::new(h.raft.clone()), Arc::new(std::collections::BTreeMap::from([(valori_consensus::types::ShardId(0), h.raft.clone())])), None);
    let (status, body) = post_json(
        router,
        "/v1/cluster/add-node",
        serde_json::json!({ "node_id": 2, "raft_addr": "127.0.0.1:1" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "got: {body}");
}

// ── Phase 2.9: membership changes land in the chained audit log ──────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn membership_changes_are_chained_admin_events() {
    use valori_node::events::event_log::EventLogWriter;
    use valori_wire::{chain_advance, decode_entry, parse_header, AdminEvent, LogEntry};

    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("events.log");
    let audit = Arc::new(std::sync::Mutex::new(
        EventLogWriter::open(&log_path, Some(4)).unwrap(),
    ));

    let h1 = boot(1, true).await;
    wait_for_leader(&h1, 1).await;
    let h2 = boot(2, false).await;

    let router = cluster_router(Arc::new(h1.raft.clone()), Arc::new(std::collections::BTreeMap::from([(valori_consensus::types::ShardId(0), h1.raft.clone())])), Some(audit.clone()));

    // Join then remove node 2 — both must be audited.
    let (status, _) = post_json(
        router.clone(),
        "/v1/cluster/add-node",
        serde_json::json!({ "node_id": 2, "raft_addr": h2.raft_addr.to_string(), "api_addr": "10.0.0.2:3000" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post_json(
        router,
        "/v1/cluster/remove-node",
        serde_json::json!({ "node_id": 2 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Read the log back raw and walk the chain: two admin events, in order,
    // chain unbroken — membership history is tamper-evident like data.
    let bytes = std::fs::read(&log_path).unwrap();
    let header = parse_header(&bytes).unwrap();
    let mut offset = header.header_len;
    let mut chain_head = header.prev_segment_chain_head;
    let mut admin_events = Vec::new();

    while offset < bytes.len() {
        let (decoded, n) = decode_entry(header.version, &bytes[offset..]).unwrap();
        assert_eq!(decoded.prev_hash, chain_head, "chain must be unbroken");
        chain_head = chain_advance(header.version, &chain_head, &decoded).unwrap();
        offset += n;
        if let LogEntry::Admin(a) = decoded.entry {
            admin_events.push(a);
        }
    }

    assert_eq!(admin_events.len(), 2, "join + leave both audited");
    match &admin_events[0] {
        AdminEvent::NodeJoined { node_id, api_addr, .. } => {
            assert_eq!(*node_id, 2);
            assert_eq!(api_addr, "10.0.0.2:3000");
        }
        other => panic!("expected NodeJoined first, got {other:?}"),
    }
    match &admin_events[1] {
        AdminEvent::NodeLeft { node_id, .. } => assert_eq!(*node_id, 2),
        other => panic!("expected NodeLeft second, got {other:?}"),
    }
}

#[tokio::test]
async fn role_endpoint_returns_leader_or_follower() {
    let h = boot(1, true).await;
    wait_for_leader(&h, 1).await;

    let router = cluster_router(Arc::new(h.raft.clone()), Arc::new(std::collections::BTreeMap::from([(valori_consensus::types::ShardId(0), h.raft.clone())])), None);
    let (status, body) = get_json(router, "/v1/cluster/role").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["role"], "leader", "single-node cluster should be leader");
    assert_eq!(body["node_id"], 1u64);
}
