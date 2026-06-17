// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The audit-at-apply bridge — Phase 2.5.
//!
//! `EventLogAuditSink` implements valori-consensus's [`AuditSink`] over the
//! BLAKE3-chained [`EventLogWriter`]. In cluster mode this is THE audit-log
//! write point: the state machine calls it once per event, at apply time,
//! strictly after quorum commit and a successful kernel apply — so the
//! chained `events.log` records exactly the quorum-committed event stream,
//! in apply order, with each event's idempotency token.
//!
//! The chain semantics are identical to standalone mode: same v3 segment
//! format, same splice-on-rotation, same `valori-verify` workflow. An
//! auditor cannot tell (and should not care) whether a log was written by
//! one node or a cluster.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use valori_consensus::AuditSink;
use valori_kernel::event::KernelEvent;

use crate::events::event_commit::DEFAULT_LOG_ROTATION_BYTES;
use crate::events::event_log::{EventLogWriter, LogEntry};

/// Adapts the chained event log to the consensus crate's audit seam.
///
/// Cloneable: the state machine and the node's snapshot/rotation machinery
/// may both need a handle; writes are serialized by the inner mutex (a
/// std mutex — calls are short, synchronous fsync-backed appends).
#[derive(Clone)]
pub struct EventLogAuditSink {
    writer: Arc<Mutex<EventLogWriter>>,
    /// Seal the live segment once it passes this size. `None` disables it.
    /// Safe in cluster mode: the audit log is forensic — state recovery is via
    /// the Raft snapshot, never an events.log replay — so sealed segments can
    /// be archived without a paired state snapshot.
    rotation_bytes: Option<u64>,
}

impl EventLogAuditSink {
    pub fn new(writer: EventLogWriter) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
            rotation_bytes: Some(DEFAULT_LOG_ROTATION_BYTES),
        }
    }

    /// Override the rotation threshold (`None` disables auto-rotation).
    pub fn with_rotation_bytes(mut self, limit: Option<u64>) -> Self {
        self.rotation_bytes = limit;
        self
    }

    /// Shared handle to the underlying writer (rotation, metrics).
    pub fn writer(&self) -> Arc<Mutex<EventLogWriter>> {
        Arc::clone(&self.writer)
    }
}

/// Seal the live segment to `events.log.<unix_ts>` and open a fresh one when it
/// exceeds `limit`. The checkpoint records the writer's chain head as the seal
/// marker; the new segment splices from it so the chain stays continuous.
fn maybe_rotate(writer: &mut EventLogWriter, limit: Option<u64>) {
    let limit = match limit {
        Some(l) if writer.bytes_written() >= l => l,
        _ => return,
    };

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    // Name archives by the monotonic segment sequence, never the wall clock:
    // two rotations in the same second would collide on a timestamp name and
    // `rename` would clobber the earlier archive (silent history loss).
    let archive_path = writer.path().with_extension(format!("log.{:06}", writer.segment_seq()));
    let checkpoint = LogEntry::Checkpoint {
        event_count: writer.event_count(),
        snapshot_hash: *writer.chain_head(),
        timestamp: now,
    };

    match writer.rotate(&archive_path, Some(checkpoint)) {
        Ok(_) => tracing::info!(
            "Audit log rotated at {} events (>{} bytes) → {:?}",
            writer.event_count(),
            limit,
            archive_path,
        ),
        Err(e) => tracing::error!("Audit log rotation failed: {e}"),
    }
}

impl AuditSink for EventLogAuditSink {
    fn record(
        &mut self,
        event: &KernelEvent,
        request_id: Option<[u8; 16]>,
    ) -> Result<(), std::io::Error> {
        let mut writer = self.writer.lock().expect("audit log mutex poisoned");
        writer
            .append_with_request_id(&LogEntry::Event(event.clone()), request_id)
            .map_err(|e| std::io::Error::other(format!("audit append failed: {e}")))?;
        maybe_rotate(&mut writer, self.rotation_bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::event_replay::recover_from_event_log;
    use tempfile::tempdir;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;

    fn ev(i: u32) -> KernelEvent {
        KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        }
    }

    #[test]
    fn audit_sink_rotates_and_history_recovers_across_segments() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");

        let writer = EventLogWriter::open(&path, Some(16)).unwrap();
        // Tiny threshold so a handful of events trips at least one rotation.
        let mut sink = EventLogAuditSink::new(writer).with_rotation_bytes(Some(64));

        for i in 0..12u32 {
            sink.record(&ev(i), None).unwrap();
        }
        drop(sink);

        // At least one sealed archive segment exists alongside the live file.
        let archives: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("events.log.") && n != "events.log")
                    .unwrap_or(false)
            })
            .collect();
        assert!(!archives.is_empty(), "audit sink should have rotated at least once");

        // Every recorded event is still recoverable across the spliced segments.
        let (state, _journal, count) = recover_from_event_log(&path).unwrap();
        assert_eq!(count, 12, "all audited events must survive rotation");
        for i in 0..12 {
            assert!(state.get_record(RecordId(i)).is_some(), "record {i} lost across audit rotation");
        }
    }
}
