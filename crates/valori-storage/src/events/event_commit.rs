// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Event Commit - The Safety Wall

use crate::events::event_journal::EventJournal;
use crate::events::event_log::{EventLogError, EventLogWriter};
use crate::provider::{StorageKey, StorageProvider};
use std::sync::Arc;
use thiserror::Error;
use valori_core::ShardId;
use valori_domain::ProjectId;
use valori_kernel::error::KernelError;
use valori_kernel::event::KernelEvent;
use valori_kernel::state::kernel::KernelState;

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

/// How many events to buffer before a forced fsync. Callers that need
/// immediate durability (snapshot save, clean shutdown) must call
/// `flush_pending()` explicitly. Default: 64 (one fsync per 64 inserts).
pub const DEFAULT_WRITE_BUFFER_SIZE: usize = 64;

pub struct EventCommitter {
    /// Event log writer (durable storage)
    event_log: EventLogWriter,

    /// Event journal (runtime state)
    journal: EventJournal,

    /// Live kernel state
    live_state: KernelState,

    /// Rotate the log when it exceeds this many bytes. None disables auto-rotation.
    log_rotation_bytes: Option<u64>,

    /// Pending log entries not yet fsynced to disk, each paired with the
    /// real wall-clock time it was committed at (captured in
    /// `commit_event_ns`, NOT re-derived from `Instant::now()` at flush
    /// time — a buffered entry can sit here for a while before
    /// `flush_pending` runs, and stamping it then would silently swap its
    /// true commit time for the flush time).
    write_buf: Vec<(crate::events::event_log::LogEntry, u64)>,

    /// Flush write_buf when it reaches this many entries (0 = flush every event).
    flush_every: usize,

    /// Storage provider for publishing sealed segments upon rotation
    storage_provider: Option<(Arc<dyn StorageProvider>, ProjectId, ShardId)>,
}

impl EventCommitter {
    /// Create a new event committer
    pub fn new(event_log: EventLogWriter, journal: EventJournal, live_state: KernelState) -> Self {
        Self {
            event_log,
            journal,
            live_state,
            log_rotation_bytes: Some(DEFAULT_LOG_ROTATION_BYTES),
            write_buf: Vec::with_capacity(DEFAULT_WRITE_BUFFER_SIZE),
            flush_every: DEFAULT_WRITE_BUFFER_SIZE,
            storage_provider: None,
        }
    }

    pub fn with_storage_provider(
        mut self,
        provider: Arc<dyn StorageProvider>,
        project_id: ProjectId,
        shard_id: ShardId,
    ) -> Self {
        self.storage_provider = Some((provider, project_id, shard_id));
        self
    }

    pub fn set_storage_provider(
        &mut self,
        provider: Arc<dyn StorageProvider>,
        project_id: ProjectId,
        shard_id: ShardId,
    ) {
        self.storage_provider = Some((provider, project_id, shard_id));
    }

    pub fn with_rotation_bytes(mut self, limit: Option<u64>) -> Self {
        self.log_rotation_bytes = limit;
        self
    }

    /// Set how many events to buffer before a forced fsync (0 = sync every event).
    pub fn with_flush_every(mut self, n: usize) -> Self {
        self.flush_every = if n == 0 { 1 } else { n };
        self.write_buf = Vec::with_capacity(self.flush_every);
        self
    }

    /// Flush buffered log entries to disk now (single fsync).
    /// Must be called before save_snapshot() and on clean shutdown.
    pub fn flush_pending(&mut self) -> Result<()> {
        if self.write_buf.is_empty() {
            return Ok(());
        }
        self.event_log
            .append_batch_with_timestamps(&self.write_buf)?;
        self.write_buf.clear();
        Ok(())
    }

    /// Commit an event into the default namespace (the ONLY way to mutate
    /// state). See [`Self::commit_event_ns`] for the ordering guarantees.
    pub fn commit_event(&mut self, event: KernelEvent) -> Result<CommitResult> {
        self.commit_event_ns(event, valori_kernel::types::id::DEFAULT_NS.0)
    }

    /// Commit an event scoped to `namespace_id` (Phase S15).
    ///
    /// Order: shadow-apply → persist → live-apply.
    /// This guarantees the audit log never contains a phantom event (an event
    /// that was written to disk but rejected by the state machine). If shadow
    /// apply fails, the log is untouched and we return the error cleanly.
    ///
    /// The log entry records the namespace (`LogEntry::EventNs`) so recovery
    /// can replay the event back into the same collection it was written to —
    /// `KernelEvent` itself carries no namespace, and before S15 every replay
    /// flattened all collections into the default namespace. Default-namespace
    /// commits keep writing the plain `Event` variant, so their logs stay
    /// byte-identical to pre-S15.
    pub fn commit_event_ns(
        &mut self,
        event: KernelEvent,
        namespace_id: u16,
    ) -> Result<CommitResult> {
        // Step 1: Shadow apply — validate WITHOUT mutating live state.
        // If the event is invalid (dup ID, wrong dim, etc.) we bail here,
        // before touching the audit log.
        let mut shadow = self.live_state.clone();
        shadow
            .apply_event_ns(&event, namespace_id)
            .map_err(CommitError::ShadowApply)?;

        // Step 2: Live apply — must succeed because shadow passed on an
        // identical state snapshot. Panic here is a programming error.
        self.live_state
            .apply_event_ns(&event, namespace_id)
            .expect("live apply after shadow-pass must succeed");

        // Capture the commit instant ONCE and reuse it for both the WAL
        // entry and the journal entry below, so they always agree — this is
        // the true commit time, not whenever the buffer happens to flush to
        // disk (see the doc comment on `write_buf`).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Step 3: Buffer the log entry; flush when the buffer is full.
        // State is already live in memory (auditable); disk write is deferred
        // for throughput. Callers that need immediate durability (snapshot save,
        // clean shutdown) must call flush_pending() explicitly.
        let entry = if namespace_id == valori_kernel::types::id::DEFAULT_NS.0 {
            crate::events::event_log::LogEntry::Event(event.clone())
        } else {
            crate::events::event_log::LogEntry::EventNs {
                namespace_id,
                event: event.clone(),
            }
        };
        self.write_buf.push((entry, now));
        if self.write_buf.len() >= self.flush_every {
            self.event_log
                .append_batch_with_timestamps(&self.write_buf)?;
            self.write_buf.clear();
        }

        // Step 4: Commit journal, stamped with the SAME instant as the WAL
        // entry above (not a fresh clock read — see `commit_buffer_at`).
        self.journal.append_buffered_ns(event.clone(), namespace_id);
        self.journal.commit_buffer_at(now);
        tracing::debug!("Event committed: {:?}", event.event_type());
        self.maybe_rotate();
        Ok(CommitResult::Committed)
    }

    /// Explicitly flush all buffered events to disk (fsync).
    pub fn flush_log(&mut self) -> Result<()> {
        self.flush_pending()?;
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
        let segment_seq = self.event_log.segment_seq();
        let archive_path = self
            .event_log
            .path()
            .with_extension(format!("log.{:06}", segment_seq));

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let checkpoint = crate::events::event_log::LogEntry::Checkpoint {
            event_count: height,
            snapshot_hash: state_hash,
            timestamp: now,
        };

        match self.event_log.rotate(&archive_path, Some(checkpoint)) {
            Ok(_) => {
                tracing::info!("Event log rotated at height {} ({} bytes)", height, limit);
                if let Some((ref provider, project_id, shard_id)) = self.storage_provider {
                    if let Ok(bytes) = std::fs::read(&archive_path) {
                        let key = StorageKey::WalSegment {
                            project_id,
                            shard_id,
                            segment_seq: segment_seq as u64,
                        };
                        match provider.put_immutable(&key, &bytes) {
                            Ok(_) => tracing::info!("Published sealed WAL segment {segment_seq} to StorageProvider"),
                            Err(e) => tracing::error!("Failed to publish sealed WAL segment {segment_seq} to StorageProvider: {e}"),
                        }
                    }
                }
            }
            Err(e) => tracing::error!("Event log rotation failed: {}", e),
        }
    }

    /// Batch commit multiple events into the default namespace.
    pub fn commit_batch(&mut self, events: Vec<KernelEvent>) -> Result<CommitResult> {
        self.commit_batch_ns(events, valori_kernel::types::id::DEFAULT_NS.0)
    }

    /// Batch commit multiple events scoped to `namespace_id` (Phase S15 —
    /// a batch always targets one collection, so one namespace per call).
    ///
    /// Same shadow-first guarantee as `commit_event_ns`: all events are
    /// shadow-applied on a clone of live state before any log write. If any
    /// event fails shadow apply, the log is untouched.
    pub fn commit_batch_ns(
        &mut self,
        events: Vec<KernelEvent>,
        namespace_id: u16,
    ) -> Result<CommitResult> {
        if events.is_empty() {
            return Ok(CommitResult::Committed);
        }

        // Step 1: Shadow apply the entire batch on a state clone.
        let mut shadow = self.live_state.clone();
        for event in &events {
            shadow
                .apply_event_ns(event, namespace_id)
                .map_err(CommitError::ShadowApply)?;
        }

        // Capture one commit instant for the whole batch, reused for both
        // the WAL entries and the journal entries below (same reasoning as
        // `commit_event_ns`: WAL and journal must agree exactly).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Step 2: Persist all events (batch is now known-good).
        let default_ns = valori_kernel::types::id::DEFAULT_NS.0;
        let log_entries: Vec<_> = events
            .iter()
            .map(|e| {
                let entry = if namespace_id == default_ns {
                    crate::events::event_log::LogEntry::Event(e.clone())
                } else {
                    crate::events::event_log::LogEntry::EventNs {
                        namespace_id,
                        event: e.clone(),
                    }
                };
                (entry, now)
            })
            .collect();
        self.event_log.append_batch_with_timestamps(&log_entries)?;

        // Step 3: Live apply (must succeed — shadow passed on identical state).
        for event in &events {
            self.live_state
                .apply_event_ns(event, namespace_id)
                .expect("live apply after shadow-pass must succeed");
        }

        // Step 4: Commit journal.
        for event in &events {
            self.journal.append_buffered_ns(event.clone(), namespace_id);
        }
        self.journal.commit_buffer_at(now);
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

    /// Decompose into components (for reconstruction).
    /// Flushes any buffered WAL entries before consuming self.
    pub fn into_parts(mut self) -> (EventLogWriter, EventJournal, KernelState) {
        let _ = self.flush_pending();
        // SAFETY: we are consuming self; Drop will run but flush_pending is
        // idempotent (write_buf will be empty) so no double-flush occurs.
        let mut this = std::mem::ManuallyDrop::new(self);
        unsafe {
            let log = std::ptr::read(&this.event_log);
            let jour = std::ptr::read(&this.journal);
            let state = std::ptr::read(&this.live_state);
            // Drop remaining fields that aren't returned.
            std::ptr::drop_in_place(&mut this.write_buf);
            std::ptr::drop_in_place(&mut this.storage_provider);
            (log, jour, state)
        }
    }

    /// Rotate the event log (Compaction/Checkpointing)
    pub fn rotate_log(
        &mut self,
        archive_path: impl AsRef<std::path::Path>,
        checkpoint_entry: Option<crate::events::event_log::LogEntry>,
    ) -> crate::events::event_commit::Result<()> {
        self.flush_pending()?;
        let segment_seq = self.event_log.segment_seq();
        self.event_log
            .rotate(archive_path.as_ref(), checkpoint_entry)
            .map_err(crate::events::event_commit::CommitError::EventLog)?;
        if let Some((ref provider, project_id, shard_id)) = self.storage_provider {
            if let Ok(bytes) = std::fs::read(archive_path.as_ref()) {
                let key = StorageKey::WalSegment {
                    project_id,
                    shard_id,
                    segment_seq: segment_seq as u64,
                };
                let _ = provider.put_immutable(&key, &bytes);
            }
        }
        Ok(())
    }

    /// Subscribe to live event stream
    pub fn subscribe(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::events::event_log::LogEntry> {
        self.journal.subscribe()
    }

    /// Write a checkpoint entry and align journal height
    pub fn write_checkpoint(
        &mut self,
        entry: crate::events::event_log::LogEntry,
    ) -> Result<CommitResult> {
        self.event_log.append(&entry)?;

        if let crate::events::event_log::LogEntry::Checkpoint { event_count, .. } = entry {
            self.journal.set_height(event_count);
        }

        Ok(CommitResult::Committed)
    }
}

impl Drop for EventCommitter {
    fn drop(&mut self) {
        let _ = self.flush_pending();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;

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

    /// Regression test for the "buffered entry gets stamped with flush time,
    /// not commit time" bug: `commit_event_ns` only buffers into
    /// `write_buf` (default `flush_every` = 64), so a single commit does NOT
    /// hit disk immediately. If the eventual flush re-reads the clock (the
    /// old `append_batch` behavior), the persisted timestamp reflects when
    /// the buffer happened to drain, not when the event was actually
    /// committed — exactly what a real node does on `commit_event_ns` then
    /// `flush_pending()` at graceful shutdown, possibly much later.
    #[test]
    fn buffered_commit_keeps_true_commit_time_even_when_flush_is_delayed() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        let event_log = EventLogWriter::open(&log_path, Some(16)).unwrap();
        let journal = EventJournal::new();
        let live_state = KernelState::new();
        let mut committer = EventCommitter::new(event_log, journal, live_state);

        let commit_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let event = KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        };
        committer.commit_event(event).unwrap();

        // The event is buffered, not yet flushed (flush_every defaults to
        // 64) — simulate real wall-clock time passing before the eventual
        // flush (e.g. graceful shutdown), same as a real deployment.
        std::thread::sleep(std::time::Duration::from_secs(2));
        committer.flush_pending().unwrap();

        let flush_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(
            flush_time > commit_time,
            "test setup: flush must happen strictly after commit"
        );

        // In-memory journal must reflect the true commit time.
        let journal_ts = committer.journal().event_timestamp(0).unwrap();
        assert_eq!(
            journal_ts, commit_time,
            "in-memory journal timestamp must be the commit instant, not the flush instant"
        );

        // What's actually on disk must ALSO reflect the true commit time,
        // not the later flush time — this is what a restart reads back.
        let (_state, recovered_journal, count) =
            crate::events::event_replay::recover_from_event_log(&log_path).unwrap();
        assert_eq!(count, 1);
        let recovered_ts = recovered_journal.event_timestamp(0).unwrap();
        assert_eq!(
            recovered_ts, commit_time,
            "persisted WAL timestamp must be the commit instant \
             ({commit_time}), not the later flush instant ({flush_time}) — \
             a buffered entry must not be re-stamped when it finally hits disk"
        );
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
