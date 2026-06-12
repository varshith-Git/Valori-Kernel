// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Raft state machine — Phase 2.3. The core correctness piece.
//!
//! `ValoriStateMachine` adapts `KernelState` to openraft's
//! [`RaftStateMachine`]. The kernel's determinism contract maps directly
//! onto Raft's: identical committed entries produce identical state on
//! every node, byte for byte, hash for hash.
//!
//! ## Apply pipeline (per committed entry)
//!
//! 1. **Dedup** — if the entry's `request_id` was already applied, skip the
//!    kernel apply and answer `deduplicated: true`. The dedup table is part
//!    of the replicated state (it travels in snapshots), so every node makes
//!    the same decision.
//! 2. **Kernel apply** — `KernelState::apply_event`. A rejected event (bad
//!    sequential id, dimension mismatch) is *also* deterministic: every node
//!    rejects identically, the response carries the rejection, state is
//!    untouched.
//! 3. **Audit record** — only after a successful apply, the event goes to
//!    the [`AuditSink`]. This is THE audit-log write point: after quorum,
//!    at apply, exactly once. (valori-node plugs its chained `EventLogWriter`
//!    in here in Phase 2.5; tests use [`MemoryAuditSink`].)
//!
//! ## Snapshots
//!
//! The snapshot payload is the Phase 1.3 V5 kernel snapshot (with its
//! arithmetic-format byte and the hash-domain guarantees) plus the dedup
//! table, framed with bincode. `install_snapshot` therefore inherits the V5
//! refusal semantics: a snapshot from a foreign arithmetic format fails to
//! decode and the node keeps its old state.

use std::collections::{HashSet, VecDeque};
use std::io::Cursor;
use std::sync::Arc;

use openraft::storage::{RaftStateMachine, Snapshot};
use openraft::{
    EntryPayload, LogId, RaftSnapshotBuilder, SnapshotMeta, StorageError, StorageIOError,
    StoredMembership,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::encode_state;
use valori_kernel::state::kernel::KernelState;

use crate::types::{ClientResponse, Entry, NodeId, TypeConfig, ValoriNode};

/// Where committed events are recorded for auditing.
///
/// THE one audit-log write point in cluster mode: called once per event,
/// at apply time, strictly after quorum commit, strictly after a successful
/// kernel apply. valori-node implements this over its BLAKE3-chained
/// `EventLogWriter` (Phase 2.5); tests use [`MemoryAuditSink`].
pub trait AuditSink: Send + Sync + 'static {
    fn record(
        &mut self,
        event: &KernelEvent,
        request_id: Option<[u8; 16]>,
    ) -> Result<(), std::io::Error>;
}

/// Discards events. For nodes that delegate auditing elsewhere and for
/// the openraft compliance suite.
pub struct NullAuditSink;

impl AuditSink for NullAuditSink {
    fn record(&mut self, _: &KernelEvent, _: Option<[u8; 16]>) -> Result<(), std::io::Error> {
        Ok(())
    }
}

/// Captures events in memory so tests can assert exactly what reached the
/// audit point, in what order.
#[derive(Default, Clone)]
pub struct MemoryAuditSink {
    records: Arc<std::sync::Mutex<Vec<(String, Option<[u8; 16]>)>>>,
}

impl MemoryAuditSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn recorded(&self) -> Vec<(String, Option<[u8; 16]>)> {
        self.records.lock().unwrap().clone()
    }
}

impl AuditSink for MemoryAuditSink {
    fn record(
        &mut self,
        event: &KernelEvent,
        request_id: Option<[u8; 16]>,
    ) -> Result<(), std::io::Error> {
        self.records
            .lock()
            .unwrap()
            .push((event.event_type().to_string(), request_id));
        Ok(())
    }
}

/// Dedup table capacity. FIFO eviction beyond this point — a retry that
/// arrives 65k writes after its original is no longer recognised, which is
/// a deliberate trade against unbounded memory. Phase 2.10 revisits.
const MAX_DEDUP_ENTRIES: usize = 65_536;

/// What travels inside a Raft snapshot, beyond openraft's own meta.
#[derive(Serialize, Deserialize)]
struct SnapshotPayload {
    /// Phase 1.3 V5 kernel snapshot bytes (format byte included).
    kernel: Vec<u8>,
    /// The dedup table — replicated so a restored follower makes the same
    /// dedup decisions as the leader.
    dedup: Vec<[u8; 16]>,
    /// BLAKE3 state hash the kernel bytes must decode to. The V5 format has
    /// no internal checksum (finding from Phase 2.3 testing: a flipped byte
    /// mid-payload decodes "successfully" into corrupt state), so the
    /// payload is self-verifying: install recomputes and refuses a mismatch.
    state_hash: [u8; 32],
}

struct StateMachineInner {
    state: KernelState,
    last_applied: Option<LogId<NodeId>>,
    membership: StoredMembership<NodeId, ValoriNode>,
    dedup_set: HashSet<[u8; 16]>,
    dedup_order: VecDeque<[u8; 16]>,
    current_snapshot: Option<(SnapshotMeta<NodeId, ValoriNode>, Vec<u8>)>,
    audit: Box<dyn AuditSink>,
    snapshot_seq: u64,
}

impl StateMachineInner {
    fn remember_request(&mut self, id: [u8; 16]) {
        if self.dedup_set.insert(id) {
            self.dedup_order.push_back(id);
            while self.dedup_order.len() > MAX_DEDUP_ENTRIES {
                if let Some(old) = self.dedup_order.pop_front() {
                    self.dedup_set.remove(&old);
                }
            }
        }
    }

    fn encode_kernel(&self) -> Result<Vec<u8>, StorageError<NodeId>> {
        // Same sizing formula valori-node uses for its own snapshots.
        let total_slots = self.state.total_record_slots();
        let dim = self.state.dim.unwrap_or(0);
        let size = 64
            + total_slots * (18 + dim * 4)
            + self.state.node_count() * 25
            + self.state.edge_count() * 29
            + 2 * 1024 * 1024;
        let mut buf = vec![0u8; size];
        let len = encode_state(&self.state, &mut buf)
            .map_err(|e| io_err(format!("kernel snapshot encode failed: {e:?}")))?;
        buf.truncate(len);
        Ok(buf)
    }
}

fn io_err(msg: String) -> StorageError<NodeId> {
    StorageError::IO {
        source: StorageIOError::write(&std::io::Error::other(msg)),
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The Raft state machine over `KernelState`. Cheap to clone — all clones
/// share state, mirroring `ValoriLogStore`.
#[derive(Clone)]
pub struct ValoriStateMachine {
    inner: Arc<Mutex<StateMachineInner>>,
}

impl Default for ValoriStateMachine {
    fn default() -> Self {
        Self::new(Box::new(NullAuditSink))
    }
}

impl ValoriStateMachine {
    pub fn new(audit: Box<dyn AuditSink>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StateMachineInner {
                state: KernelState::new(),
                last_applied: None,
                membership: StoredMembership::default(),
                dedup_set: HashSet::new(),
                dedup_order: VecDeque::new(),
                current_snapshot: None,
                audit,
                snapshot_seq: 0,
            })),
        }
    }

    /// Current BLAKE3 state hash — the cross-node equality invariant.
    pub async fn state_hash(&self) -> [u8; 32] {
        hash_state_blake3(&self.inner.lock().await.state)
    }

    /// Run a read-only closure against the kernel state (Phase 2.5 uses
    /// this for serving reads without copying the state out).
    pub async fn with_state<T>(&self, f: impl FnOnce(&KernelState) -> T) -> T {
        f(&self.inner.lock().await.state)
    }
}

impl RaftStateMachine<TypeConfig> for ValoriStateMachine {
    type SnapshotBuilder = Self;

    async fn applied_state(
        &mut self,
    ) -> Result<(Option<LogId<NodeId>>, StoredMembership<NodeId, ValoriNode>), StorageError<NodeId>>
    {
        let inner = self.inner.lock().await;
        Ok((inner.last_applied, inner.membership.clone()))
    }

    async fn apply<I>(&mut self, entries: I) -> Result<Vec<ClientResponse>, StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry> + Send,
        I::IntoIter: Send,
    {
        let mut inner = self.inner.lock().await;
        let mut replies = Vec::new();

        for entry in entries {
            inner.last_applied = Some(entry.log_id);
            let log_index = entry.log_id.index;

            match entry.payload {
                EntryPayload::Blank => {
                    replies.push(ClientResponse {
                        log_index,
                        state_hash: hash_state_blake3(&inner.state),
                        deduplicated: false,
                    });
                }
                EntryPayload::Membership(m) => {
                    inner.membership = StoredMembership::new(Some(entry.log_id), m);
                    replies.push(ClientResponse {
                        log_index,
                        state_hash: hash_state_blake3(&inner.state),
                        deduplicated: false,
                    });
                }
                EntryPayload::Normal(req) => {
                    // 1. Dedup — replicated decision, identical on all nodes.
                    if let Some(id) = req.request_id {
                        if inner.dedup_set.contains(&id) {
                            replies.push(ClientResponse {
                                log_index,
                                state_hash: hash_state_blake3(&inner.state),
                                deduplicated: true,
                            });
                            continue;
                        }
                    }

                    // 2. Kernel apply. Rejections are deterministic too —
                    //    every node rejects identically; state is untouched.
                    //    The entry is still consumed (last_applied advanced).
                    let applied = inner.state.apply_event(&req.event).is_ok();

                    // 3. Audit record + dedup memory — successful applies only.
                    if applied {
                        if let Some(id) = req.request_id {
                            inner.remember_request(id);
                        }
                        inner
                            .audit
                            .record(&req.event, req.request_id)
                            .map_err(|e| io_err(format!("audit sink write failed: {e}")))?;
                    }

                    replies.push(ClientResponse {
                        log_index,
                        state_hash: hash_state_blake3(&inner.state),
                        deduplicated: false,
                    });
                }
            }
        }
        Ok(replies)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<NodeId>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, ValoriNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<NodeId>> {
        let bytes = snapshot.into_inner();
        let (payload, _): (SnapshotPayload, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
                .map_err(|e| io_err(format!("snapshot payload decode failed: {e}")))?;

        // V5 semantics apply: a foreign-format snapshot fails here and the
        // node keeps its old state.
        let state = decode_state(&payload.kernel)
            .map_err(|e| io_err(format!("kernel snapshot decode refused: {e:?}")))?;

        // Self-verification: the decoded state must hash to what the builder
        // recorded. Catches silent corruption the V5 decode cannot see.
        let actual = hash_state_blake3(&state);
        if actual != payload.state_hash {
            return Err(io_err(format!(
                "snapshot state-hash mismatch: payload claims {} but decoded state hashes to {} — refusing install",
                hex(&payload.state_hash),
                hex(&actual),
            )));
        }

        let mut inner = self.inner.lock().await;
        inner.state = state;
        inner.last_applied = meta.last_log_id;
        inner.membership = meta.last_membership.clone();
        inner.dedup_set = payload.dedup.iter().copied().collect();
        inner.dedup_order = payload.dedup.into();
        inner.current_snapshot = Some((meta.clone(), bytes));
        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<TypeConfig>>, StorageError<NodeId>> {
        let inner = self.inner.lock().await;
        Ok(inner.current_snapshot.as_ref().map(|(meta, bytes)| Snapshot {
            meta: meta.clone(),
            snapshot: Box::new(Cursor::new(bytes.clone())),
        }))
    }
}

impl RaftSnapshotBuilder<TypeConfig> for ValoriStateMachine {
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfig>, StorageError<NodeId>> {
        let mut inner = self.inner.lock().await;

        let payload = SnapshotPayload {
            kernel: inner.encode_kernel()?,
            dedup: inner.dedup_order.iter().copied().collect(),
            state_hash: hash_state_blake3(&inner.state),
        };
        let bytes = bincode::serde::encode_to_vec(&payload, bincode::config::standard())
            .map_err(|e| io_err(format!("snapshot payload encode failed: {e}")))?;

        inner.snapshot_seq += 1;
        let meta = SnapshotMeta {
            last_log_id: inner.last_applied,
            last_membership: inner.membership.clone(),
            snapshot_id: format!(
                "{}-{}",
                inner.last_applied.map_or(0, |l| l.index),
                inner.snapshot_seq
            ),
        };
        inner.current_snapshot = Some((meta.clone(), bytes.clone()));

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(bytes)),
        })
    }
}
