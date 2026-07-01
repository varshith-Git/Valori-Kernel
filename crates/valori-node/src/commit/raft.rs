// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! RaftCommitter — Phase 2.5. The Phase 1.9 stub made real.
//!
//! Implements the [`Committer`] seam over an openraft handle: `commit`
//! becomes `Raft::client_write`, which returns only after the event is
//! quorum-replicated AND applied to the local kernel (and therefore audited
//! — the state machine writes the chained `events.log` at apply).
//!
//! ## Sync-over-async bridge
//!
//! `Committer` is a synchronous trait (the standalone path is synchronous
//! fsync-backed I/O), while `client_write` is async. The committer captures
//! a runtime handle at construction; `commit` uses `block_in_place` when
//! called from a runtime worker (multi-thread runtime required — which the
//! node always runs) and plain `block_on` from non-async contexts.
//!
//! ## Error mapping
//!
//! - Kernel rejection (deterministic, replicated) → `CommitError::Rejected`
//! - `ForwardToLeader` → `CommitError::NotLeader` carrying the leader's API
//!   address, so the HTTP layer (Phase 2.6) can answer 307 with a Location.
//! - Anything else (no quorum, fatal) → `CommitError::Io`.

use openraft::error::RaftError;
use tokio::runtime::Handle;

use valori_consensus::types::{Raft, CURRENT_SCHEMA_VERSION};
use valori_consensus::{ClientRequest, ValoriStateMachine};
use valori_kernel::event::KernelEvent;

use crate::commit::{CommitError, CommitReceipt, Committer};

pub struct RaftCommitter {
    raft: Raft,
    sm: ValoriStateMachine,
    handle: Handle,
}

impl RaftCommitter {
    pub fn new(raft: Raft, sm: ValoriStateMachine, handle: Handle) -> Self {
        Self { raft, sm, handle }
    }

    /// The state machine handle — the HTTP layer serves reads from it.
    pub fn state_machine(&self) -> &ValoriStateMachine {
        &self.sm
    }

    /// The raw Raft handle (cluster management API, Phase 2.6).
    pub fn raft(&self) -> &Raft {
        &self.raft
    }

    fn block_on<F: std::future::Future>(&self, fut: F) -> F::Output {
        match Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(|| self.handle.block_on(fut)),
            Err(_) => self.handle.block_on(fut),
        }
    }

    fn write(&self, event: KernelEvent) -> Result<CommitReceipt, CommitError> {
        let request = ClientRequest {
            event,
            request_id: None,
            schema_version: CURRENT_SCHEMA_VERSION,
        namespace_id: 0,
        };

        let result = self.block_on(self.raft.client_write(request));

        match result {
            Ok(resp) => {
                if let Some(reason) = resp.data.rejected {
                    return Err(CommitError::Rejected(reason));
                }
                Ok(CommitReceipt {
                    log_index: resp.data.log_index,
                })
            }
            Err(RaftError::APIError(e)) => match e {
                openraft::error::ClientWriteError::ForwardToLeader(forward) => {
                    Err(CommitError::NotLeader {
                        leader_api_addr: forward.leader_node.as_ref().map(|n| n.api_addr.clone()),
                    })
                }
                other => Err(CommitError::Io(format!("raft write failed: {other}"))),
            },
            Err(RaftError::Fatal(e)) => Err(CommitError::Io(format!("raft fatal: {e}"))),
        }
    }
}

impl Committer for RaftCommitter {
    fn commit(&mut self, event: KernelEvent) -> Result<CommitReceipt, CommitError> {
        self.write(event)
    }

    fn log_height(&self) -> u64 {
        self.raft
            .metrics()
            .borrow()
            .last_applied
            .map_or(0, |l| l.index)
    }

    fn flush(&mut self) -> Result<(), CommitError> {
        // The audit sink fsyncs at every apply; the Raft log is in-memory
        // until Phase 2.10. Nothing buffered here.
        Ok(())
    }
}
