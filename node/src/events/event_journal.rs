// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
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
pub struct EventJournal<const D: usize> {
    /// Committed events (canonical truth)
    committed: Vec<KernelEvent<D>>,
    
    /// Buffered events (shadow execution, not yet truth)
    buffer: Vec<KernelEvent<D>>,
    
    /// Committed event count (for proof generation)
    committed_height: u64,
}

impl<const D: usize> EventJournal<D> {
    /// Create a new empty journal
    pub fn new() -> Self {
        Self {
            committed: Vec::new(),
            buffer: Vec::new(),
            committed_height: 0,
        }
    }

    /// Create a journal from committed events (recovery scenario)
    pub fn from_committed(events: Vec<KernelEvent<D>>) -> Self {
        let committed_height = events.len() as u64;
        Self {
            committed: events,
            buffer: Vec::new(),
            committed_height,
        }
    }

    /// Append an event to the buffer (not yet committed)
    ///
    /// This event is in "shadow execution" state.
    /// It will only become truth after commit_buffer()
    pub fn append_buffered(&mut self, event: KernelEvent<D>) {
        self.buffer.push(event);
    }

    /// Commit all buffered events
    ///
    /// Promotes shadow events to canonical truth.
    /// This should only be called after:
    /// - Events are durably written to disk
    /// - Shadow state validation passes
    pub fn commit_buffer(&mut self) {
        self.committed.append(&mut self.buffer);
        self.committed_height = self.committed.len() as u64;
        self.buffer.clear();
    }

    /// Rollback buffered events
    ///
    /// Discards shadow execution state.
    /// Used when:
    /// - Validation fails
    /// - Write to disk fails
    /// - Hash verification fails
    pub fn rollback_buffer(&mut self) {
        self.buffer.clear();
    }

    /// Get committed events (canonical truth)
    pub fn committed(&self) -> &[KernelEvent<D>] {
        &self.committed
    }

    /// Get buffered events (shadow execution)
    pub fn buffered(&self) -> &[KernelEvent<D>] {
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
}

impl<const D: usize> Default for EventJournal<D> {
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
        let mut journal = EventJournal::<16>::new();

        // Add to buffer
        journal.append_buffered(KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::<16>::new_zeros(),
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
        let mut journal = EventJournal::<16>::new();

        // Add to buffer
        journal.append_buffered(KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::<16>::new_zeros(),
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
                vector: FxpVector::<16>::new_zeros(),
            },
            KernelEvent::InsertRecord {
                id: RecordId(2),
                vector: FxpVector::<16>::new_zeros(),
            },
        ];

        let journal = EventJournal::from_committed(events);

        assert_eq!(journal.committed_height(), 2);
        assert_eq!(journal.buffer_size(), 0);
    }

    #[test]
    fn test_journal_crash_safety() {
        let mut journal = EventJournal::<16>::new();

        // Simulate: append to buffer
        journal.append_buffered(KernelEvent::InsertRecord {
            id: RecordId(1),
            vector: FxpVector::<16>::new_zeros(),
        });

        // Simulate: crash before commit (rollback)
        journal.rollback_buffer();

        // After crash recovery, buffer is empty
        assert_eq!(journal.buffer_size(), 0);
        assert_eq!(journal.committed_height(), 0);
    }
}
