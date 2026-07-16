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
//! The snapshot payload is the V6 kernel snapshot (with its
//! arithmetic-format byte and the hash-domain guarantees) plus the dedup
//! table, framed with bincode. `install_snapshot` therefore inherits the
//! kernel's refusal semantics: a snapshot from a foreign arithmetic format
//! fails to decode and the node keeps its old state.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Cursor;
use std::sync::Arc;

use openraft::storage::{RaftStateMachine, Snapshot};
use openraft::{
    EntryPayload, LogId, RaftSnapshotBuilder, SnapshotMeta, StorageError, StorageIOError,
    StoredMembership,
};
use redb::Database;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::log_store_redb::SM_META;

use valori_metadata::CollectionRegistry;
use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::types::id::RecordId as KRecordId;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::encode_state;
use valori_kernel::state::kernel::KernelState;

use crate::types::{ClientResponse, Entry, NodeId, TypeConfig, ValoriNode, CURRENT_SCHEMA_VERSION};

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
/// arrives after this many writes is no longer recognised and may double-apply.
/// At 45 events/min the 1M window gives ~15 days; at 10k/s it gives ~100 sec.
/// If retries can be older than this window, increase or add a persistent table.
const MAX_DEDUP_ENTRIES: usize = 1_048_576;

// Keys within the SM_META table (prefixed "sm_" to avoid collisions with
// log-store keys in the same redb file).
const KEY_SM_LAST_APPLIED: &str = "sm_last_applied";
const KEY_SM_MEMBERSHIP: &str = "sm_membership";
const KEY_SM_SNAPSHOT_META: &str = "sm_snapshot_meta";
const KEY_SM_SNAPSHOT_DATA: &str = "sm_snapshot_data";

/// What travels inside a Raft snapshot, beyond openraft's own meta.
/// V2: adds `created_at` and `text_corpus` so decay ranking and BM25
/// reranking work correctly on a restored follower (M-1 fix).
#[derive(Serialize, Deserialize)]
struct SnapshotPayload {
    /// V6 kernel snapshot bytes (format byte included).
    kernel: Vec<u8>,
    /// The dedup table — replicated so a restored follower makes the same
    /// dedup decisions as the leader.
    dedup: Vec<[u8; 16]>,
    /// BLAKE3 state hash the kernel bytes must decode to. The V6 format has
    /// no internal checksum (finding from Phase 2.3 testing: a flipped byte
    /// mid-payload decodes "successfully" into corrupt state), so the
    /// payload is self-verifying: install recomputes and refuses a mismatch.
    state_hash: [u8; 32],
    /// Unix-second creation timestamps keyed by record id. Needed for
    /// decay-based search reranking; omitted from hash (derived, not kernel state).
    created_at: Vec<(u32, u64)>,
    /// BM25 text corpus: record_id → body text indexed at insert time.
    text_corpus: Vec<(u64, String)>,
    /// Phase S2: namespace name -> id registry (map entries, next_id), so a
    /// restored follower agrees with the leader on every collection name's
    /// id. Not part of `state_hash` — names are not kernel state.
    namespace_registry: (Vec<(String, u16)>, u16),
}

struct StateMachineInner {
    state: KernelState,
    last_applied: Option<LogId<NodeId>>,
    membership: StoredMembership<NodeId, ValoriNode>,
    dedup_set: HashSet<[u8; 16]>,
    dedup_order: VecDeque<[u8; 16]>,
    current_snapshot: Option<(SnapshotMeta<NodeId, ValoriNode>, Vec<u8>)>,
    audit: Box<dyn AuditSink>,
    /// C4.1b: unix-second creation timestamps for records, keyed by record id.
    /// Each replica stamps its own wall clock at apply time, so values agree
    /// only approximately (clock skew) — which is why they are NOT part of
    /// the BLAKE3 state hash. Snapshot install/restore overwrites a follower
    /// with the leader's values, re-converging the map exactly.
    created_at: HashMap<u32, u64>,
    /// BM25 text corpus — record_id → raw text for cluster-side reranking.
    /// The cluster_server reads this via with_text_corpus() and runs BM25
    /// locally after fetching vector candidates. Avoids a valori-node dep.
    text_corpus: std::collections::HashMap<u64, String>,
    /// Phase S2: cluster-wide, Raft-replicated name -> NamespaceId registry.
    /// Not part of the BLAKE3 state hash — replicated via Raft, converges identically.
    namespace_registry: CollectionRegistry,
    /// Set when a redb database is shared with the log store. Persists
    /// `last_applied`, `membership`, and snapshot data across restarts so
    /// openraft does not replay already-applied log entries through the
    /// AuditSink, which would produce duplicate `events.log` writes.
    db: Option<Arc<Database>>,
    /// When set, log entries up to and including this index are being replayed
    /// (catching up after restart). Audit writes are suppressed for these
    /// entries to prevent duplicate `events.log` lines — the entries were
    /// already written to audit before the restart.
    replay_until: Option<u64>,
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

    fn persist_applied(&self) -> Result<(), StorageError<NodeId>> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok(()),
        };
        let last_applied_bytes = sm_encode(&self.last_applied)?;
        let membership_bytes = sm_encode(&self.membership)?;
        let txn = db.begin_write().map_err(|e| io_err(format!("sm persist begin_write: {e}")))?;
        {
            let mut table = txn.open_table(SM_META).map_err(|e| io_err(format!("sm_meta open: {e}")))?;
            table.insert(KEY_SM_LAST_APPLIED, last_applied_bytes.as_slice())
                .map_err(|e| io_err(format!("sm_meta insert last_applied: {e}")))?;
            table.insert(KEY_SM_MEMBERSHIP, membership_bytes.as_slice())
                .map_err(|e| io_err(format!("sm_meta insert membership: {e}")))?;
        }
        txn.commit().map_err(|e| io_err(format!("sm persist commit: {e}")))?;
        Ok(())
    }

    fn persist_snapshot(&self) -> Result<(), StorageError<NodeId>> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok(()),
        };
        let (meta, data) = match &self.current_snapshot {
            Some(s) => s,
            None => return Ok(()),
        };
        let meta_bytes = sm_encode(meta)?;
        let txn = db.begin_write().map_err(|e| io_err(format!("sm snapshot begin_write: {e}")))?;
        {
            let mut table = txn.open_table(SM_META).map_err(|e| io_err(format!("sm_meta open: {e}")))?;
            table.insert(KEY_SM_SNAPSHOT_META, meta_bytes.as_slice())
                .map_err(|e| io_err(format!("sm_meta insert snapshot_meta: {e}")))?;
            table.insert(KEY_SM_SNAPSHOT_DATA, data.as_slice())
                .map_err(|e| io_err(format!("sm_meta insert snapshot_data: {e}")))?;
        }
        txn.commit().map_err(|e| io_err(format!("sm snapshot commit: {e}")))?;
        Ok(())
    }

    fn encode_kernel(&self) -> Result<Vec<u8>, StorageError<NodeId>> {
        let hint = valori_kernel::snapshot::encode::encode_capacity_hint(&self.state);
        let mut buf = Vec::with_capacity(hint);
        encode_state(&self.state, &mut buf)
            .map_err(|e| io_err(format!("kernel snapshot encode failed: {e:?}")))?;
        Ok(buf)
    }
}

fn io_err(msg: String) -> StorageError<NodeId> {
    StorageError::IO {
        source: StorageIOError::write(&std::io::Error::other(msg)),
    }
}

fn sm_encode<T: serde::Serialize>(v: &T) -> Result<Vec<u8>, StorageError<NodeId>> {
    bincode::serde::encode_to_vec(v, bincode::config::standard())
        .map_err(|e| io_err(format!("sm encode: {e}")))
}

fn sm_decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, StorageError<NodeId>> {
    bincode::serde::decode_from_slice(bytes, bincode::config::standard())
        .map(|(v, _)| v)
        .map_err(|e| io_err(format!("sm decode: {e}")))
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
        Self::new(Box::new(NullAuditSink), 0)
    }
}

impl ValoriStateMachine {
    pub fn new(audit: Box<dyn AuditSink>, dim: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StateMachineInner {
                state: KernelState::with_dim(dim),
                last_applied: None,
                membership: StoredMembership::default(),
                dedup_set: HashSet::new(),
                dedup_order: VecDeque::new(),
                current_snapshot: None,
                audit,
                db: None,
                replay_until: None,
                created_at: HashMap::new(),
                text_corpus: std::collections::HashMap::new(),
                namespace_registry: CollectionRegistry::new(),
            })),
        }
    }

    /// Construct a state machine backed by a shared redb database.
    ///
    /// Called instead of [`Self::new`] when `VALORI_RAFT_LOG_PATH` is set.
    /// Reads previously persisted `last_applied`, `membership`, and snapshot
    /// from the `sm_meta` table so openraft resumes from where it left off
    /// rather than replaying every committed entry through the AuditSink.
    pub fn with_db(
        audit: Box<dyn AuditSink>,
        db: Arc<Database>,
        dim: usize,
    ) -> Result<Self, StorageError<NodeId>> {
        // Read persisted state machine metadata.
        let txn = db.begin_read().map_err(|e| io_err(format!("sm_meta read txn: {e}")))?;
        let table = txn.open_table(SM_META).map_err(|e| io_err(format!("sm_meta open: {e}")))?;

        // The highest log index we applied before shutdown. Entries up to this
        // index have already been written to the audit log — replaying them
        // through the AuditSink would produce duplicates.
        let persisted_last_applied: Option<LogId<NodeId>> =
            match table.get(KEY_SM_LAST_APPLIED).map_err(|e| io_err(format!("sm_meta get: {e}")))? {
                Some(v) => sm_decode(v.value())?,
                None => None,
            };

        let membership: StoredMembership<NodeId, ValoriNode> =
            match table.get(KEY_SM_MEMBERSHIP).map_err(|e| io_err(format!("sm_meta get: {e}")))? {
                Some(v) => sm_decode(v.value())?,
                None => StoredMembership::default(),
            };

        // Restore kernel state from the persisted snapshot if one exists.
        let snapshot: Option<(SnapshotMeta<NodeId, ValoriNode>, Vec<u8>)> = match (
            table.get(KEY_SM_SNAPSHOT_META).map_err(|e| io_err(format!("sm_meta get: {e}")))?,
            table.get(KEY_SM_SNAPSHOT_DATA).map_err(|e| io_err(format!("sm_meta get: {e}")))?,
        ) {
            (Some(meta_v), Some(data_v)) => {
                let meta: SnapshotMeta<NodeId, ValoriNode> = sm_decode(meta_v.value())?;
                Some((meta, data_v.value().to_vec()))
            }
            _ => None,
        };
        drop(table);
        drop(txn);

        // replay_until: suppress audit writes for any entry at or below this
        // index, since those entries were already audited before the restart.
        // Cleared lazily in apply() once we advance past it.
        let replay_until: Option<u64> = persisted_last_applied.map(|l| l.index);

        // Determine the correct `last_applied` to report to openraft and the
        // in-memory state to start with:
        //
        // • Snapshot exists: restore state from the snapshot and tell openraft
        //   we're at `snapshot.last_log_id`. Entries from `snapshot.last_log_id + 1`
        //   up to `persisted_last_applied` will be replayed by openraft to bring
        //   the state back to where it was. Audit writes are suppressed for those
        //   entries because `replay_until = persisted_last_applied`.
        //
        // • No snapshot: start with empty state and tell openraft `last_applied = None`.
        //   Openraft replays ALL committed entries from the Raft log (or, if the log
        //   was compacted, requests a snapshot from the leader). Either way the state
        //   is rebuilt correctly. Audit writes are suppressed for entries up to
        //   `persisted_last_applied`, preventing duplicates.
        let (state, dedup_set, dedup_order, created_at, text_corpus, namespace_registry, current_snapshot, last_applied) = match snapshot {
            Some((meta, bytes)) => {
                let (payload, _): (SnapshotPayload, usize) =
                    bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
                        .map_err(|e| io_err(format!("persisted snapshot decode: {e}")))?;
                let state = decode_state(&payload.kernel)
                    .map_err(|e| io_err(format!("persisted snapshot kernel decode: {e:?}")))?;
                let actual = hash_state_blake3(&state);
                if actual != payload.state_hash {
                    return Err(io_err(format!(
                        "persisted snapshot state-hash mismatch: stored {} vs decoded {} — refusing restore",
                        hex(&payload.state_hash),
                        hex(&actual),
                    )));
                }
                let dedup_set: HashSet<[u8; 16]> = payload.dedup.iter().copied().collect();
                let dedup_order: VecDeque<[u8; 16]> = payload.dedup.into();
                // M-1: restore decay timestamps and BM25 corpus from snapshot.
                let created_at: HashMap<u32, u64> = payload.created_at.into_iter().collect();
                let text_corpus: std::collections::HashMap<u64, String> = payload.text_corpus.into_iter().collect();
                // S2: restore the namespace registry from the snapshot.
                let namespace_registry = CollectionRegistry {
                    map: payload.namespace_registry.0.into_iter().collect(),
                    next_id: payload.namespace_registry.1,
                };
                // Use the snapshot's own last_log_id, NOT the separately persisted
                // last_applied. Openraft will replay snapshot.last_log_id+1 onward.
                let last_applied = meta.last_log_id;
                (state, dedup_set, dedup_order, created_at, text_corpus, namespace_registry, Some((meta, bytes)), last_applied)
            }
            None => {
                // No snapshot: start from scratch. Returning last_applied=None
                // tells openraft to replay the full committed log so the
                // in-memory KernelState is rebuilt from real entries rather than
                // being left empty with a stale last_applied pointer.
                (KernelState::with_dim(dim), HashSet::new(), VecDeque::new(), HashMap::new(), std::collections::HashMap::new(), CollectionRegistry::new(), None, None)
            }
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(StateMachineInner {
                state,
                last_applied,
                membership,
                dedup_set,
                dedup_order,
                current_snapshot,
                audit,
                db: Some(db),
                replay_until,
                created_at,
                text_corpus,
                namespace_registry,
            })),
        })
    }

    /// Current BLAKE3 state hash — the cross-node equality invariant.
    pub async fn state_hash(&self) -> [u8; 32] {
        hash_state_blake3(&self.inner.lock().await.state)
    }

    /// The dimension the kernel has actually locked to (set on first insert).
    /// Returns `None` if no records have been inserted yet.
    pub async fn locked_dim(&self) -> Option<usize> {
        self.inner.lock().await.state.dim
    }

    /// Run a read-only closure against the kernel state (Phase 2.5 uses
    /// this for serving reads without copying the state out).
    pub async fn with_state<T>(&self, f: impl FnOnce(&KernelState) -> T) -> T {
        f(&self.inner.lock().await.state)
    }

    /// Clone the current kernel state, releasing the lock immediately.
    /// Use this when the subsequent work is CPU-heavy (e.g. snapshot encoding)
    /// and should run outside the async lock on a blocking thread pool.
    pub async fn clone_state(&self) -> KernelState {
        self.inner.lock().await.state.clone()
    }

    /// Look up a `SetMeta`-committed value by key and parse it as JSON.
    /// Reads the replicated `KernelState::meta` map, not any per-node sidecar,
    /// so every replica answers identically regardless of which node handled
    /// the original write.
    pub async fn get_meta_json(&self, key: &str) -> Option<serde_json::Value> {
        self.inner.lock().await.state.meta.get(key)
            .and_then(|s| serde_json::from_str(s).ok())
    }

    /// C4.1b: unix-second creation timestamp for a record, or None if unknown.
    pub async fn record_created_at(&self, id: u32) -> Option<u64> {
        self.inner.lock().await.created_at.get(&id).copied()
    }

    /// C4.1b: run a closure with both kernel state and created_at map.
    pub async fn with_state_and_timestamps<T>(
        &self,
        f: impl FnOnce(&KernelState, &HashMap<u32, u64>) -> T,
    ) -> T {
        let inner = self.inner.lock().await;
        f(&inner.state, &inner.created_at)
    }

    /// Run a read-only closure against the BM25 text corpus.
    /// cluster_server builds a ValoriReranker on the fly from these texts.
    pub async fn with_text_corpus<T>(
        &self,
        f: impl FnOnce(&std::collections::HashMap<u64, String>) -> T,
    ) -> T {
        f(&self.inner.lock().await.text_corpus)
    }

    /// Phase S2: resolve a collection name to its NamespaceId via the
    /// replicated registry. `None`/`Some("default")` resolves to `Some(0)`.
    /// A cheap in-memory HashMap read under the same lock every other state
    /// read already takes — this is the single source of truth for
    /// cluster-mode namespace resolution (replaces the old, per-node,
    /// non-replicated `NamespaceRegistry` in valori-node).
    pub async fn resolve_namespace(&self, name: Option<&str>) -> Option<u16> {
        self.inner.lock().await.namespace_registry.resolve(name)
    }

    /// Phase S2: list every known collection (name, id), "default" first.
    pub async fn list_namespaces(&self) -> Vec<(String, u16)> {
        self.inner.lock().await.namespace_registry.list()
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
                        rejected: None,
                        allocated_record_id: None,
                        allocated_node_id: None,
                        allocated_edge_id: None,
                        allocated_namespace_id: None,
                    });
                }
                EntryPayload::Membership(m) => {
                    inner.membership = StoredMembership::new(Some(entry.log_id), m);
                    replies.push(ClientResponse {
                        log_index,
                        state_hash: hash_state_blake3(&inner.state),
                        deduplicated: false,
                        rejected: None,
                        allocated_record_id: None,
                        allocated_node_id: None,
                        allocated_edge_id: None,
                        allocated_namespace_id: None,
                    });
                }
                EntryPayload::Normal(req) => {
                    // H-4: Version gate — entries from a newer leader schema are
                    // rejected at the application layer (not StorageError) so the
                    // node stays alive and replication continues. A StorageError here
                    // would permanently halt the node, allowing a single crafted entry
                    // to take down a cluster node. The operator warning drives the upgrade.
                    if req.schema_version > CURRENT_SCHEMA_VERSION {
                        tracing::error!(
                            log_index,
                            schema_version = req.schema_version,
                            current = CURRENT_SCHEMA_VERSION,
                            "log entry schema version too new — entry REJECTED, node needs upgrade"
                        );
                        inner.last_applied = Some(entry.log_id);
                        replies.push(ClientResponse {
                            log_index,
                            state_hash: hash_state_blake3(&inner.state),
                            deduplicated: false,
                            rejected: Some(format!(
                                "schema version {} exceeds this node's max ({CURRENT_SCHEMA_VERSION})",
                                req.schema_version
                            )),
                            allocated_record_id: None,
                            allocated_node_id: None,
                            allocated_edge_id: None,
                            allocated_namespace_id: None,
                        });
                        continue;
                    }

                    // 1. Dedup — replicated decision, identical on all nodes.
                    if let Some(id) = req.request_id {
                        if inner.dedup_set.contains(&id) {
                            replies.push(ClientResponse {
                                log_index,
                                state_hash: hash_state_blake3(&inner.state),
                                deduplicated: true,
                                rejected: None,
                                allocated_record_id: None,
                                allocated_node_id: None,
                                allocated_edge_id: None,
                                allocated_namespace_id: None,
                            });
                            continue;
                        }
                    }

                    // For Auto* events the handler doesn't pre-allocate an
                    // ID — the state machine picks the next ID here, which is
                    // deterministic because all replicas apply entries in the
                    // same Raft-ordered sequence.
                    let pre_alloc_id: Option<KRecordId> =
                        if matches!(&req.event, KernelEvent::AutoInsertRecord { .. } | KernelEvent::AutoInsertRecordEncrypted { .. }) {
                            Some(inner.state.next_record_id())
                        } else {
                            None
                        };
                    let pre_alloc_node_id: Option<u32> =
                        if matches!(&req.event, KernelEvent::AutoCreateNode { .. }) {
                            Some(inner.state.next_node_id().0)
                        } else {
                            None
                        };
                    let pre_alloc_edge_id: Option<u32> =
                        if matches!(&req.event, KernelEvent::AutoCreateEdge { .. }) {
                            Some(inner.state.next_edge_id().0)
                        } else {
                            None
                        };

                    // S2: namespace registry is resolved/mutated here, inside
                    // the same Raft-ordered apply loop, so every replica
                    // makes the identical decision — same reasoning as
                    // pre_alloc_id above. AutoCreateNamespace speculatively
                    // inserts (idempotent — a name already registered just
                    // returns its existing id); DropNamespace only resolves
                    // (read-only) here, removal happens after a confirmed
                    // successful kernel apply, below.
                    let mut ns_registry_err: Option<&'static str> = None;
                    let resolved_namespace_id: Option<u16> = match &req.event {
                        KernelEvent::AutoCreateNamespace { name } => {
                            match inner.namespace_registry.create(name) {
                                Some(id) => Some(id),
                                None => {
                                    ns_registry_err = Some("namespace limit reached");
                                    None
                                }
                            }
                        }
                        KernelEvent::DropNamespace { name } => {
                            inner.namespace_registry.resolve(Some(name))
                        }
                        _ => None,
                    };

                    // 2. Kernel apply. Rejections are deterministic too —
                    //    every node rejects identically; state is untouched.
                    //    The entry is still consumed (last_applied advanced).
                    let rejected = if let Some(e) = ns_registry_err {
                        Some(e.to_string())
                    } else {
                        match &req.event {
                            KernelEvent::AutoCreateNamespace { .. } => {
                                let id = resolved_namespace_id
                                    .expect("resolved above whenever ns_registry_err is None");
                                inner.state.apply_event_ns(&req.event, id).err().map(|e| format!("{e:?}"))
                            }
                            KernelEvent::DropNamespace { name } => match resolved_namespace_id {
                                Some(id) => inner.state.apply_event_ns(&req.event, id).err().map(|e| format!("{e:?}")),
                                None => Some(format!("namespace '{name}' not found")),
                            },
                            // S3a: dispatch through apply_event_ns with the
                            // request's namespace_id instead of the
                            // DEFAULT_NS-hardcoded apply_event(). Backward
                            // compatible: req.namespace_id defaults to 0 for
                            // old callers, byte-identical to prior behavior.
                            // Variants that carry their own internal
                            // namespace_id (InsertRecordEncrypted family)
                            // ignore this parameter and use their own field.
                            _ => inner.state.apply_event_ns(&req.event, req.namespace_id).err().map(|e| format!("{e:?}")),
                        }
                    };

                    // 3. Audit record + dedup memory — successful applies only.
                    // During replay (log_index <= replay_until), the entry was
                    // already written to the audit log before the restart, so
                    // we skip the write to prevent duplicates. Dedup memory IS
                    // rebuilt during replay so future request_id dedup is correct.
                    let in_replay = inner.replay_until.map(|t| log_index <= t).unwrap_or(false);
                    // Clear replay mode once we advance past the target index.
                    if !in_replay {
                        inner.replay_until = None;
                    }
                    let (allocated_record_id, allocated_node_id, allocated_edge_id) =
                        if rejected.is_none() {
                            if let Some(id) = req.request_id {
                                inner.remember_request(id);
                            }
                            if !in_replay {
                                inner
                                    .audit
                                    .record(&req.event, req.request_id)
                                    .map_err(|e| io_err(format!("audit sink write failed: {e}")))?;
                            }
                            // C4.1b: stamp creation time for decay on AutoInsertRecord.
                            if let Some(rid) = pre_alloc_id {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs()).unwrap_or(0);
                                inner.created_at.insert(rid.0, now);
                                // BM25: store raw text from metadata for cluster reranking
                                if let KernelEvent::AutoInsertRecord { ref metadata, .. } = req.event {
                                    if let Some(ref meta_bytes) = metadata {
                                        if let Ok(text) = std::str::from_utf8(meta_bytes) {
                                            inner.text_corpus.insert(rid.0 as u64, text.to_string());
                                        }
                                    }
                                }
                            }
                            (
                                pre_alloc_id.map(|r| r.0),
                                pre_alloc_node_id,
                                pre_alloc_edge_id,
                            )
                        } else {
                            (None, None, None)
                        };

                    // S2: finalize the namespace registry mutation now that
                    // we know whether the kernel apply succeeded.
                    // AutoCreateNamespace: undo the speculative insert if the
                    // kernel rejected it, so the registry and KernelState
                    // never diverge (there is no kernel-side source of truth
                    // to reconcile against otherwise). DropNamespace: only
                    // remove from the map on a CONFIRMED successful kernel
                    // apply — resolution above was read-only.
                    let allocated_namespace_id = match &req.event {
                        KernelEvent::AutoCreateNamespace { name } => {
                            if rejected.is_some() {
                                inner.namespace_registry.map.remove(name);
                                None
                            } else {
                                resolved_namespace_id
                            }
                        }
                        KernelEvent::DropNamespace { name } => {
                            if rejected.is_none() {
                                inner.namespace_registry.map.remove(name);
                            }
                            None
                        }
                        _ => None,
                    };

                    replies.push(ClientResponse {
                        log_index,
                        state_hash: hash_state_blake3(&inner.state),
                        deduplicated: false,
                        rejected,
                        allocated_record_id,
                        allocated_node_id,
                        allocated_edge_id,
                        allocated_namespace_id,
                    });
                }
            }
        }
        // Persist last_applied + membership once per batch so a restarted node
        // does not replay already-applied entries through the AuditSink.
        inner.persist_applied()?;
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

        // Kernel (V6) refusal semantics apply: a foreign-format snapshot
        // fails here and the node keeps its old state.
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
        // M-1: restore decay timestamps and BM25 corpus from snapshot.
        inner.created_at = payload.created_at.into_iter().collect();
        inner.text_corpus = payload.text_corpus.into_iter().collect();
        // S2: restore the namespace registry from the snapshot.
        inner.namespace_registry = CollectionRegistry {
            map: payload.namespace_registry.0.into_iter().collect(),
            next_id: payload.namespace_registry.1,
        };
        inner.current_snapshot = Some((meta.clone(), bytes));
        inner.persist_snapshot()?;
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

        // M-1: include created_at and text_corpus so a restored follower has
        // the same decay-ranking and BM25 data as the leader.
        let payload = SnapshotPayload {
            kernel: inner.encode_kernel()?,
            dedup: inner.dedup_order.iter().copied().collect(),
            state_hash: hash_state_blake3(&inner.state),
            created_at: inner.created_at.iter().map(|(&k, &v)| (k, v)).collect(),
            text_corpus: inner.text_corpus.iter().map(|(&k, v)| (k, v.clone())).collect(),
            namespace_registry: (
                inner.namespace_registry.map.iter().map(|(k, &v)| (k.clone(), v)).collect(),
                inner.namespace_registry.next_id,
            ),
        };
        let bytes = bincode::serde::encode_to_vec(&payload, bincode::config::standard())
            .map_err(|e| io_err(format!("snapshot payload encode failed: {e}")))?;

        // Snapshot identity is derived, not counted: (last_applied index,
        // state-hash prefix). Identical state yields the identical ID (the
        // snapshots are interchangeable); different state can't collide.
        // No mutable counter to persist across restarts.
        let meta = SnapshotMeta {
            last_log_id: inner.last_applied,
            last_membership: inner.membership.clone(),
            snapshot_id: format!(
                "{}-{}",
                inner.last_applied.map_or(0, |l| l.index),
                &hex(&payload.state_hash)[..16],
            ),
        };
        inner.current_snapshot = Some((meta.clone(), bytes.clone()));
        inner.persist_snapshot()?;

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(bytes)),
        })
    }
}
