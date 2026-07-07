// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `Persistence` — the single standalone durability funnel (Phase E1).
//!
//! Before E1, `Engine` held `Option<EventCommitter>` AND `Option<WalWriter>`
//! and every write method contained the same dual branch:
//!
//! ```text
//!   if let Some(ref mut committer) = self.event_committer { … }
//!   else { if let Some(ref mut writer) = self.wal_writer { … } … }
//! ```
//!
//! duplicated across 10+ methods. This enum collapses the choice into ONE
//! place: `Engine` owns a `Persistence` and every mutation flows through
//! `Engine::commit_and_apply_ns` → `Persistence::log_event_ns`.
//!
//! Why an enum and not `Box<dyn Committer>` (the original Phase 1.9 plan):
//! ~40 call sites across server.rs, replication.rs, valori-ffi, and tests
//! need the *concrete* `EventCommitter` (journal heights, log rotation,
//! subscribe streams, wholesale replacement during recovery). A trait object
//! would force a downcast at every one of them. The enum keeps static
//! dispatch and offers `event_committer()` / `event_committer_mut()`
//! accessors instead. The `Committer` trait remains the cluster seam
//! (`RaftCommitter`, `EventLogAuditSink` in cluster.rs).

use crate::commit::CommitError;
use crate::events::event_commit::{CommitError as EventCommitError, EventCommitter};
use crate::wal_writer::WalWriter;
use valori_kernel::event::KernelEvent;
use valori_kernel::state::command::Command;

/// The standalone durability backend. Exactly one is active per engine:
/// the event log supersedes the WAL entirely when both are configured
/// (initialising both would double-write; see `Engine::new`).
pub enum Persistence {
    /// Event-sourced: BLAKE3-chained event log (canonical since Phase 23).
    EventLog(EventCommitter),
    /// Legacy WAL: bincode command log (pre-Phase-23 persistence).
    Wal(WalWriter),
    /// In-memory only — no durability configured.
    Ephemeral,
}

impl Persistence {
    /// Concrete access for observability call sites (proof, timeline,
    /// receipts, replication streaming). `None` unless event-log backed.
    pub fn event_committer(&self) -> Option<&EventCommitter> {
        match self {
            Persistence::EventLog(c) => Some(c),
            _ => None,
        }
    }

    pub fn event_committer_mut(&mut self) -> Option<&mut EventCommitter> {
        match self {
            Persistence::EventLog(c) => Some(c),
            _ => None,
        }
    }

    /// Durably log one event, scoped to `namespace_id`.
    /// Does NOT apply the event to engine state — the caller
    /// (`Engine::commit_and_apply_ns`) does that exactly once afterwards.
    ///
    /// EventLog: shadow-apply → persist → live-apply (inside EventCommitter).
    /// Wal: append the equivalent `Command`; events the legacy WAL format
    /// cannot represent (`SetMeta`) are skipped, matching pre-E1 behavior.
    /// Ephemeral: no-op.
    pub fn log_event_ns(&mut self, event: &KernelEvent, namespace_id: u16) -> Result<(), CommitError> {
        match self {
            Persistence::EventLog(c) => c
                .commit_event_ns(event.clone(), namespace_id)
                .map(|_| ())
                .map_err(translate),
            Persistence::Wal(w) => {
                if let Some(cmd) = command_for(event, namespace_id) {
                    w.append_command(&cmd)
                        .map_err(|e| CommitError::Io(e.to_string()))?;
                }
                Ok(())
            }
            Persistence::Ephemeral => Ok(()),
        }
    }

    /// Durably log a batch of events atomically (event log) or sequentially
    /// (WAL), scoped to `namespace_id`. Same apply contract as `log_event_ns`.
    pub fn log_batch_ns(&mut self, events: &[KernelEvent], namespace_id: u16) -> Result<(), CommitError> {
        match self {
            Persistence::EventLog(c) => c
                .commit_batch_ns(events.to_vec(), namespace_id)
                .map(|_| ())
                .map_err(translate),
            Persistence::Wal(w) => {
                for event in events {
                    if let Some(cmd) = command_for(event, namespace_id) {
                        w.append_command(&cmd)
                            .map_err(|e| CommitError::Io(e.to_string()))?;
                    }
                }
                Ok(())
            }
            Persistence::Ephemeral => Ok(()),
        }
    }
}

/// Translate a `KernelEvent` into the legacy WAL `Command`, attaching the
/// namespace. Returns `None` for events the `Command` enum cannot represent
/// (`SetMeta` and namespace lifecycle events, which pre-E1 code never
/// WAL-appended either).
fn command_for(event: &KernelEvent, namespace_id: u16) -> Option<Command> {
    match event {
        KernelEvent::InsertRecord { id, vector, metadata, tag } => Some(Command::InsertRecord {
            namespace_id,
            id: *id,
            vector: vector.clone(),
            metadata: metadata.clone(),
            tag: *tag,
        }),
        KernelEvent::InsertRecordEncrypted { id, key_id, ciphertext, tag, .. } => {
            Some(Command::InsertRecordEncrypted {
                namespace_id,
                id: *id,
                key_id: *key_id,
                ciphertext: ciphertext.clone(),
                tag: *tag,
            })
        }
        KernelEvent::DeleteRecord { id } => Some(Command::DeleteRecord { id: *id }),
        KernelEvent::SoftDeleteRecord { id } => Some(Command::SoftDeleteRecord { id: *id }),
        KernelEvent::CreateNode { id, kind, record } => Some(Command::CreateNode {
            namespace_id,
            node_id: *id,
            kind: *kind,
            record: *record,
        }),
        KernelEvent::CreateEdge { id, from, to, kind } => Some(Command::CreateEdge {
            edge_id: *id,
            kind: *kind,
            from: *from,
            to: *to,
        }),
        KernelEvent::DeleteNode { id } => Some(Command::DeleteNode { node_id: *id }),
        KernelEvent::DeleteEdge { id } => Some(Command::DeleteEdge { edge_id: *id }),
        KernelEvent::ShredKey { key_id } => Some(Command::ShredKey { key_id: *key_id }),
        _ => None,
    }
}

fn translate(e: EventCommitError) -> CommitError {
    match e {
        EventCommitError::LiveApply(ke) => CommitError::Apply(ke),
        EventCommitError::ShadowApply(ke) => CommitError::Apply(ke),
        EventCommitError::EventLog(_) | EventCommitError::VerificationFailed => {
            CommitError::Io(e.to_string())
        }
    }
}
