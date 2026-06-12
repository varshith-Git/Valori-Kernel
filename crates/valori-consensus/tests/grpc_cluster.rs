// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.4 — a real 3-node Raft cluster over gRPC on localhost.
//!
//! Each node: ValoriLogStore + ValoriStateMachine + tonic server on an
//! OS-assigned port, peers connected through ValoriNetworkFactory. The
//! acceptance gate: leader election over the wire, replicated writes, and
//! all three kernels converging to the same BLAKE3 state hash.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use openraft::{Config, Raft};

use valori_consensus::types::{ClientRequest, NodeId, TypeConfig, ValoriNode};
use valori_consensus::{serve_raft, ValoriLogStore, ValoriNetworkFactory, ValoriStateMachine};
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;

struct TestNode {
    raft: Raft<TypeConfig>,
    sm: ValoriStateMachine,
    node: ValoriNode,
}

async fn spawn_node(id: NodeId) -> TestNode {
    let config = Arc::new(
        Config {
            heartbeat_interval: 100,
            election_timeout_min: 250,
            election_timeout_max: 500,
            ..Default::default()
        }
        .validate()
        .unwrap(),
    );

    let log_store = ValoriLogStore::new();
    let sm = ValoriStateMachine::default();

    let raft = Raft::new(id, config, ValoriNetworkFactory, log_store, sm.clone())
        .await
        .unwrap();

    let (addr, _handle) = serve_raft(raft.clone(), "127.0.0.1:0").await.unwrap();

    TestNode {
        raft,
        sm,
        node: ValoriNode {
            api_addr: String::new(),
            raft_addr: addr.to_string(),
        },
    }
}

fn insert(id: u32) -> ClientRequest {
    ClientRequest {
        event: KernelEvent::InsertRecord {
            id: RecordId(id),
            vector: FxpVector::new_zeros(4),
            metadata: Some(vec![id as u8]),
            tag: id as u64,
        },
        request_id: None,
    }
}

/// Spin up a 3-node cluster, elect a leader, return the nodes.
async fn three_node_cluster() -> Vec<TestNode> {
    let nodes = vec![spawn_node(1).await, spawn_node(2).await, spawn_node(3).await];

    let members: BTreeMap<NodeId, ValoriNode> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| ((i + 1) as NodeId, n.node.clone()))
        .collect();

    // Initialize the cluster through node 1 — this triggers the first
    // election over real gRPC.
    nodes[0].raft.initialize(members).await.unwrap();

    // Wait for a leader to emerge.
    nodes[0]
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader.is_some(), "leader elected")
        .await
        .unwrap();

    nodes
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_nodes_elect_a_leader_over_grpc() {
    let nodes = three_node_cluster().await;
    let leader = nodes[0].raft.metrics().borrow().current_leader;
    assert!(leader.is_some(), "a leader must be elected over the wire");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_replicate_and_all_kernels_converge_to_one_hash() {
    let nodes = three_node_cluster().await;
    let leader_id = nodes[0].raft.metrics().borrow().current_leader.unwrap();
    let leader = &nodes[(leader_id - 1) as usize];

    // Write 10 events through the leader, keeping the authoritative log
    // index of the final write from its own response (leader metrics can
    // lag a hair behind the apply).
    let mut last_index = 0;
    for i in 0..10u32 {
        let resp = leader.raft.client_write(insert(i)).await.unwrap();
        assert!(!resp.data.deduplicated);
        last_index = resp.data.log_index;
    }
    for node in &nodes {
        node.raft
            .wait(Some(Duration::from_secs(10)))
            .applied_index_at_least(Some(last_index), "follower caught up")
            .await
            .unwrap();
    }

    // The SMR invariant, over a real network: one hash, three nodes.
    let h1 = nodes[0].sm.state_hash().await;
    let h2 = nodes[1].sm.state_hash().await;
    let h3 = nodes[2].sm.state_hash().await;
    assert_eq!(h1, h2, "node 2 diverged from node 1");
    assert_eq!(h2, h3, "node 3 diverged from node 2");

    // And every kernel holds the 10 records.
    for node in &nodes {
        assert_eq!(node.sm.with_state(|s| s.record_count()).await, 10);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_request_id_is_deduplicated_across_the_cluster() {
    let nodes = three_node_cluster().await;
    let leader_id = nodes[0].raft.metrics().borrow().current_leader.unwrap();
    let leader = &nodes[(leader_id - 1) as usize];

    let rid = [7u8; 16];
    let mut req = insert(0);
    req.request_id = Some(rid);
    let first = leader.raft.client_write(req).await.unwrap();
    assert!(!first.data.deduplicated);

    // Same idempotency token again (client retry after a timeout).
    let mut retry = insert(1);
    retry.request_id = Some(rid);
    let second = leader.raft.client_write(retry).await.unwrap();
    assert!(second.data.deduplicated, "retry must be recognised by request_id");

    assert_eq!(
        leader.sm.with_state(|s| s.record_count()).await,
        1,
        "the retry must not double-apply"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_to_a_follower_is_redirected_to_the_leader() {
    let nodes = three_node_cluster().await;
    let leader_id = nodes[0].raft.metrics().borrow().current_leader.unwrap();
    let follower = nodes
        .iter()
        .enumerate()
        .find(|(i, _)| (*i + 1) as NodeId != leader_id)
        .map(|(_, n)| n)
        .unwrap();

    // The leader knows it's leader before the followers hear the first
    // heartbeat — wait until THIS follower has learned who leads.
    follower
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(
            |m| m.current_leader == Some(leader_id),
            "follower learned the leader",
        )
        .await
        .unwrap();

    let err = follower.raft.client_write(insert(0)).await.unwrap_err();
    let forward = err.forward_to_leader().expect("ForwardToLeader error expected");
    assert_eq!(forward.leader_id, Some(leader_id), "error names the real leader");
    assert!(
        forward.leader_node.is_some(),
        "error carries the leader's addresses for the client to retry against"
    );
}
