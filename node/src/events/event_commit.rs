// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Event Commit - The Safety Wall

use valori_kernel::state::kernel::KernelState;
use valori_kernel::event::KernelEvent;
use valori_kernel::error::KernelError;
use crate::events::event_log::{EventLogWriter, EventLogError};
use crate::events::event_journal::EventJournal;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommitError {
    #[error("Event log error: {0}")]
    EventLog(#[from] EventLogError),
    
    #[error("Kernel error during shadow apply: {0:?}")]
    ShadowApply(KernelError),
    
    #[error("Kernel error during live apply: {0:?}")]
    LiveApply(KernelError),
    
    #[error("State verification failed")]
    VerificationFailed,
}

pub type Result<T> = std::result::Result<T, CommitError>;

/// Result of a commit operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitResult {
    /// Event committed successfully
    Committed,
    
    /// Event rolled back (failed before commit boundary)
    RolledBack,
}

/// Shadow execution context for safe event application
pub struct ShadowExecutor {
    /// Shadow kernel (test execution environment)
    shadow: KernelState,
}

impl ShadowExecutor {
    /// Create a new shadow executor from current live state
    pub fn from_state(live: &KernelState) -> std::result::Result<Self, CommitError> {
        use valori_kernel::snapshot::encode::encode_state;
        use valori_kernel::snapshot::decode::decode_state;

        // Allocate snapshot buffer on HEAP (not stack)
        let mut buffer = vec![0u8; 10 * 1024 * 1024]; // 10MB heap-allocated
        
        let len = encode_state(live, &mut buffer)
            .map_err(|e| CommitError::ShadowApply(e))?;
        buffer.truncate(len);

        // Decode into shadow
        let shadow = decode_state(&buffer)
            .map_err(|e| CommitError::ShadowApply(e))?;

        Ok(Self { shadow })
    }

    /// Apply an event to the shadow kernel
    pub fn shadow_apply(&mut self, event: &KernelEvent) -> std::result::Result<(), KernelError> {
        self.shadow.apply_event(event)
    }

    /// Get reference to shadow state (for verification)
    pub fn shadow_state(&self) -> &KernelState {
        &self.shadow
    }

    /// Consume shadow and return the state (after commit)
    pub fn into_state(self) -> KernelState {
        self.shadow
    }
}

/// Event committer - enforces the commit barrier
pub struct EventCommitter {
    /// Event log writer (durable storage)
    event_log: EventLogWriter,
    
    /// Event journal (runtime state)
    journal: EventJournal,
    
    /// Live kernel state
    live_state: KernelState,
}

impl EventCommitter {
    /// Create a new event committer
    pub fn new(
        event_log: EventLogWriter,
        journal: EventJournal,
        live_state: KernelState,
    ) -> Self {
        Self {
            event_log,
            journal,
            live_state,
        }
    }

    /// Commit an event (the ONLY way to mutate state)
    pub fn commit_event(&mut self, event: KernelEvent) -> Result<CommitResult> {
        // Step 1: Persist to disk FIRST (crash safety)
        let entry = crate::events::event_log::LogEntry::Event(event.clone());
        self.event_log.append(&entry)?;

        // Step 2: Add to journal buffer (shadow execution space)
        self.journal.append_buffered(event.clone());

        // Step 3: Shadow execution (test the event)
        let mut shadow = ShadowExecutor::from_state(&self.live_state)?;
        
        match shadow.shadow_apply(&event) {
            Ok(_) => {
                // Shadow apply succeeded
            }
            Err(e) => {
                // Shadow apply failed → safe rollback
                tracing::warn!("Shadow apply failed: {:?}. Rolling back buffer.", e);
                self.journal.rollback_buffer();
                return Ok(CommitResult::RolledBack);
            }
        }

        // Step 4: COMMIT BOUNDARY
        self.journal.commit_buffer();

        // Step 5: Apply to live state
        match self.live_state.apply_event(&event) {
            Ok(_) => {
                tracing::debug!("Event committed: {:?}", event.event_type());
                Ok(CommitResult::Committed)
            }
            Err(e) => {
                tracing::error!(
                    "CRITICAL: Live apply failed after shadow success: {:?}",
                    e
                );
                Err(CommitError::LiveApply(e))
            }
        }
    }

    /// Batch commit multiple events
    pub fn commit_batch(&mut self, events: Vec<KernelEvent>) -> Result<CommitResult> {
        if events.is_empty() {
            return Ok(CommitResult::Committed);
        }

        // Step 1: Persist ALL events to disk first
        let log_entries: Vec<_> = events.iter()
            .map(|e| crate::events::event_log::LogEntry::Event(e.clone()))
            .collect();
            
        self.event_log.append_batch(&log_entries)?;

        // Step 2: Add all to buffer
        for event in &events {
            self.journal.append_buffered(event.clone());
        }

        // Step 3: Shadow apply ALL events
        let mut shadow = ShadowExecutor::from_state(&self.live_state)?;
        
        for event in &events {
            match shadow.shadow_apply(event) {
                Ok(_) => continue,
                Err(e) => {
                    tracing::warn!(
                        "Shadow apply failed in batch: {:?}. Rolling back {} events.",
                        e,
                        events.len()
                    );
                    self.journal.rollback_buffer();
                    return Ok(CommitResult::RolledBack);
                }
            }
        }

        // Step 4: COMMIT BOUNDARY
        self.journal.commit_buffer();

        // Step 5: Apply all to live state
        for event in &events {
            self.live_state.apply_event(event)
                .map_err(CommitError::LiveApply)?;
        }

        tracing::debug!("Batch committed: {} events", events.len());
        Ok(CommitResult::Committed)
    }

    /// Get reference to live state
    pub fn live_state(&self) -> &KernelState {
        &self.live_state
    }

    /// Get mutable reference to live state (use sparingly)
    pub fn live_state_mut(&mut self) -> &mut KernelState {
        &mut self.live_state
    }

    /// Get reference to journal
    pub fn journal(&self) -> &EventJournal {
        &self.journal
    }

    /// Get reference to event log
    pub fn event_log(&self) -> &EventLogWriter {
        &self.event_log
    }

    /// Decompose into components (for reconstruction)
    pub fn into_parts(self) -> (EventLogWriter, EventJournal, KernelState) {
        (self.event_log, self.journal, self.live_state)
    }

    /// Rotate the event log (Compaction/Checkpointing)
    pub fn rotate_log(
        &mut self,
        archive_path: impl AsRef<std::path::Path>,
        checkpoint_entry: Option<crate::events::event_log::LogEntry>
    ) -> crate::events::event_commit::Result<()> {
        self.event_log.rotate(archive_path, checkpoint_entry)
            .map_err(crate::events::event_commit::CommitError::EventLog)
    }

    /// Subscribe to live event stream
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<crate::events::event_log::LogEntry> {
        self.journal.subscribe()
    }

    /// Write a checkpoint entry and align journal height
    pub fn write_checkpoint(
        &mut self, 
        entry: crate::events::event_log::LogEntry
    ) -> Result<CommitResult> {
        self.event_log.append(&entry)?;
        
        if let crate::events::event_log::LogEntry::Checkpoint { event_count, .. } = entry {
            self.journal.set_height(event_count);
        }
        
        Ok(CommitResult::Committed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use tempfile::tempdir;

    #[test]
    fn test_commit_success() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        let event_log = EventLogWriter::open(&log_path, Some(16)).unwrap();
        let journal = EventJournal::new();
        let live_state = KernelState::new();

        let mut committer = EventCommitter::new(event_log, journal, live_state);

        let event = KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        };

        let result = committer.commit_event(event).unwrap();
        assert_eq!(result, CommitResult::Committed);

        assert!(committer.live_state().get_record(RecordId(0)).is_some());
        assert_eq!(committer.journal().committed_height(), 1);
    }

    #[test]
    fn test_commit_rollback_on_error() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        let event_log = EventLogWriter::open(&log_path, Some(16)).unwrap();
        let journal = EventJournal::new();
        let live_state = KernelState::new();

        let mut committer = EventCommitter::new(event_log, journal, live_state);

        let event1 = KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        };
        committer.commit_event(event1).unwrap();

        let event2 = KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        };
        
        let result = committer.commit_event(event2).unwrap();
        assert_eq!(result, CommitResult::RolledBack);
        assert_eq!(committer.journal().committed_height(), 1);
    }

    #[test]
    fn test_batch_commit() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        let event_log = EventLogWriter::open(&log_path, Some(16)).unwrap();
        let journal = EventJournal::new();
        let live_state = KernelState::new();

        let mut committer = EventCommitter::new(event_log, journal, live_state);

        let events = vec![
            KernelEvent::InsertRecord {
                id: RecordId(0),
                vector: FxpVector::new_zeros(16),
                metadata: None,
                tag: 0,
            },
            KernelEvent::InsertRecord {
                id: RecordId(1),
                vector: FxpVector::new_zeros(16),
                metadata: None,
                tag: 0,
            },
        ];

        let result = committer.commit_batch(events).unwrap();
        assert_eq!(result, CommitResult::Committed);
        assert_eq!(committer.journal().committed_height(), 2);
    }
}
