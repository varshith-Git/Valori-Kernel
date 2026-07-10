// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! openraft type configuration — Phase 2.1.
//!
//! This module pins every type parameter openraft is generic over, in one
//! place. Everything in Phase 2.2–2.10 (log store, state machine, network,
//! committer) is written against `TypeConfig`; changing a type here is a
//! cluster-wide wire change and must be treated like a format bump.
//!
//! ## The application command: `ClientRequest`
//!
//! Raft replicates `ClientRequest`, not bare `KernelEvent` — the envelope
//! carries the Phase 1.2 idempotency token so the dedup decision survives
//! leader retries and is itself replicated (every node makes the same
//! dedup decision deterministically).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Cursor;
use valori_kernel::event::KernelEvent;

/// Stable numeric node identity. Comes from `VALORI_NODE_ID` (Phase 1.8).
pub type NodeId = u64;

/// Identifies one independent Raft group ("shard") within a process
/// (Phase S1 — multi-Raft skeleton). `ShardId(0)` is the sole shard when
/// `VALORI_SHARD_COUNT=1`. Namespace-to-shard HTTP routing is live (S3–S9):
/// `shard_for_namespace(ns, count) = ns % count` in valori-node.
///
/// This is the shared `valori-core` type (re-exported through the kernel) —
/// the same `ShardId` the rest of the platform uses, not a local duplicate.
pub use valori_kernel::types::id::ShardId;

/// A cluster member as known to Raft membership config.
///
/// `api_addr` is the public HTTP data-plane address (axum, port 3000-ish);
/// `raft_addr` is the internal gRPC consensus address (Phase 2.4,
/// `VALORI_RAFT_BIND`, port 3100-ish). Both travel inside membership
/// entries so any node can tell a client where the leader's API lives.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ValoriNode {
    pub api_addr: String,
    pub raft_addr: String,
}

impl fmt::Display for ValoriNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "api={} raft={}", self.api_addr, self.raft_addr)
    }
}

/// The schema version this node writes into every `ClientRequest`.
///
/// Bump this constant when a new `KernelEvent` variant or any field change
/// would make the entry uninterpretable to an older node.  The state machine
/// refuses entries whose `schema_version` exceeds this value.
///
/// Rolling-upgrade window: a cluster may mix nodes at `CURRENT_SCHEMA_VERSION`
/// and `CURRENT_SCHEMA_VERSION - 1` simultaneously. Once all nodes are
/// upgraded, the older version drops out of the window.
pub const CURRENT_SCHEMA_VERSION: u8 = 0;

/// The command Raft replicates. One kernel event plus its idempotency token.
///
/// EVOLUTION: this struct crosses the wire between nodes — fields are
/// append-only with `#[serde(default)]`, same policy as valori-wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRequest {
    /// The deterministic kernel event to apply once committed.
    pub event: KernelEvent,
    /// Client-supplied idempotency token (Phase 1.2 schema). The state
    /// machine drops a request whose token it has already applied, so a
    /// leader-failover retry cannot double-apply.
    #[serde(default)]
    pub request_id: Option<[u8; 16]>,
    /// Schema version written by the leader at proposal time (Phase 3.2).
    /// Nodes running an older schema version that receive an entry with a
    /// higher version refuse to apply it and log an error — halting
    /// replication until the operator upgrades the node.
    /// Old nodes that pre-date this field decode it as 0 (`#[serde(default)]`).
    #[serde(default)]
    pub schema_version: u8,
    /// Phase S3a: which namespace this event targets. The state machine
    /// calls `KernelState::apply_event_ns(event, namespace_id)` with this
    /// value for every event type whose own `KernelEvent` variant doesn't
    /// already carry an internal `namespace_id` field (i.e. everything
    /// except `InsertRecordEncrypted`/`AutoInsertRecordEncrypted`, which
    /// carry their own and ignore this one). Old requests decode this as 0
    /// (`#[serde(default)]`) — identical to prior behavior, since every
    /// write went to namespace 0 regardless before this field existed.
    #[serde(default)]
    pub namespace_id: u16,
}

/// What the state machine returns to the waiting client after apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientResponse {
    /// Raft log index at which the event was applied.
    pub log_index: u64,
    /// BLAKE3 state hash after this apply — lets the client (or a proxying
    /// follower) verify it observed the same state the leader produced.
    pub state_hash: [u8; 32],
    /// True when the request was recognised as a duplicate by its
    /// `request_id` and skipped; `state_hash` is still the current hash.
    #[serde(default)]
    pub deduplicated: bool,
    /// `Some(reason)` when the kernel deterministically rejected the event
    /// (bad sequential id, dimension mismatch). State is untouched; every
    /// node rejected identically. Added in Phase 2.5 (append-only field).
    #[serde(default)]
    pub rejected: Option<String>,
    /// Populated when the request used `KernelEvent::AutoInsertRecord`.
    /// The record ID assigned by the state machine at apply time — every
    /// replica assigned the same ID, so this is the canonical value.
    #[serde(default)]
    pub allocated_record_id: Option<u32>,
    /// Populated when the request used `KernelEvent::AutoCreateNode`.
    #[serde(default)]
    pub allocated_node_id: Option<u32>,
    /// Populated when the request used `KernelEvent::AutoCreateEdge`.
    #[serde(default)]
    pub allocated_edge_id: Option<u32>,
    /// Populated when the request used `KernelEvent::AutoCreateNamespace`.
    /// The NamespaceId assigned by the state machine at apply time (or the
    /// pre-existing id, if the name was already registered — idempotent).
    /// Phase S2.
    #[serde(default)]
    pub allocated_namespace_id: Option<u16>,
}

openraft::declare_raft_types!(
    /// Every openraft type parameter for Valori, fixed in one place.
    pub TypeConfig:
        D = ClientRequest,
        R = ClientResponse,
        NodeId = NodeId,
        Node = ValoriNode,
        Entry = openraft::Entry<TypeConfig>,
        SnapshotData = Cursor<Vec<u8>>,
        AsyncRuntime = openraft::TokioRuntime,
);

/// Shorthands used across the crate (and by valori-node in Phase 2.5).
pub type LogId = openraft::LogId<NodeId>;
pub type Vote = openraft::Vote<NodeId>;
pub type Entry = openraft::Entry<TypeConfig>;
pub type StoredMembership = openraft::StoredMembership<NodeId, ValoriNode>;
pub type SnapshotMeta = openraft::SnapshotMeta<NodeId, ValoriNode>;
pub type Raft = openraft::Raft<TypeConfig>;
