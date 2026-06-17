// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
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
        let shadow = live.clone();
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
/// Default rotation threshold: 256 MiB.
pub const DEFAULT_LOG_ROTATION_BYTES: u64 = 256 * 1024 * 1024;

pub struct EventCommitter {
    /// Event log writer (durable storage)
    event_log: EventLogWriter,

    /// Event journal (runtime state)
    journal: EventJournal,

    /// Live kernel state
    live_state: KernelState,

    /// Rotate the log when it exceeds this many bytes. None disables auto-rotation.
    log_rotation_bytes: Option<u64>,
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
            log_rotation_bytes: Some(DEFAULT_LOG_ROTATION_BYTES),
        }
    }

    pub fn with_rotation_bytes(mut self, limit: Option<u64>) -> Self {
        self.log_rotation_bytes = limit;
        self
    }

    /// Commit an event (the ONLY way to mutate state)
    pub fn commit_event(&mut self, event: KernelEvent) -> Result<CommitResult> {
        // Step 1: Persist to disk FIRST (crash safety)
        let entry = crate::events::event_log::LogEntry::Event(event.clone());
        self.event_log.append(&entry)?;
        // Step 2: Add to journal buffer
        self.journal.append_buffered(event.clone());

        // Step 3 & 4: Execute on live state
        match self.live_state.apply_event(&event) {
            Ok(_) => {
                // Success: Commit the buffer
                self.journal.commit_buffer();
                tracing::debug!("Event committed: {:?}", event.event_type());
                self.maybe_rotate();
                Ok(CommitResult::Committed)
            }
            Err(e) => {
                // Failure: rollback the buffer, state is unmodified
                tracing::warn!("Apply failed: {:?}. Rolling back buffer.", e);
                self.journal.rollback_buffer();
                Err(CommitError::LiveApply(e))
            }
        }
    }

    /// Explicitly flush the event log buffer to disk
    pub fn flush_log(&mut self) -> Result<()> {
        self.event_log.flush()?;
        Ok(())
    }

    /// Rotate the log if it has exceeded the configured byte limit.
    fn maybe_rotate(&mut self) {
        let limit = match self.log_rotation_bytes {
            Some(l) => l,
            None => return,
        };

        if self.event_log.bytes_written() < limit {
            return;
        }

        let height = self.journal.committed_height();
        let state_hash = {
            use valori_kernel::snapshot::blake3::hash_state_blake3;
            hash_state_blake3(&self.live_state)
        };

        // Name archives by the monotonic segment sequence: a wall-clock name
        // would collide (and silently clobber an earlier archive) when two
        // rotations land in the same second.
        let archive_path = self
            .event_log
            .path()
            .with_extension(format!("log.{:06}", self.event_log.segment_seq()));

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let checkpoint = crate::events::event_log::LogEntry::Checkpoint {
            event_count: height,
            snapshot_hash: state_hash,
            timestamp: now,
        };

        match self.event_log.rotate(archive_path, Some(checkpoint)) {
            Ok(_) => tracing::info!(
                "Event log rotated at height {} ({} bytes)",
                height,
                limit,
            ),
            Err(e) => tracing::error!("Event log rotation failed: {}", e),
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

        // Step 3 & 4: Execute on live state
        for event in &events {
            match self.live_state.apply_event(event) {
                Ok(_) => continue,
                Err(e) => {
                    tracing::warn!("Apply failed in batch: {:?}. Rolling back {} events.", e, events.len());
                    self.journal.rollback_buffer();
                    return Err(CommitError::LiveApply(e));
                }
            }
        }

        // Success: Commit the buffer
        self.journal.commit_buffer();
        tracing::debug!("Batch committed: {} events", events.len());
        self.maybe_rotate();
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
    fn test_commit_rejects_invalid_event() {
        // The simplified commit path (no shadow execution) returns Err on apply
        // failure.  Callers use `?` so the error propagates correctly.
        // The journal height stays at 1 because the second event was never committed.
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

        // Duplicate record ID — kernel rejects this.
        let event2 = KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        };

        let result = committer.commit_event(event2);
        assert!(result.is_err(), "duplicate ID must be rejected");
        // Journal height is unchanged — the failed event was rolled back.
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
