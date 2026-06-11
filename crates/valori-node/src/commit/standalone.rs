// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! StandaloneCommitter — the single-node implementation of `Committer`.
//!
//! Wraps `EventCommitter` (which owns the event log, journal, and live
//! `KernelState`) and exposes it through the `Committer` trait so the engine
//! can call `self.committer.commit(event)` without caring whether the backend
//! is the standalone log writer or (in Phase 2) Raft consensus.
//!
//! # Why the capacity check is here
//!
//! Previously capacity guards ran inside `Engine::insert_record_from_f32` etc.
//! Moving them into `StandaloneCommitter::commit` makes the check deterministic:
//! it happens inside a shadow application against `KernelState`, so a Phase 2
//! Raft follower replaying the same event will see the same error path.

use crate::commit::{CommitError, CommitReceipt, Committer};
use crate::events::event_commit::{CommitError as EventCommitError, EventCommitter};
use valori_kernel::event::KernelEvent;

/// Wraps `EventCommitter` and implements `Committer`.
///
/// In Phase 1.9, `Engine` holds an `Option<StandaloneCommitter>` alongside
/// its existing `Option<EventCommitter>`. Phase 2 wires `Box<dyn Committer>`
/// and removes both `Option`s.
pub struct StandaloneCommitter {
    inner: EventCommitter,
}

impl StandaloneCommitter {
    pub fn new(inner: EventCommitter) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &EventCommitter {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut EventCommitter {
        &mut self.inner
    }

    pub fn into_inner(self) -> EventCommitter {
        self.inner
    }
}

impl Committer for StandaloneCommitter {
    fn commit(&mut self, event: KernelEvent) -> Result<CommitReceipt, CommitError> {
        self.inner.commit_event(event).map(|_| CommitReceipt {
            log_index: self.inner.journal().committed_height(),
        }).map_err(translate)
    }

    fn commit_batch(&mut self, events: Vec<KernelEvent>) -> Result<CommitReceipt, CommitError> {
        if events.is_empty() {
            return Err(CommitError::EmptyBatch);
        }
        self.inner.commit_batch(events).map(|_| CommitReceipt {
            log_index: self.inner.journal().committed_height(),
        }).map_err(translate)
    }

    fn log_height(&self) -> u64 {
        self.inner.journal().committed_height()
    }

    fn flush(&mut self) -> Result<(), CommitError> {
        self.inner.flush_log().map_err(|e| CommitError::Io(e.to_string()))
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
