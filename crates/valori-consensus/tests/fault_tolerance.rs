// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.8 — fault tolerance, process-level: nodes are killed by shutting
//! down their Raft core AND aborting their gRPC server, so peers see real
//! connection failures on real sockets.
//!
//! Covered here:
//! - leader crash → re-election among survivors → writes continue
//! - minority follower loss → quorum holds → writes continue
//! - majority loss → writes stall (no quorum) rather than fork
//!
//! True network *partitions* (both sides alive but separated) and
//! crash-*restart* need a simulated transport and a persistent Raft log
//! respectively — both are Phase 2.10 (turmoil-style harness + redb store).

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
    server: tokio::task::JoinHandle<()>,
}

impl TestNode {
    /// Hard-kill: stop the Raft core and the gRPC listener. Peers get
    /// connection errors, exactly like a crashed process.
    async fn kill(&self) {
        let _ = self.raft.shutdown().await;
        self.server.abort();
    }
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
    let sm = ValoriStateMachine::default();
    let raft = Raft::new(id, config, ValoriNetworkFactory, ValoriLogStore::new(), sm.clone())
        .await
        .unwrap();
    let (addr, server) = serve_raft(raft.clone(), "127.0.0.1:0").await.unwrap();
    TestNode {
        raft,
        sm,
        node: ValoriNode {
            api_addr: String::new(),
            raft_addr: addr.to_string(),
        },
        server,
    }
}

async fn three_node_cluster() -> Vec<TestNode> {
    let nodes = vec![spawn_node(1).await, spawn_node(2).await, spawn_node(3).await];
    let members: BTreeMap<NodeId, ValoriNode> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| ((i + 1) as NodeId, n.node.clone()))
        .collect();
    nodes[0].raft.initialize(members).await.unwrap();
    nodes[0]
        .raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader.is_some(), "leader elected")
        .await
        .unwrap();
    nodes
}

fn leader_index(nodes: &[TestNode]) -> usize {
    let id = nodes[0]
        .raft
        .metrics()
        .borrow()
        .current_leader
        .or_else(|| nodes[1].raft.metrics().borrow().current_leader)
        .expect("a leader exists");
    (id - 1) as usize
}

fn insert(id: u32) -> ClientRequest {
    ClientRequest {
        event: KernelEvent::InsertRecord {
            id: RecordId(id),
            vector: FxpVector::new_zeros(4),
            metadata: None,
            tag: id as u64,
        },
        request_id: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn leader_crash_triggers_reelection_and_writes_continue() {
    let nodes = three_node_cluster().await;
    let old_leader = leader_index(&nodes);

    // Write through the original leader first.
    let mut next_record = 0u32;
    for _ in 0..3 {
        nodes[old_leader].raft.client_write(insert(next_record)).await.unwrap();
        next_record += 1;
    }

    // Crash the leader.
    nodes[old_leader].kill().await;

    // A survivor must win an election (new term, new leader ≠ old).
    let survivors: Vec<usize> = (0..3).filter(|i| *i != old_leader).collect();
    let new_leader_id = {
        let w = nodes[survivors[0]]
            .raft
            .wait(Some(Duration::from_secs(10)))
            .metrics(
                |m| m.current_leader.is_some() && m.current_leader != Some((old_leader + 1) as NodeId),
                "survivor elected a new leader",
            )
            .await
            .unwrap();
        w.current_leader.unwrap()
    };
    let new_leader = &nodes[(new_leader_id - 1) as usize];

    // Writes continue through the new leader.
    let mut last_index = 0;
    for _ in 0..3 {
        last_index = new_leader
            .raft
            .client_write(insert(next_record))
            .await
            .unwrap()
            .data
            .log_index;
        next_record += 1;
    }

    // Both survivors converge: 6 records, one hash.
    for &i in &survivors {
        nodes[i]
            .raft
            .wait(Some(Duration::from_secs(10)))
            .applied_index_at_least(Some(last_index), "survivor caught up")
            .await
            .unwrap();
        assert_eq!(nodes[i].sm.with_state(|s| s.record_count()).await, 6);
    }
    assert_eq!(
        nodes[survivors[0]].sm.state_hash().await,
        nodes[survivors[1]].sm.state_hash().await,
        "survivors must agree byte-for-byte after the failover"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn minority_follower_loss_does_not_stop_writes() {
    let nodes = three_node_cluster().await;
    let leader = leader_index(&nodes);
    let follower = (0..3).find(|i| *i != leader).unwrap();

    nodes[follower].kill().await;

    // 2 of 3 alive — quorum holds; writes commit normally.
    let mut last_index = 0;
    for i in 0..5u32 {
        last_index = nodes[leader]
            .raft
            .client_write(insert(i))
            .await
            .unwrap()
            .data
            .log_index;
    }

    let other_survivor = (0..3).find(|i| *i != leader && *i != follower).unwrap();
    nodes[other_survivor]
        .raft
        .wait(Some(Duration::from_secs(10)))
        .applied_index_at_least(Some(last_index), "surviving follower caught up")
        .await
        .unwrap();
    assert_eq!(
        nodes[leader].sm.state_hash().await,
        nodes[other_survivor].sm.state_hash().await,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn majority_loss_stalls_writes_instead_of_forking() {
    let nodes = three_node_cluster().await;
    let leader = leader_index(&nodes);
    let followers: Vec<usize> = (0..3).filter(|i| *i != leader).collect();

    // Baseline write while healthy.
    nodes[leader].raft.client_write(insert(0)).await.unwrap();
    let count_before = nodes[leader].sm.with_state(|s| s.record_count()).await;

    // Kill BOTH followers — the leader is alone, no quorum exists.
    nodes[followers[0]].kill().await;
    nodes[followers[1]].kill().await;

    // A write must not commit: either it errors, or it hangs past a
    // deadline (openraft keeps retrying replication). Both are correct —
    // what is FORBIDDEN is returning success.
    let write = nodes[leader].raft.client_write(insert(1));
    match tokio::time::timeout(Duration::from_secs(3), write).await {
        Err(_elapsed) => {} // hung waiting for quorum — correct
        Ok(Err(_raft_err)) => {} // surfaced an error — correct
        Ok(Ok(resp)) => panic!(
            "write committed without quorum (log_index {}) — safety violation",
            resp.data.log_index
        ),
    }

    // And the lone leader's state machine must not have applied it.
    assert_eq!(
        nodes[leader].sm.with_state(|s| s.record_count()).await,
        count_before,
        "no quorum → no apply"
    );
}
