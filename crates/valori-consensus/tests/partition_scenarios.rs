// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 2.10d — partition scenario tests.
//!
//! Covers the two gaps left by Phase 2.8 (process-kill tests):
//!
//! 1. **Asymmetric partition** — one direction of a link is blocked while the
//!    other stays open. Raft must still make progress through the 2/3 quorum
//!    that can communicate fully, and the lagging node must catch up after
//!    the link is restored.
//!
//! 2. **BLAKE3 chain integrity across partition-and-heal** — after a split,
//!    writes committed on the surviving majority, and the stale node catching
//!    up via log replay: every replica's BLAKE3 state hash must be identical.
//!    This is the compliance guarantee: no partition scenario produces a
//!    divergent audit chain that "forgets" committed writes.
//!
//! Both tests use the in-process `partition_harness` transport. No real sockets.

use valori_consensus::partition_harness::{
    insert_vector, make_cluster, wait_for_convergence, wait_for_leader,
};
use valori_kernel::types::scalar::FxpScalar;

fn v(a: i32, b: i32, c: i32) -> Vec<FxpScalar> {
    vec![FxpScalar(a), FxpScalar(b), FxpScalar(c)]
}

/// Asymmetric partition: block AppendEntries from leader to one follower
/// (leader → follower1) while the reverse link stays open. The 2/3 quorum
/// (leader + follower2) must still commit writes. After the link is restored,
/// follower1 must catch up to the same BLAKE3 hash as the others.
#[tokio::test]
async fn asymmetric_partition_lagging_node_catches_up() {
    let (rafts, sms, partition) = make_cluster(3).await;
    let (leader_idx, leader_id) = wait_for_leader(&rafts).await;

    // Write one record before the asymmetric cut so all 3 nodes start equal.
    insert_vector(&rafts[leader_idx], v(1, 2, 3)).await;
    wait_for_convergence(&sms).await;

    // Pick a follower to cut off in one direction.
    let follower_id = (1u64..=3).find(|&id| id != leader_id).unwrap();

    // Block only leader → follower1. The follower can still send Vote RPCs to
    // the leader and the other follower, but it won't receive AppendEntries from
    // the leader. The leader + follower2 form a 2/3 quorum and can still commit.
    partition.block(leader_id, follower_id);

    // Five writes through the leader — should all succeed because quorum ≥ 2.
    for i in 0..5i32 {
        insert_vector(&rafts[leader_idx], v(i * 10, i * 10 + 1, i * 10 + 2)).await;
    }

    // Restore the link.
    partition.unblock(leader_id, follower_id);

    // All three state machines must converge to the same hash: 6 committed events.
    wait_for_convergence(&sms).await;

    // Verify every replica has all 6 records.
    for sm in &sms {
        let count = sm.with_state(|s| s.record_count()).await;
        assert_eq!(count, 6, "every replica must have all 6 records after heal");
    }

    // The lagging node must not have a divergent BLAKE3 chain.
    let h0 = sms[0].state_hash().await;
    let h1 = sms[1].state_hash().await;
    let h2 = sms[2].state_hash().await;
    assert_eq!(h0, h1, "node 1 and node 2 must share a BLAKE3 state hash");
    assert_eq!(h1, h2, "node 2 and node 3 must share a BLAKE3 state hash");
    assert_ne!(h0, [0u8; 32], "state hash must be non-zero");
}

/// Partition-and-heal BLAKE3 integrity: the fundamental compliance guarantee.
///
/// Scenario:
///   1. Pre-partition: 3 writes, all nodes converge.
///   2. Symmetric partition: old leader isolated, 2-node minority elects new leader.
///   3. During partition: 3 more writes committed on the new majority.
///   4. Partition isolated node: 0 writes can commit (minority of 1).
///   5. Heal: isolated node catches up.
///   6. Assert: all 3 nodes have identical BLAKE3 state hashes and all 6 records.
///
/// This test is the proof that the audit trail remains consistent across any
/// partition scenario: an isolated node cannot diverge the chain, and after
/// healing the chain is exactly the same on every replica.
#[tokio::test]
async fn blake3_chain_consistent_across_partition_and_heal() {
    let (rafts, sms, partition) = make_cluster(3).await;
    let (leader_idx, old_leader_id) = wait_for_leader(&rafts).await;

    // Phase 1: pre-partition writes — all 3 nodes must have these.
    for i in 0..3i32 {
        insert_vector(&rafts[leader_idx], v(i, i + 1, i + 2)).await;
    }
    wait_for_convergence(&sms).await;

    let pre_partition_hash = sms[0].state_hash().await;
    assert_ne!(pre_partition_hash, [0u8; 32]);

    // Phase 2: symmetric partition — isolate the old leader.
    for id in 1u64..=3 {
        if id != old_leader_id {
            partition.block_both(old_leader_id, id);
        }
    }

    // Phase 3: find the new leader elected by the surviving 2-node majority.
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let new_leader_raft = 'elect: loop {
        for raft in &rafts {
            let m = raft.metrics().borrow().clone();
            if m.id != old_leader_id && m.current_leader == Some(m.id) {
                break 'elect raft.clone();
            }
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timeout: surviving majority failed to elect a new leader within 5 s");
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
    };

    // Phase 4: 3 more writes through the new leader during the partition.
    for i in 3..6i32 {
        insert_vector(&new_leader_raft, v(i * 100, i * 100 + 1, i * 100 + 2)).await;
    }

    // Phase 5: confirm the isolated node did NOT receive these writes.
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    let isolated_idx = (old_leader_id - 1) as usize;
    let isolated_count = sms[isolated_idx].with_state(|s| s.record_count()).await;
    assert_eq!(
        isolated_count, 3,
        "isolated node must not have received the 3 post-partition writes"
    );
    let isolated_hash_during = sms[isolated_idx].state_hash().await;
    assert_eq!(
        isolated_hash_during, pre_partition_hash,
        "isolated node's BLAKE3 hash must be frozen at the pre-partition state"
    );

    // Phase 6: heal — clear all partition rules.
    partition.clear();

    // All 3 nodes must converge to the same hash with all 6 records.
    wait_for_convergence(&sms).await;

    for sm in &sms {
        let count = sm.with_state(|s| s.record_count()).await;
        assert_eq!(count, 6, "all 6 records must be present on every replica after heal");
    }

    let final_hash = sms[0].state_hash().await;
    assert_eq!(sms[1].state_hash().await, final_hash, "node 2 BLAKE3 hash must match node 1");
    assert_eq!(sms[2].state_hash().await, final_hash, "node 3 BLAKE3 hash must match node 1");
    assert_ne!(final_hash, pre_partition_hash, "hash must have advanced from the 3 new writes");
    assert_ne!(final_hash, [0u8; 32]);
}

/// Stale node cannot fork the BLAKE3 chain.
///
/// An isolated node that receives no new committed entries keeps its hash frozen.
/// After healing, it must adopt exactly the chain produced by the surviving majority —
/// it cannot introduce a fork or a divergent branch.
#[tokio::test]
async fn isolated_node_hash_frozen_then_converges() {
    let (rafts, sms, partition) = make_cluster(3).await;
    let (leader_idx, leader_id) = wait_for_leader(&rafts).await;

    // Baseline: 2 writes, all converge.
    insert_vector(&rafts[leader_idx], v(10, 20, 30)).await;
    insert_vector(&rafts[leader_idx], v(40, 50, 60)).await;
    wait_for_convergence(&sms).await;
    let frozen_hash = sms[0].state_hash().await;

    // Isolate one follower symmetrically.
    let follower_id = (1u64..=3).find(|&id| id != leader_id).unwrap();
    let follower_idx = (follower_id - 1) as usize;
    for id in 1u64..=3 {
        if id != follower_id {
            partition.block_both(follower_id, id);
        }
    }

    // 3 more writes committed through the 2/3 quorum.
    for i in 0..3i32 {
        insert_vector(&rafts[leader_idx], v(i * 7, i * 7 + 1, i * 7 + 2)).await;
    }

    // Brief wait — enough for replication to the non-isolated nodes but not the isolated one.
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // The isolated node's hash must be frozen.
    assert_eq!(
        sms[follower_idx].state_hash().await,
        frozen_hash,
        "isolated follower's hash must not change while partitioned"
    );

    // Heal.
    partition.clear();
    wait_for_convergence(&sms).await;

    // All 3 must agree and have all 5 records.
    for (i, sm) in sms.iter().enumerate() {
        assert_eq!(
            sm.with_state(|s| s.record_count()).await,
            5,
            "node {} must have all 5 records", i + 1
        );
    }
    let h0 = sms[0].state_hash().await;
    assert_eq!(sms[1].state_hash().await, h0);
    assert_eq!(sms[2].state_hash().await, h0);
    assert_ne!(h0, frozen_hash, "hash must have advanced after the 3 new writes");
}
