// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.7 — snapshot transfer: a node joining AFTER the leader has
//! snapshotted and purged its log cannot catch up by log replay — the
//! entries are gone. It must receive the leader's snapshot over gRPC
//! (`InstallSnapshot`), decode it through the V5 + BLAKE3 self-verification
//! gates (Phase 2.3), and converge to the leader's exact state hash.
//!
//! Everything here runs over real sockets — the same transport production
//! uses.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use openraft::{Config, Raft};

use valori_consensus::types::{ClientRequest, NodeId, ShardId, TypeConfig, ValoriNode};
use valori_consensus::{serve_raft_single, ValoriLogStore, ValoriNetworkFactory, ValoriStateMachine};
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
            // Keep nothing extra in the log after a snapshot — forces the
            // install-snapshot path for any latecomer.
            max_in_snapshot_log_to_keep: 0,
            ..Default::default()
        }
        .validate()
        .unwrap(),
    );

    let sm = ValoriStateMachine::default();
    let raft = Raft::new(id, config, ValoriNetworkFactory::new(ShardId(0)), ValoriLogStore::new(), sm.clone())
        .await
        .unwrap();
    let (addr, _task) = serve_raft_single(raft.clone(), "127.0.0.1:0").await.unwrap();

    TestNode {
        raft,
        sm,
        node: ValoriNode {
            api_addr: String::new(),
            raft_addr: addr.to_string(),
        },
    }
}

fn insert(id: u32, rid: Option<[u8; 16]>) -> ClientRequest {
    ClientRequest {
        event: KernelEvent::InsertRecord {
            id: RecordId(id),
            vector: FxpVector::new_zeros(4),
            metadata: Some(vec![id as u8]),
            tag: id as u64,
        },
        request_id: rid,
        schema_version: 0,
    namespace_id: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn late_joiner_catches_up_via_snapshot_after_log_purge() {
    // ── Leader alone: write history, snapshot, purge ─────────────────────────
    let n1 = spawn_node(1).await;
    let members: BTreeMap<NodeId, ValoriNode> = [(1, n1.node.clone())].into_iter().collect();
    n1.raft.initialize(members).await.unwrap();
    n1.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();

    let mut last_index = 0;
    for i in 0..20u32 {
        let resp = n1
            .raft
            .client_write(insert(i, Some([i as u8; 16])))
            .await
            .unwrap();
        last_index = resp.data.log_index;
    }

    // Snapshot the state machine, then purge the Raft log up to it.
    n1.raft.trigger().snapshot().await.unwrap();
    n1.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(
            |m| m.snapshot.map_or(0, |s| s.index) >= last_index,
            "snapshot covers the writes",
        )
        .await
        .unwrap();
    n1.raft.trigger().purge_log(last_index).await.unwrap();
    n1.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(
            |m| m.purged.map_or(0, |p| p.index) >= last_index,
            "log purged past the writes",
        )
        .await
        .unwrap();

    let leader_hash = n1.sm.state_hash().await;

    // ── Latecomer joins: the log it needs no longer exists ───────────────────
    let n2 = spawn_node(2).await;
    n1.raft
        .add_learner(2, n2.node.clone(), true)
        .await
        .expect("learner must catch up via InstallSnapshot");

    // Convergence: node 2 received and installed the leader's snapshot.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if n2.sm.with_state(|s| s.record_count()).await == 20 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "late joiner never received the snapshot"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        n2.sm.state_hash().await,
        leader_hash,
        "snapshot-installed state must hash identically to the leader's"
    );

    // The joiner's snapshot is recorded as its baseline.
    let (last_applied, _) = {
        use openraft::storage::RaftStateMachine;
        let mut sm = n2.sm.clone();
        sm.applied_state().await.unwrap()
    };
    assert!(
        last_applied.map_or(0, |l| l.index) >= last_index,
        "joiner's last_applied must cover the snapshotted history"
    );

    // ── The dedup table travelled in the snapshot ────────────────────────────
    // Promote node 2 to voter, then retry a pre-snapshot request_id through
    // the cluster: every node (including the snapshot-restored one) must
    // recognise it as a duplicate when applying.
    n1.raft
        .change_membership([1, 2].into_iter().collect::<std::collections::BTreeSet<_>>(), false)
        .await
        .unwrap();

    let retry = n1.raft.client_write(insert(99, Some([5u8; 16]))).await.unwrap();
    assert!(
        retry.data.deduplicated,
        "request_id from before the snapshot must still deduplicate"
    );
    assert_eq!(n1.sm.with_state(|s| s.record_count()).await, 20);

    // Node 2 applied the same dedup decision — counts stay equal.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let h1 = n1.sm.state_hash().await;
        let h2 = n2.sm.state_hash().await;
        if h1 == h2 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "node 2 diverged after dedup");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_then_more_writes_joiner_gets_snapshot_plus_tail() {
    // History = snapshot + a live tail: the joiner needs BOTH transfer paths
    // (install snapshot, then ordinary log replication for the tail).
    let n1 = spawn_node(1).await;
    n1.raft
        .initialize(
            [(1, n1.node.clone())].into_iter().collect::<BTreeMap<NodeId, ValoriNode>>(),
        )
        .await
        .unwrap();
    n1.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.current_leader == Some(1), "self-elected")
        .await
        .unwrap();

    // 10 writes → snapshot + purge.
    let mut snap_index = 0;
    for i in 0..10u32 {
        snap_index = n1.raft.client_write(insert(i, None)).await.unwrap().data.log_index;
    }
    n1.raft.trigger().snapshot().await.unwrap();
    n1.raft
        .wait(Some(Duration::from_secs(10)))
        .metrics(|m| m.snapshot.map_or(0, |s| s.index) >= snap_index, "snapshotted")
        .await
        .unwrap();
    n1.raft.trigger().purge_log(snap_index).await.unwrap();

    // 5 more writes — the live tail, present only in the log.
    for i in 10..15u32 {
        n1.raft.client_write(insert(i, None)).await.unwrap();
    }

    let n2 = spawn_node(2).await;
    n1.raft.add_learner(2, n2.node.clone(), true).await.unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if n2.sm.with_state(|s| s.record_count()).await == 15 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "joiner must receive snapshot (10) + tail (5)"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        n1.sm.state_hash().await,
        n2.sm.state_hash().await,
        "snapshot + tail must reproduce the leader's exact state"
    );
}
