// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase S1 — multiple independent Raft groups over ONE shared gRPC
//! listener per node.
//!
//! This is the only test that exercises the new `RaftRpcService` shard
//! dispatch table (`network.rs`): each simulated node runs TWO shards'
//! `Raft` instances behind a single `serve_raft(HashMap<ShardId, Raft>, addr)`
//! listener — the same topology `bootstrap_cluster` produces in production.
//! A test built around two independent `serve_raft_single` clusters would
//! never catch a `shard_id` routing bug, since two separate listeners are
//! isolated by construction; this test is deliberately NOT that.
//!
//! Acceptance gates:
//! - each shard elects its own leader independently, over the same wire
//! - a write to shard 0 never appears in shard 1's state (isolation)
//! - each shard's BLAKE3 state hash converges across its own 3 replicas,
//!   and the two shards' converged hashes differ (no cross-shard bleed)
//! - shard 0's Raft core failing on a node does not affect shard 1's
//!   liveness on that same node (shared listener, independent groups)

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use openraft::{Config, Raft};

use valori_consensus::types::{ClientRequest, NodeId, ShardId, TypeConfig, ValoriNode};
use valori_consensus::{serve_raft, ValoriLogStore, ValoriNetworkFactory, ValoriStateMachine};
use valori_kernel::event::KernelEvent;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::vector::FxpVector;

const SHARD0: ShardId = ShardId(0);
const SHARD1: ShardId = ShardId(1);

struct ShardRaft {
    raft: Raft<TypeConfig>,
    sm: ValoriStateMachine,
}

/// One simulated cluster node running both shards behind one listener.
struct ShardedNode {
    shards: HashMap<ShardId, ShardRaft>,
    node: ValoriNode,
}

async fn spawn_sharded_node(id: NodeId) -> ShardedNode {
    let mut shards = HashMap::new();
    let mut raft_instances: HashMap<ShardId, Raft<TypeConfig>> = HashMap::new();

    for shard_id in [SHARD0, SHARD1] {
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
        let raft = Raft::new(
            id,
            config,
            ValoriNetworkFactory::new(shard_id),
            ValoriLogStore::new(),
            sm.clone(),
        )
        .await
        .unwrap();

        raft_instances.insert(shard_id, raft.clone());
        shards.insert(shard_id, ShardRaft { raft, sm });
    }

    // ONE shared gRPC listener multiplexing both shards — the production
    // topology (bootstrap_cluster calls serve_raft once per node, not once
    // per shard).
    let (addr, _handle) = serve_raft(raft_instances, "127.0.0.1:0").await.unwrap();

    ShardedNode {
        shards,
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
        schema_version: 0,
    namespace_id: 0,
    }
}

/// Spin up 3 sharded nodes (2 shards each), initialize BOTH shards'
/// Raft groups independently over the same membership addresses, and wait
/// for each shard to elect its own leader.
async fn two_shard_cluster() -> Vec<ShardedNode> {
    let nodes = vec![
        spawn_sharded_node(1).await,
        spawn_sharded_node(2).await,
        spawn_sharded_node(3).await,
    ];

    let members: BTreeMap<NodeId, ValoriNode> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| ((i + 1) as NodeId, n.node.clone()))
        .collect();

    for shard_id in [SHARD0, SHARD1] {
        nodes[0].shards[&shard_id]
            .raft
            .initialize(members.clone())
            .await
            .unwrap();
    }

    for shard_id in [SHARD0, SHARD1] {
        nodes[0].shards[&shard_id]
            .raft
            .wait(Some(Duration::from_secs(10)))
            .metrics(|m| m.current_leader.is_some(), "leader elected")
            .await
            .unwrap();
    }

    nodes
}

fn leader_for<'a>(nodes: &'a [ShardedNode], shard: ShardId) -> &'a ShardedNode {
    let leader_id = nodes[0].shards[&shard].raft.metrics().borrow().current_leader.unwrap();
    &nodes[(leader_id - 1) as usize]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn each_shard_elects_its_own_leader_independently() {
    let nodes = two_shard_cluster().await;
    let leader0 = nodes[0].shards[&SHARD0].raft.metrics().borrow().current_leader;
    let leader1 = nodes[0].shards[&SHARD1].raft.metrics().borrow().current_leader;
    assert!(leader0.is_some(), "shard 0 must elect a leader over the shared listener");
    assert!(leader1.is_some(), "shard 1 must elect a leader over the shared listener");
    // Deliberately not asserting leader0 != leader1 or leader0 == leader1 —
    // either is a valid outcome; only independent convergence is asserted.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn write_to_shard_0_does_not_appear_in_shard_1() {
    let nodes = two_shard_cluster().await;
    let leader0 = leader_for(&nodes, SHARD0);

    let empty_hash = nodes[0].shards[&SHARD1].sm.state_hash().await;

    let resp = leader0.shards[&SHARD0].raft.client_write(insert(0)).await.unwrap();
    for node in &nodes {
        node.shards[&SHARD0]
            .raft
            .wait(Some(Duration::from_secs(10)))
            .applied_index_at_least(Some(resp.data.log_index), "shard 0 caught up")
            .await
            .unwrap();
    }

    // Shard 0 saw the write...
    assert_eq!(nodes[0].shards[&SHARD0].sm.with_state(|s| s.record_count()).await, 1);
    // ...shard 1, on every node, did not — still the empty-state hash.
    for node in &nodes {
        assert_eq!(
            node.shards[&SHARD1].sm.state_hash().await,
            empty_hash,
            "shard 1 must be untouched by a shard 0 write"
        );
        assert_eq!(node.shards[&SHARD1].sm.with_state(|s| s.record_count()).await, 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn each_shard_converges_independently_and_hashes_differ() {
    let nodes = two_shard_cluster().await;

    // Different event counts per shard so a hash collision would be a real
    // bug, not a coincidence of identical inputs.
    let leader0 = leader_for(&nodes, SHARD0);
    let mut last0 = 0;
    for i in 0..5u32 {
        last0 = leader0.shards[&SHARD0].raft.client_write(insert(i)).await.unwrap().data.log_index;
    }
    let leader1 = leader_for(&nodes, SHARD1);
    let mut last1 = 0;
    for i in 0..3u32 {
        last1 = leader1.shards[&SHARD1].raft.client_write(insert(i)).await.unwrap().data.log_index;
    }

    for node in &nodes {
        node.shards[&SHARD0]
            .raft
            .wait(Some(Duration::from_secs(10)))
            .applied_index_at_least(Some(last0), "shard 0 converged")
            .await
            .unwrap();
        node.shards[&SHARD1]
            .raft
            .wait(Some(Duration::from_secs(10)))
            .applied_index_at_least(Some(last1), "shard 1 converged")
            .await
            .unwrap();
    }

    let mut shard0_hashes = Vec::new();
    for node in &nodes {
        shard0_hashes.push(node.shards[&SHARD0].sm.state_hash().await);
    }
    let mut shard1_hashes = Vec::new();
    for node in &nodes {
        shard1_hashes.push(node.shards[&SHARD1].sm.state_hash().await);
    }

    assert!(shard0_hashes.windows(2).all(|w| w[0] == w[1]), "shard 0 replicas diverged");
    assert!(shard1_hashes.windows(2).all(|w| w[0] == w[1]), "shard 1 replicas diverged");
    assert_ne!(
        shard0_hashes[0], shard1_hashes[0],
        "shard 0 and shard 1 applied different events — equal hashes would mean cross-shard contamination"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn shard_0_leader_failure_does_not_affect_shard_1_liveness() {
    let nodes = two_shard_cluster().await;
    let leader1_before = nodes[0].shards[&SHARD1].raft.metrics().borrow().current_leader.unwrap();

    // Hard-fail shard 0's leader Raft core only — its shard 1 Raft core and
    // the shared gRPC listener on that node stay up, exactly like one Raft
    // group crashing while its host process (and the other shard) survives.
    {
        let leader0 = leader_for(&nodes, SHARD0);
        leader0.shards[&SHARD0].raft.shutdown().await.ok();
    }

    // Shard 1 must be unaffected: still writable, same or a legitimately
    // re-elected leader, no disruption from shard 0's failure.
    let leader1 = leader_for(&nodes, SHARD1);
    let resp = leader1.shards[&SHARD1].raft.client_write(insert(99)).await.unwrap();
    for node in &nodes {
        node.shards[&SHARD1]
            .raft
            .wait(Some(Duration::from_secs(10)))
            .applied_index_at_least(Some(resp.data.log_index), "shard 1 unaffected by shard 0 failure")
            .await
            .unwrap();
    }
    let _ = leader1_before; // sanity anchor only; shard 1 leadership is independent of shard 0's.
}
