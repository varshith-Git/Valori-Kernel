// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `commit` module — the Committer trait seam.
//!
//! This module is the single narrow interface through which ALL state mutations
//! flow. Phase 1.9 delivers:
//!   - The `Committer` trait (this file)
//!   - `StandaloneCommitter` (standalone.rs) — wraps EventCommitter verbatim
//!   - `WalCommitter` (wal.rs) — wraps the legacy WalWriter path
//!
//! Phase 2 adds `RaftCommitter` in `valori-consensus`, satisfying this same
//! trait. The `Engine` struct will own `Box<dyn Committer>` instead of the
//! current `Option<EventCommitter>` + `Option<WalWriter>` pair.
//!
//! ## Why this matters for capacity checks
//!
//! Today capacity guards (HTTP 507) run in `Engine::insert_record_from_f32`
//! and friends, BEFORE any commit path. In Phase 1.9 they move INSIDE
//! `StandaloneCommitter::commit` (into shadow execution against KernelState).
//! This makes them deterministic and replication-safe: a follower applying
//! the same event through `KernelState::apply_event` respects capacity natively.
//!
//! **Phase 1.9 — reserved module skeleton. StandaloneCommitter implementation
//! wired into Engine in Phase 1.9.**
//!
//! See docs/phases/phase-1.9-committer-trait.md for the full design.

pub mod standalone;
pub use standalone::StandaloneCommitter;

pub mod audit;
pub mod raft;
pub use audit::EventLogAuditSink;
pub use raft::RaftCommitter;

use valori_kernel::event::KernelEvent;
use thiserror::Error;

/// Result of a successful commit.
#[derive(Debug, Clone, Copy)]
pub struct CommitReceipt {
    /// Monotonically increasing log index of the committed event.
    /// Standalone mode: EventJournal committed height.
    /// Cluster mode (Phase 2): Raft commit index.
    pub log_index: u64,
}

/// All pool-level errors that can occur during a commit attempt.
/// Surfaced to HTTP handlers as HTTP 507 with a structured body.
#[derive(Debug, Error)]
pub enum CommitError {
    #[error("capacity exceeded: {pool} pool is full ({used}/{cap})")]
    Capacity {
        pool: &'static str,
        used: usize,
        cap:  usize,
    },

    #[error("shadow application rejected event: {0:?}")]
    Apply(valori_kernel::error::KernelError),

    #[error("persistence layer error: {0}")]
    Io(String),

    #[error("batch was empty — nothing to commit")]
    EmptyBatch,

    // ── Phase 2.5 cluster-mode variants ──────────────────────────────────────
    /// The replicated state machine deterministically rejected the event
    /// (every node rejected identically; state untouched).
    #[error("event rejected by the replicated state machine: {0}")]
    Rejected(String),

    /// This node is a follower. The HTTP layer answers 307 with the
    /// leader's API address (Phase 2.6).
    #[error("not the leader{}", leader_api_addr.as_deref().map(|a| format!(" — leader API at {a}")).unwrap_or_default())]
    NotLeader { leader_api_addr: Option<String> },
}

/// The one way to mutate KernelState through the Engine.
///
/// # Invariants
///
/// * **Atomicity**: Either the event is committed (persisted + applied to live
///   state) or it is not; no partial states are visible.
/// * **Order**: Commits are strictly ordered by the returned `log_index`.
/// * **Determinism**: The same sequence of events produces the same
///   KernelState regardless of which implementation is behind the trait.
/// * **Capacity**: If shadow application returns `KernelError::CapacityExceeded`,
///   `commit` MUST return `Err(CommitError::Capacity)` without any I/O.
///
/// # Phase 1.9 note
///
/// `Engine` currently has `Option<EventCommitter>` and `Option<WalWriter>`.
/// Phase 1.9 replaces both with `committer: Box<dyn Committer>`.
/// All `Engine::*` methods that currently contain:
/// ```text
///   if let Some(ref mut committer) = self.event_committer { … } else { … }
/// ```
/// become a single `self.committer.commit(event)?;` call.
pub trait Committer: Send + Sync {
    /// Attempt to commit a single event.
    fn commit(&mut self, event: KernelEvent) -> Result<CommitReceipt, CommitError>;

    /// Attempt to commit a batch of events atomically.
    /// Default: commit one at a time. Override for group-commit optimisation.
    fn commit_batch(&mut self, events: Vec<KernelEvent>) -> Result<CommitReceipt, CommitError> {
        let mut last = None;
        for event in events {
            last = Some(self.commit(event)?);
        }
        last.ok_or(CommitError::EmptyBatch)
    }

    /// Current committed log height. Used by health reporting and metrics.
    fn log_height(&self) -> u64;

    /// Flush any in-memory write buffer to durable storage.
    /// No-op in implementations that fsync on every commit.
    fn flush(&mut self) -> Result<(), CommitError>;
}
