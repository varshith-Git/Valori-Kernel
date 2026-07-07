// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Multi-shard split-brain tests.
//!
//! These tests exercise the scenario where a network partition affects only ONE
//! shard's Raft group while the other shard remains fully connected — the exact
//! failure mode a per-port firewall rule or connection-level failure produces.
//!
//! Acceptance gates:
//! - Partitioning shard 0 does not disrupt shard 1's writes or convergence
//! - After healing, the partitioned shard converges independently
//! - BLAKE3 chains remain correct per shard throughout the split-brain

use valori_consensus::partition_harness::{
    insert_shard_vector, make_sharded_cluster, wait_for_shard_convergence, wait_for_shard_leader,
};
use valori_consensus::types::ShardId;
use valori_kernel::types::scalar::FxpScalar;

const SHARD0: ShardId = ShardId(0);
const SHARD1: ShardId = ShardId(1);

fn v(a: i32, b: i32, c: i32) -> Vec<FxpScalar> {
    vec![FxpScalar(a), FxpScalar(b), FxpScalar(c)]
}

/// Partition shard 0 (isolate its leader) while shard 1 stays fully connected.
/// Shard 1 must continue accepting writes. After healing, shard 0 must converge
/// independently. BLAKE3 hashes must be correct for both shards throughout.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn shard0_partitioned_shard1_unaffected() {
    let (shards, partition) = make_sharded_cluster(3, 2).await;

    // Wait for both shards to elect leaders.
    let (s0_leader_idx, s0_leader_id) = wait_for_shard_leader(&shards[0]).await;
    let (s1_leader_idx, _s1_leader_id) = wait_for_shard_leader(&shards[1]).await;

    // Baseline: one write per shard, all converge.
    insert_shard_vector(&shards[0][s0_leader_idx].raft, v(1, 2, 3)).await;
    insert_shard_vector(&shards[1][s1_leader_idx].raft, v(10, 20, 30)).await;
    wait_for_shard_convergence(&shards[0]).await;
    wait_for_shard_convergence(&shards[1]).await;

    let s0_pre_hash = shards[0][0].sm.state_hash().await;
    let s1_pre_hash = shards[1][0].sm.state_hash().await;

    // Partition shard 0 ONLY: isolate its leader from all followers.
    for id in 1u64..=3 {
        if id != s0_leader_id {
            partition.block_both(SHARD0, s0_leader_id, id);
        }
    }

    // Shard 1 is unpartitioned — writes must succeed.
    for i in 0..5i32 {
        insert_shard_vector(&shards[1][s1_leader_idx].raft, v(i * 100, i * 100 + 1, i * 100 + 2)).await;
    }
    wait_for_shard_convergence(&shards[1]).await;

    // Shard 1 advanced; shard 0's isolated node is frozen.
    let s1_post_hash = shards[1][0].sm.state_hash().await;
    assert_ne!(s1_post_hash, s1_pre_hash, "shard 1 must have advanced");

    for pair in &shards[1] {
        let count = pair.sm.with_state(|s| s.record_count()).await;
        assert_eq!(count, 6, "shard 1: all 6 records (1 pre + 5 during partition)");
    }

    // Shard 0's isolated leader should still be frozen at pre-partition state.
    let isolated_idx = (s0_leader_id - 1) as usize;
    let s0_isolated_hash = shards[0][isolated_idx].sm.state_hash().await;
    assert_eq!(s0_isolated_hash, s0_pre_hash, "shard 0 isolated node must be frozen");

    // Shard 0's surviving majority should have elected a new leader.
    // Write through the new shard 0 majority.
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    let new_s0_leader = loop {
        for pair in &shards[0] {
            let m = pair.raft.metrics().borrow().clone();
            if m.id != s0_leader_id && m.current_leader == Some(m.id) {
                break;
            }
        }
        // Find any node in shard 0 that sees a leader != old leader.
        let mut found = None;
        for (i, pair) in shards[0].iter().enumerate() {
            let m = pair.raft.metrics().borrow().clone();
            if m.id != s0_leader_id && m.current_leader.is_some() && m.current_leader != Some(s0_leader_id) {
                found = Some(i);
                break;
            }
        }
        if let Some(idx) = found {
            let leader_id = shards[0][idx].raft.metrics().borrow().current_leader.unwrap();
            break &shards[0][(leader_id - 1) as usize];
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timeout: shard 0 surviving majority failed to elect new leader");
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
    };

    insert_shard_vector(&new_s0_leader.raft, v(77, 88, 99)).await;

    // Heal shard 0.
    partition.clear();

    // Both shards must converge independently.
    wait_for_shard_convergence(&shards[0]).await;
    wait_for_shard_convergence(&shards[1]).await;

    // Shard 0: 2 records (1 pre + 1 during partition via new leader).
    for pair in &shards[0] {
        let count = pair.sm.with_state(|s| s.record_count()).await;
        assert_eq!(count, 2, "shard 0: 2 records after heal");
    }

    // BLAKE3 per-shard consistency.
    let mut h0 = Vec::new();
    for p in &shards[0] { h0.push(p.sm.state_hash().await); }
    let mut h1 = Vec::new();
    for p in &shards[1] { h1.push(p.sm.state_hash().await); }
    assert!(h0.windows(2).all(|w| w[0] == w[1]), "shard 0 replicas diverged after heal");
    assert!(h1.windows(2).all(|w| w[0] == w[1]), "shard 1 replicas diverged after heal");
    assert_ne!(h0[0], h1[0], "shards must have different hashes (different event sets)");
}

/// Both shards partitioned simultaneously but differently — shard 0 loses
/// node 1, shard 1 loses node 3. Both must heal and converge independently.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn cross_shard_asymmetric_partition() {
    let (shards, partition) = make_sharded_cluster(3, 2).await;

    let (_s0_li, _s0_lid) = wait_for_shard_leader(&shards[0]).await;
    let (_s1_li, _s1_lid) = wait_for_shard_leader(&shards[1]).await;

    // Pre-partition: one write per shard.
    insert_shard_vector(&shards[0][_s0_li].raft, v(1, 1, 1)).await;
    insert_shard_vector(&shards[1][_s1_li].raft, v(2, 2, 2)).await;
    wait_for_shard_convergence(&shards[0]).await;
    wait_for_shard_convergence(&shards[1]).await;

    // Shard 0: isolate node 1 (may or may not be leader).
    for id in 2u64..=3 {
        partition.block_both(SHARD0, 1, id);
    }
    // Shard 1: isolate node 3.
    for id in 1u64..=2 {
        partition.block_both(SHARD1, 3, id);
    }

    // Wait for new leaders if needed, then write to each shard's majority.
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Find shard 0 leader among nodes 2,3 (node 1 is isolated).
    let s0_new_leader = find_leader_excluding(&shards[0], 1).await;
    insert_shard_vector(&s0_new_leader, v(3, 3, 3)).await;

    // Find shard 1 leader among nodes 1,2 (node 3 is isolated).
    let s1_new_leader = find_leader_excluding(&shards[1], 3).await;
    insert_shard_vector(&s1_new_leader, v(4, 4, 4)).await;

    // Heal everything.
    partition.clear();

    wait_for_shard_convergence(&shards[0]).await;
    wait_for_shard_convergence(&shards[1]).await;

    for pair in &shards[0] {
        assert_eq!(pair.sm.with_state(|s| s.record_count()).await, 2);
    }
    for pair in &shards[1] {
        assert_eq!(pair.sm.with_state(|s| s.record_count()).await, 2);
    }
}

/// BLAKE3 chain integrity: partitioning one shard must not corrupt the other
/// shard's audit chain, even when both shards share the same physical node.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn blake3_chain_per_shard_independent() {
    let (shards, partition) = make_sharded_cluster(3, 2).await;

    let (s0_li, s0_lid) = wait_for_shard_leader(&shards[0]).await;
    let (s1_li, _) = wait_for_shard_leader(&shards[1]).await;

    // 3 writes per shard, converge.
    for i in 0..3i32 {
        insert_shard_vector(&shards[0][s0_li].raft, v(i, i+1, i+2)).await;
        insert_shard_vector(&shards[1][s1_li].raft, v(i*10, i*10+1, i*10+2)).await;
    }
    wait_for_shard_convergence(&shards[0]).await;
    wait_for_shard_convergence(&shards[1]).await;

    let s0_hash_pre = shards[0][0].sm.state_hash().await;
    let s1_hash_pre = shards[1][0].sm.state_hash().await;
    assert_ne!(s0_hash_pre, [0u8; 32]);
    assert_ne!(s1_hash_pre, [0u8; 32]);

    // Partition shard 0: isolate its leader.
    for id in 1u64..=3 {
        if id != s0_lid {
            partition.block_both(SHARD0, s0_lid, id);
        }
    }

    // Write to shard 1 (unpartitioned) — its chain advances.
    for i in 0..3i32 {
        insert_shard_vector(&shards[1][s1_li].raft, v(i*100, i*100+1, i*100+2)).await;
    }
    wait_for_shard_convergence(&shards[1]).await;

    let s1_hash_post = shards[1][0].sm.state_hash().await;
    assert_ne!(s1_hash_post, s1_hash_pre, "shard 1 chain must advance");

    // Shard 0's isolated node is frozen.
    let isolated = (s0_lid - 1) as usize;
    assert_eq!(shards[0][isolated].sm.state_hash().await, s0_hash_pre);

    // Heal, converge shard 0.
    partition.clear();
    // Shard 0 majority may have written nothing new, so convergence = all match.
    wait_for_shard_convergence(&shards[0]).await;

    // Final check: both shards' chains are internally consistent.
    let mut h0 = Vec::new();
    for p in &shards[0] { h0.push(p.sm.state_hash().await); }
    let mut h1 = Vec::new();
    for p in &shards[1] { h1.push(p.sm.state_hash().await); }
    assert!(h0.windows(2).all(|w| w[0] == w[1]), "shard 0 chain diverged");
    assert!(h1.windows(2).all(|w| w[0] == w[1]), "shard 1 chain diverged");
}

/// Helper: find a leader in the shard, excluding a specific node ID.
async fn find_leader_excluding(
    shard: &[valori_consensus::partition_harness::ShardRaftPair],
    exclude: u64,
) -> &openraft::Raft<valori_consensus::types::TypeConfig> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        for pair in shard {
            let m = pair.raft.metrics().borrow().clone();
            if m.id != exclude && m.current_leader == Some(m.id) {
                return &pair.raft;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timeout: no leader found excluding node {exclude}");
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
    }
}
