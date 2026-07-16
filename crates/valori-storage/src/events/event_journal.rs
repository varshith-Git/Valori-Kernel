// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Event Journal - Runtime State Management
//!
//! Maintains the distinction between:
//! - **committed** = canonical truth (replayed state)
//! - **buffer** = shadow execution (pending commit)
//!
//! # Semantics
//! - buffer ≠ truth
//! - Only commit() promotes events to truth
//! - Crash during buffer → discard buffer, replay committed
//!
//! # Flow
//! 1. append_buffered() - add to shadow
//! 2. shadow_apply() - test execution
//! 3. commit_buffer() - promote to truth
//! 4. rollback_buffer() - discard on failure

use valori_kernel::event::KernelEvent;

/// Event Journal manages runtime event state
///
/// This is the in-memory representation of the event log state.
/// It enforces the buffer/committed distinction for crash safety.
#[derive(Clone, Debug)]
pub struct EventJournal {
    /// Committed events (canonical truth)
    committed: Vec<KernelEvent>,

    /// Unix-second wall-clock timestamp for each committed event (parallel to `committed`).
    /// Stamped at `commit_buffer()` time; used by as-of / point-in-time search.
    timestamps: Vec<u64>,

    /// Buffered events (shadow execution, not yet truth)
    buffer: Vec<KernelEvent>,

    /// Committed event count (for proof generation)
    committed_height: u64,

    /// Live event broadcast channel
    /// Capacity should be large enough to handle bursts
    tx: tokio::sync::broadcast::Sender<crate::events::event_log::LogEntry>,
}

impl EventJournal {
    /// Create a new empty journal
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(10000);
        Self {
            committed: Vec::new(),
            timestamps: Vec::new(),
            buffer: Vec::new(),
            committed_height: 0,
            tx,
        }
    }

    /// Create a new empty journal starting at a specific height (e.g. after snapshot)
    pub fn new_at_height(height: u64) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(10000);
        Self {
            committed: Vec::new(),
            timestamps: Vec::new(),
            buffer: Vec::new(),
            committed_height: height,
            tx,
        }
    }

    /// Create a journal from committed events (recovery scenario).
    /// Timestamps are set to 0 for recovered events (no original wall-clock available).
    pub fn from_committed(events: Vec<KernelEvent>) -> Self {
        let committed_height = events.len() as u64;
        let timestamps = vec![0u64; events.len()];
        let (tx, _) = tokio::sync::broadcast::channel(10000);
        Self {
            committed: events,
            timestamps,
            buffer: Vec::new(),
            committed_height,
            tx,
        }
    }

    pub fn set_height(&mut self, height: u64) {
        self.committed_height = height;
    }

    /// Append an event to the buffer (not yet committed)
    pub fn append_buffered(&mut self, event: KernelEvent) {
        self.buffer.push(event);
    }

    /// Commit all buffered events. Each event is stamped with the current wall-clock time.
    pub fn commit_buffer(&mut self) {
        use crate::events::event_log::LogEntry;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for event in &self.buffer {
            let _ = self.tx.send(LogEntry::Event(event.clone()));
            self.timestamps.push(now);
        }

        self.committed.append(&mut self.buffer);
        self.committed_height = self.committed.len() as u64;
        self.buffer.clear();
    }

    /// Rollback buffered events
    pub fn rollback_buffer(&mut self) {
        self.buffer.clear();
    }

    /// Get committed events (canonical truth)
    pub fn committed(&self) -> &[KernelEvent] {
        &self.committed
    }

    /// Get buffered events (shadow execution)
    pub fn buffered(&self) -> &[KernelEvent] {
        &self.buffer
    }

    /// Get committed event count
    pub fn committed_height(&self) -> u64 {
        self.committed_height
    }

    /// Get buffer size
    pub fn buffer_size(&self) -> usize {
        self.buffer.len()
    }

    /// Check if buffer is empty
    pub fn has_pending_buffer(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// Iterate committed events paired with their unix-second wall-clock timestamps.
    pub fn committed_with_timestamps(&self) -> impl Iterator<Item = (&KernelEvent, u64)> {
        self.committed.iter().zip(self.timestamps.iter().copied())
    }

    /// Wall-clock timestamp (unix seconds) for the event at `log_index`.
    pub fn event_timestamp(&self, log_index: usize) -> Option<u64> {
        self.timestamps.get(log_index).copied()
    }

    /// Find the last log index whose timestamp is ≤ `unix_secs`.
    /// Returns `None` when there are no events or all events are newer than the target.
    pub fn find_log_index_at_or_before(&self, unix_secs: u64) -> Option<usize> {
        let pos = self.timestamps.partition_point(|&t| t <= unix_secs);
        if pos == 0 {
            None
        } else {
            Some(pos - 1)
        }
    }

    /// Subscribe to live event stream
    pub fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::events::event_log::LogEntry> {
        self.tx.subscribe()
    }
}

impl Default for EventJournal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;

    #[test]
    fn test_journal_buffer_commit() {
        let mut journal = EventJournal::new();

        // Add to buffer
        journal.append_buffered(KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        });

        assert_eq!(journal.buffer_size(), 1);
        assert_eq!(journal.committed_height(), 0);

        // Commit
        journal.commit_buffer();

        assert_eq!(journal.buffer_size(), 0);
        assert_eq!(journal.committed_height(), 1);
        assert_eq!(journal.committed().len(), 1);
    }

    #[test]
    fn test_journal_buffer_rollback() {
        let mut journal = EventJournal::new();

        // Add to buffer
        journal.append_buffered(KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        });

        assert_eq!(journal.buffer_size(), 1);

        // Rollback
        journal.rollback_buffer();

        assert_eq!(journal.buffer_size(), 0);
        assert_eq!(journal.committed_height(), 0);
    }

    #[test]
    fn test_journal_from_committed() {
        let events = vec![
            KernelEvent::InsertRecord {
                id: RecordId(1),
                vector: FxpVector::new_zeros(16),
                metadata: None,
                tag: 0,
            },
            KernelEvent::InsertRecord {
                id: RecordId(2),
                vector: FxpVector::new_zeros(16),
                metadata: None,
                tag: 0,
            },
        ];

        let journal = EventJournal::from_committed(events);

        assert_eq!(journal.committed_height(), 2);
        assert_eq!(journal.buffer_size(), 0);
    }

    #[test]
    fn test_journal_crash_safety() {
        let mut journal = EventJournal::new();

        // Simulate: append to buffer
        journal.append_buffered(KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        });

        // Simulate: crash before commit (rollback)
        journal.rollback_buffer();

        // After crash recovery, buffer is empty
        assert_eq!(journal.buffer_size(), 0);
        assert_eq!(journal.committed_height(), 0);
    }
}
