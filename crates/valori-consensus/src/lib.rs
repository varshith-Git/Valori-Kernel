// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Raft consensus layer for Valori cluster mode.
//!
//! Phase 2 of the multi-node roadmap (see `docs/phases/README.md`):
//!
//! | Module | Phase | What it is |
//! |---|---|---|
//! | [`types`] | 2.1 | openraft type config — every generic pinned once |
//! | `log_store` | 2.2 | Raft log storage (internal, truncatable) |
//! | `state_machine` | 2.3 | KernelState adapter + audit-log write at apply |
//! | `network` | 2.4 | tonic/gRPC transport between peers |
//!
//! The `Committer` implementation over the Raft handle (`RaftCommitter`)
//! lives in valori-node's `commit::raft` — the trait is node-side, and
//! node depends on this crate (Phase 2.5).
//!
//! ## The one rule
//!
//! **Raft commits, kernel applies, audit log records.** The Raft log is
//! internal plumbing — truncatable, purgeable, never shown to auditors.
//! The audit log (`events.log`, BLAKE3-chained) is written exactly once
//! per event, at APPLY time, strictly after quorum commit. The two must
//! never be conflated.

pub mod log_store;
pub mod log_store_redb;
pub mod network;
pub mod state_machine;
pub mod types;

pub use log_store::ValoriLogStore;
pub use log_store_redb::RedbLogStore;
pub use network::{
    serve_raft, serve_raft_tls, RaftRpcService, RaftTlsConfig, ValoriNetwork,
    ValoriNetworkFactory,
};
pub use state_machine::{AuditSink, MemoryAuditSink, NullAuditSink, ValoriStateMachine};
pub use types::{
    ClientRequest, ClientResponse, NodeId, TypeConfig, ValoriNode,
};
