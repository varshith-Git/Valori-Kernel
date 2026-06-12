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

use valori_consensus::AuditSink;
use valori_kernel::event::KernelEvent;

use crate::events::event_log::{EventLogWriter, LogEntry};

/// Adapts the chained event log to the consensus crate's audit seam.
///
/// Cloneable: the state machine and the node's snapshot/rotation machinery
/// may both need a handle; writes are serialized by the inner mutex (a
/// std mutex — calls are short, synchronous fsync-backed appends).
#[derive(Clone)]
pub struct EventLogAuditSink {
    writer: Arc<Mutex<EventLogWriter>>,
}

impl EventLogAuditSink {
    pub fn new(writer: EventLogWriter) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
        }
    }

    /// Shared handle to the underlying writer (rotation, metrics).
    pub fn writer(&self) -> Arc<Mutex<EventLogWriter>> {
        Arc::clone(&self.writer)
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
            .map_err(|e| std::io::Error::other(format!("audit append failed: {e}")))
    }
}
