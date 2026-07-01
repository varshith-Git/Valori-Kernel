// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Partition harness — in-memory Raft transport with switchable network partitions.
//!
//! Instead of real gRPC sockets, every RPC is a direct async call to the target
//! `Raft` instance. A `PartitionTable` (shared across all nodes) controls which
//! (source, target) pairs are blocked. Blocking returns an immediate
//! `RPCError::Network`, which openraft treats as a lost message and retries
//! after the election timeout — exactly what a real partition looks like.
//!
//! ## Usage
//!
//! ```ignore
//! let (rafts, sms, partition) = make_cluster(3).await;
//! let (leader_idx, leader_id) = wait_for_leader(&rafts).await;
//!
//! // Isolate the leader from all followers.
//! for id in 1u64..=3 {
//!     if id != leader_id {
//!         partition.block_both(leader_id, id);
//!     }
//! }
//! // …wait for re-election on the remaining 2 nodes…
//! partition.clear();
//! // …wait for convergence…
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use openraft::error::{InstallSnapshotError, NetworkError, RPCError, RaftError, RemoteError};
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::Config;

use crate::types::{NodeId, Raft, TypeConfig, ValoriNode};
use crate::{ClientRequest, MemoryAuditSink, ValoriLogStore, ValoriStateMachine};

// ── PartitionTable ────────────────────────────────────────────────────────────

/// Shared, thread-safe set of blocked (source → target) pairs.
///
/// Blocking is one-directional: `block(a, b)` drops RPCs from `a` to `b`
/// but not from `b` to `a`. Use `block_both` / `unblock_both` for symmetric
/// partitions (the usual split-brain scenario).
#[derive(Default, Clone)]
pub struct PartitionTable {
    blocked: Arc<std::sync::Mutex<HashSet<(NodeId, NodeId)>>>,
}

impl PartitionTable {
    pub fn block(&self, from: NodeId, to: NodeId) {
        self.blocked.lock().unwrap().insert((from, to));
    }
    pub fn unblock(&self, from: NodeId, to: NodeId) {
        self.blocked.lock().unwrap().remove(&(from, to));
    }
    pub fn block_both(&self, a: NodeId, b: NodeId) {
        self.block(a, b);
        self.block(b, a);
    }
    pub fn unblock_both(&self, a: NodeId, b: NodeId) {
        self.unblock(a, b);
        self.unblock(b, a);
    }
    pub fn is_blocked(&self, from: NodeId, to: NodeId) -> bool {
        self.blocked.lock().unwrap().contains(&(from, to))
    }
    pub fn clear(&self) {
        self.blocked.lock().unwrap().clear();
    }
}

// ── RaftRegistry ──────────────────────────────────────────────────────────────

/// Shared directory of all live `Raft` instances in this test cluster.
/// `PartitionNetwork::append_entries` / `vote` / `install_snapshot` look
/// up the target here at call time (lazy — the registry may be empty when
/// `new_client` is called during `Raft::new`).
#[derive(Default, Clone)]
pub struct RaftRegistry {
    nodes: Arc<tokio::sync::Mutex<HashMap<NodeId, Raft>>>,
}

impl RaftRegistry {
    pub async fn register(&self, id: NodeId, raft: Raft) {
        self.nodes.lock().await.insert(id, raft);
    }
    pub async fn get(&self, id: NodeId) -> Option<Raft> {
        self.nodes.lock().await.get(&id).cloned()
    }
}

// ── PartitionNetworkFactory / PartitionNetwork ────────────────────────────────

pub struct PartitionNetworkFactory {
    source: NodeId,
    partition: PartitionTable,
    registry: RaftRegistry,
}

impl PartitionNetworkFactory {
    pub fn new(source: NodeId, partition: PartitionTable, registry: RaftRegistry) -> Self {
        Self { source, partition, registry }
    }
}

impl RaftNetworkFactory<TypeConfig> for PartitionNetworkFactory {
    type Network = PartitionNetwork;

    async fn new_client(&mut self, target: NodeId, _node: &ValoriNode) -> PartitionNetwork {
        PartitionNetwork {
            source: self.source,
            target,
            partition: self.partition.clone(),
            registry: self.registry.clone(),
        }
    }
}

pub struct PartitionNetwork {
    source: NodeId,
    target: NodeId,
    partition: PartitionTable,
    registry: RaftRegistry,
}

#[derive(Debug)]
struct PartitionedError;
impl std::fmt::Display for PartitionedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "network partitioned ({} → {})", 0, 0)
    }
}
impl std::error::Error for PartitionedError {}

impl PartitionNetwork {
    fn check_partition<E: std::error::Error>(
        &self,
    ) -> Result<(), RPCError<NodeId, ValoriNode, E>> {
        if self.partition.is_blocked(self.source, self.target) {
            Err(RPCError::Network(NetworkError::new(&PartitionedError)))
        } else {
            Ok(())
        }
    }

    async fn target_raft<E: std::error::Error>(
        &self,
    ) -> Result<Raft, RPCError<NodeId, ValoriNode, E>> {
        self.registry
            .get(self.target)
            .await
            .ok_or_else(|| RPCError::Network(NetworkError::new(&PartitionedError)))
    }
}

impl RaftNetwork<TypeConfig> for PartitionNetwork {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<NodeId>, RPCError<NodeId, ValoriNode, RaftError<NodeId>>>
    {
        self.check_partition()?;
        self.target_raft()
            .await?
            .append_entries(rpc)
            .await
            .map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        _option: RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, ValoriNode, RaftError<NodeId>>> {
        self.check_partition()?;
        self.target_raft()
            .await?
            .vote(rpc)
            .await
            .map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<NodeId, ValoriNode, RaftError<NodeId, InstallSnapshotError>>,
    > {
        self.check_partition()?;
        self.target_raft()
            .await?
            .install_snapshot(rpc)
            .await
            .map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
    }
}

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Spin up `n` in-process Raft nodes wired through the partition transport.
/// Node IDs are 1..=n. Node 1 calls `initialize` to bootstrap the cluster.
pub async fn make_cluster(
    n: usize,
) -> (Vec<Raft>, Vec<ValoriStateMachine>, PartitionTable) {
    let partition = PartitionTable::default();
    let registry = RaftRegistry::default();

    let config = Arc::new(
        Config {
            heartbeat_interval: 50,
            election_timeout_min: 150,
            election_timeout_max: 300,
            ..Default::default()
        }
        .validate()
        .unwrap(),
    );

    let mut members: BTreeMap<NodeId, ValoriNode> = BTreeMap::new();
    for i in 1..=(n as NodeId) {
        members.insert(
            i,
            ValoriNode {
                api_addr: format!("test-{i}:0"),
                raft_addr: String::new(),
            },
        );
    }

    let mut rafts = Vec::new();
    let mut sms = Vec::new();

    for i in 1..=(n as NodeId) {
        let sm = ValoriStateMachine::new(Box::new(MemoryAuditSink::new()), 0);
        let factory = PartitionNetworkFactory::new(i, partition.clone(), registry.clone());
        let raft = Raft::new(i, config.clone(), factory, ValoriLogStore::new(), sm.clone())
            .await
            .unwrap();
        registry.register(i, raft.clone()).await;
        rafts.push(raft);
        sms.push(sm);
    }

    // Node 1 bootstraps — the call is idempotent (openraft returns NotAllowed
    // on an already-initialized cluster).
    rafts[0].initialize(members).await.unwrap();

    (rafts, sms, partition)
}

/// Wait until any Raft in `rafts` believes itself to be the current leader.
/// Returns `(index_into_rafts, leader_node_id)`.
/// Panics after 5 seconds.
pub async fn wait_for_leader(rafts: &[Raft]) -> (usize, NodeId) {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        for (i, raft) in rafts.iter().enumerate() {
            let m = raft.metrics().borrow().clone();
            if m.current_leader == Some(m.id) {
                return (i, m.id);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            let states: Vec<_> = rafts
                .iter()
                .map(|r| {
                    let m = r.metrics().borrow().clone();
                    format!("id={} leader={:?}", m.id, m.current_leader)
                })
                .collect();
            panic!("timeout: no leader after 5s — states: {states:?}");
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
    }
}

/// Wait until all state machines in `sms` report the same BLAKE3 state hash.
/// Panics after 5 seconds.
pub async fn wait_for_convergence(sms: &[ValoriStateMachine]) {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        let mut hashes = Vec::with_capacity(sms.len());
        for sm in sms {
            hashes.push(sm.state_hash().await);
        }
        if hashes.windows(2).all(|w| w[0] == w[1]) {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timeout: state hashes did not converge after 5s — {hashes:?}");
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
    }
}

/// Commit an `AutoInsertRecord` through the given Raft leader.
/// Returns the assigned record ID.
pub async fn insert_vector(
    raft: &Raft,
    values: Vec<valori_kernel::types::scalar::FxpScalar>,
) -> u32 {
    let resp = raft
        .client_write(ClientRequest {
            event: valori_kernel::event::KernelEvent::AutoInsertRecord {
                vector: valori_kernel::types::vector::FxpVector { data: values },
                metadata: None,
                tag: 0,
            },
            request_id: None,
            schema_version: 0,
            namespace_id: 0,
        })
        .await
        .expect("client_write failed");
    resp.data.allocated_record_id.expect("no allocated_record_id in response")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::types::scalar::FxpScalar;

    fn vec3(a: i32, b: i32, c: i32) -> Vec<FxpScalar> {
        vec![FxpScalar(a), FxpScalar(b), FxpScalar(c)]
    }

    // ── 1. Basic consensus ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_three_node_consensus() {
        let (rafts, sms, _partition) = make_cluster(3).await;
        let (leader_idx, _) = wait_for_leader(&rafts).await;

        let id = insert_vector(&rafts[leader_idx], vec3(100, 200, 300)).await;
        assert_eq!(id, 0, "first insert should get id 0");

        let id2 = insert_vector(&rafts[leader_idx], vec3(400, 500, 600)).await;
        assert_eq!(id2, 1, "second insert should get id 1");

        wait_for_convergence(&sms).await;

        // All 3 replicas must have both records.
        for sm in &sms {
            sm.with_state(|s| {
                assert!(s.get_record(valori_kernel::types::id::RecordId(0)).is_some());
                assert!(s.get_record(valori_kernel::types::id::RecordId(1)).is_some());
            })
            .await;
        }
    }

    // ── 2. Leader isolation triggers re-election ──────────────────────────────

    #[tokio::test]
    async fn test_leader_isolation_triggers_reelection() {
        let (rafts, _sms, partition) = make_cluster(3).await;
        let (_, old_leader_id) = wait_for_leader(&rafts).await;

        // Sever the old leader from both followers.
        for id in 1u64..=3 {
            if id != old_leader_id {
                partition.block_both(old_leader_id, id);
            }
        }

        // Wait for the remaining two nodes to elect a NEW leader.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            let found = rafts.iter().any(|r| {
                let m = r.metrics().borrow().clone();
                m.id != old_leader_id && m.current_leader == Some(m.id)
            });
            if found {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("timeout: no new leader elected after isolating old leader");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
        }

        // Heal the partition.
        partition.clear();
    }

    // ── 3. Partition heal — state convergence ─────────────────────────────────

    #[tokio::test]
    async fn test_partition_heal_state_convergence() {
        let (rafts, sms, partition) = make_cluster(3).await;
        let (leader_idx, old_leader_id) = wait_for_leader(&rafts).await;

        // Write before partition.
        insert_vector(&rafts[leader_idx], vec3(1, 2, 3)).await;
        wait_for_convergence(&sms).await;

        // Isolate old leader.
        for id in 1u64..=3 {
            if id != old_leader_id {
                partition.block_both(old_leader_id, id);
            }
        }

        // Find the new leader among the non-isolated nodes.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        let new_leader_raft = 'find: loop {
            for raft in &rafts {
                let m = raft.metrics().borrow().clone();
                if m.id != old_leader_id && m.current_leader == Some(m.id) {
                    break 'find raft.clone();
                }
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("timeout: no new leader after partition");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
        };

        // Write through the new leader while old leader is partitioned.
        insert_vector(&new_leader_raft, vec3(7, 8, 9)).await;

        // Heal and wait for all 3 replicas to converge.
        partition.clear();
        wait_for_convergence(&sms).await;

        // Both records must be present everywhere.
        for sm in &sms {
            sm.with_state(|s| {
                assert!(s.get_record(valori_kernel::types::id::RecordId(0)).is_some(),
                    "record 0 missing after heal");
                assert!(s.get_record(valori_kernel::types::id::RecordId(1)).is_some(),
                    "record 1 missing after heal");
            })
            .await;
        }
    }

    // ── 4. Minority partition cannot commit ───────────────────────────────────

    #[tokio::test]
    async fn test_minority_partition_cannot_commit() {
        let (rafts, sms, partition) = make_cluster(3).await;
        let (leader_idx, leader_id) = wait_for_leader(&rafts).await;

        // Find one follower to isolate (not the leader).
        let isolated_id = (1u64..=3).find(|&id| id != leader_id).unwrap();
        let isolated_idx = (isolated_id - 1) as usize;

        // Isolate the follower from everyone — it forms a minority of 1.
        for id in 1u64..=3 {
            if id != isolated_id {
                partition.block_both(isolated_id, id);
            }
        }

        // The remaining 2-node majority (leader + 1 follower) can still commit.
        let id = insert_vector(&rafts[leader_idx], vec3(42, 43, 44)).await;
        assert_eq!(id, 0);

        // The isolated node's SM should NOT have the record — it was cut off
        // before the insert was submitted, so it never received the AppendEntries.
        // Give the cluster a moment to ensure no stray replication sneaks through.
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
        let isolated_has_record = sms[isolated_idx]
            .with_state(|s| s.get_record(valori_kernel::types::id::RecordId(0)).is_some())
            .await;
        assert!(!isolated_has_record, "isolated minority should not have committed the entry");

        partition.clear();
    }
}
