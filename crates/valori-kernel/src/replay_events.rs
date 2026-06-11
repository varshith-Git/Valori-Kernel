// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Event Replay Infrastructure
//!
//! This module provides deterministic replay from event logs.
//!
//! # Guarantees
//! - Same event log => Same final state (on any architecture)
//! - Crash-symmetric: replay(committed_events) = recovered_state
//! - No partial application: events are atomic

use crate::event::KernelEvent;
use crate::state::kernel::KernelState;
use crate::error::{Result, KernelError};
use serde::{Serialize, Deserialize};
use alloc::vec::Vec;

/// EventJournal manages committed and buffered events
///
/// # Commit Semantics
/// - Buffered events are NOT state truth
/// - Only committed events define history
/// - Commit flow: append → buffer → verify → commit → apply → hash → store
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventJournal {
    /// Committed events represent the canonical truth
    pub committed: Vec<KernelEvent>,
    
    /// Buffered events are pending commit (not yet state truth)
    pub buffer: Vec<KernelEvent>,
}

impl EventJournal {
    /// Create a new empty journal
    pub fn new() -> Self {
        Self {
            committed: Vec::new(),
            buffer: Vec::new(),
        }
    }

    /// Append an event to the buffer (not yet committed)
    pub fn append(&mut self, event: KernelEvent) {
        self.buffer.push(event);
    }

    /// Commit all buffered events
    /// 
    /// After commit, events become part of the canonical history
    pub fn commit(&mut self) {
        self.committed.append(&mut self.buffer);
        self.buffer.clear();
    }

    /// Discard all buffered events
    ///
    /// Used for crash recovery or rollback scenarios
    pub fn discard_buffer(&mut self) {
        self.buffer.clear();
    }

    /// Get total number of committed events
    pub fn committed_len(&self) -> usize {
        self.committed.len()
    }

    /// Get number of buffered (uncommitted) events
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }
}

/// Canonical Event Log File Format
///
/// This is the stable serialization contract for event logs.
/// Schema changes require version bump and migration path.
#[repr(C)]
#[derive(Serialize, Deserialize, Debug)]
pub struct EventLogFile {
    /// Protocol version (must match)
    pub version: u32,
    
    /// Dimension (must match kernel configuration)
    pub dim: u32,
    
    /// Ordered sequence of events
    pub events: Vec<KernelEvent>,
}

impl EventLogFile {
    /// Create a new event log file
    pub fn new(events: Vec<KernelEvent>, dim: u32) -> Self {
        Self {
            version: 1,
            dim,
            events,
        }
    }

    /// Validate compatibility with runtime configuration
    pub fn validate(&self, expected_dim: Option<usize>) -> Result<()> {
        if self.version != 1 {
            return Err(KernelError::InvalidInput);
        }

        if let Some(d) = expected_dim {
            if self.dim != d as u32 {
                return Err(KernelError::InvalidInput);
            }
        }

        Ok(())
    }
}

/// Replay events to reconstruct kernel state
///
/// This is the determinism contract:
/// Any machine, anywhere, must produce identical:
/// - Memory graph
/// - Hashes
/// - Search results
/// - Snapshot serialization
///
/// from the same event log.
pub fn replay_events(
    events: &[KernelEvent]
) -> Result<KernelState>
{
    let mut state = KernelState::new();

    for evt in events {
        state.apply_event(evt)?;
    }

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::id::{RecordId};
    use crate::types::vector::FxpVector;

    #[test]
    fn test_journal_commit_semantics() {
        let mut journal = EventJournal::new();

        // Append to buffer
        journal.append(KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::new_zeros(),
            metadata: None,
            tag: 0,
        });

        assert_eq!(journal.buffer_len(), 1);
        assert_eq!(journal.committed_len(), 0);

        // Commit
        journal.commit();

        assert_eq!(journal.buffer_len(), 0);
        assert_eq!(journal.committed_len(), 1);
    }

    #[test]
    fn test_journal_discard() {
        let mut journal = EventJournal::new();

        journal.append(KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::new_zeros(),
            metadata: None,
            tag: 0,
        });

        journal.discard_buffer();

        assert_eq!(journal.buffer_len(), 0);
        assert_eq!(journal.committed_len(), 0);
    }

    #[test]
    fn test_event_log_file_validation() {
        let log_file = EventLogFile::new(vec![], 16);
        
        assert!(log_file.validate(Some(16)).is_ok());
    }

    #[test]
    fn test_event_log_file_dim_mismatch() {
        let bad_log = EventLogFile {
            version: 1,
            dim: 32,
            events: vec![],
        };

        assert!(bad_log.validate(Some(16)).is_err());
    }
}
