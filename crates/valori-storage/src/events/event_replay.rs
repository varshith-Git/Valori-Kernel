// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Event Replay - Authoritative Recovery
//!
//! This module enforces the recovery contract:
//! **Event Log ALWAYS wins. Snapshot is just a cache.**

use crate::events::event_journal::EventJournal;
use crate::provider::{ListPrefix, StorageKey, StorageProvider};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use thiserror::Error;
use valori_core::ShardId;
use valori_domain::ProjectId;
use valori_kernel::error::KernelError;
use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::state::kernel::KernelState;

#[derive(Error, Debug)]
pub enum ReplayError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Storage provider error: {0}")]
    Storage(#[from] crate::provider::StorageError),

    #[error("Event log header invalid")]
    InvalidHeader,

    #[error("Dimension mismatch: log has {log_dim}, expected {expected_dim}")]
    DimensionMismatch { log_dim: u32, expected_dim: u32 },

    #[error("Event deserialization failed: {0}")]
    Deserialization(String),

    #[error("Event application failed: {0:?}")]
    EventApplication(KernelError),

    #[error("Event log corrupted at offset {offset}")]
    Corrupted { offset: usize },
}

pub type Result<T> = std::result::Result<T, ReplayError>;

/// Replay events into a fresh kernel state, each into its recorded
/// namespace (S15 — pre-S15 `Event` entries carry namespace 0, so old logs
/// replay exactly as they always did).
pub fn replay_events(events: &[(u16, KernelEvent)]) -> Result<KernelState> {
    let mut state = KernelState::new();

    for (idx, (namespace_id, event)) in events.iter().enumerate() {
        state.apply_event_ns(event, *namespace_id).map_err(|e| {
            tracing::error!("Event replay failed at index {}: {:?}", idx, e);
            ReplayError::EventApplication(e)
        })?;
    }

    Ok(state)
}

/// One segment's replay result: its sequence number, the events it carries
/// (with each event's namespace, S15, and the real persisted wall-clock
/// timestamp it was committed with — decoded straight from the WAL frame,
/// not reconstructed), the chain head it splices FROM (header), and the
/// chain head it closes WITH.
pub struct SegmentReplay {
    pub segment_seq: u32,
    pub prev_segment_chain_head: [u8; 32],
    pub final_chain_head: [u8; 32],
    pub events: Vec<(u16, KernelEvent, u64)>,
}

/// Read one segment from memory buffer, validating its internal hash chain.
pub fn read_segment_bytes(buffer: &[u8], expected_dim: Option<u32>) -> Result<SegmentReplay> {
    let header = valori_wire::parse_header(buffer).map_err(|_| ReplayError::InvalidHeader)?;
    if let Some(expected) = expected_dim {
        if expected != 0 && header.dim != 0 && header.dim != expected {
            return Err(ReplayError::DimensionMismatch {
                log_dim: header.dim,
                expected_dim: expected,
            });
        }
    }

    use crate::events::event_log::{walk_segment_body, LogEntry, SegmentWalkError};

    let (decoded_entries, chain_head) = walk_segment_body(
        header.version,
        buffer,
        header.header_len,
        header.prev_segment_chain_head,
    )
    .map_err(|e| match e {
        SegmentWalkError::ChainBroken { offset } => ReplayError::Corrupted { offset },
        SegmentWalkError::Wire { offset, .. } => ReplayError::Corrupted { offset },
    })?;

    let mut events = Vec::new();
    for decoded in decoded_entries {
        let ts = decoded.wall_time_secs;
        match decoded.entry {
            LogEntry::Event(event) => {
                events.push((valori_kernel::types::id::DEFAULT_NS.0, event, ts));
            }
            LogEntry::EventNs {
                namespace_id,
                event,
            } => {
                events.push((namespace_id, event, ts));
            }
            _ => {}
        }
    }

    Ok(SegmentReplay {
        segment_seq: header.segment_seq,
        prev_segment_chain_head: header.prev_segment_chain_head,
        final_chain_head: chain_head,
        events,
    })
}

/// Read one segment file, validating its internal hash chain, and report the
/// splice endpoints so multi-segment recovery can verify continuity.
fn read_segment_full(path: impl AsRef<Path>, expected_dim: Option<u32>) -> Result<SegmentReplay> {
    let mut buffer = Vec::new();
    BufReader::new(File::open(path.as_ref())?).read_to_end(&mut buffer)?;
    read_segment_bytes(&buffer, expected_dim)
}

/// Discover and read every local segment for `live_path` in order, keeping
/// each event's real persisted commit timestamp (see `SegmentReplay`).
fn read_all_segments_with_timestamps(
    live_path: impl AsRef<Path>,
    expected_dim: Option<u32>,
) -> Result<Vec<(u16, KernelEvent, u64)>> {
    let live_path = live_path.as_ref();

    // The live file plus any `events.log.<suffix>` archives in the same dir.
    let mut paths = vec![live_path.to_path_buf()];
    if let (Some(dir), Some(fname)) = (
        live_path.parent(),
        live_path.file_name().and_then(|n| n.to_str()),
    ) {
        let prefix = format!("{fname}.");
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with(&prefix) {
                        paths.push(entry.path());
                    }
                }
            }
        }
    }

    let mut segments: Vec<SegmentReplay> = paths
        .iter()
        .map(|p| read_segment_full(p, expected_dim))
        .collect::<Result<_>>()?;
    segments.sort_by_key(|s| s.segment_seq);

    let mut all = Vec::new();
    let mut prev_close: Option<[u8; 32]> = None;
    for seg in segments {
        if let Some(prev) = prev_close {
            if seg.prev_segment_chain_head != prev {
                return Err(ReplayError::Corrupted { offset: 0 });
            }
        }
        prev_close = Some(seg.final_chain_head);
        all.extend(seg.events);
    }
    Ok(all)
}

/// Discover and replay every local segment for `live_path` in order.
pub fn read_all_segments(
    live_path: impl AsRef<Path>,
    expected_dim: Option<u32>,
) -> Result<Vec<(u16, KernelEvent)>> {
    Ok(read_all_segments_with_timestamps(live_path, expected_dim)?
        .into_iter()
        .map(|(ns, event, _ts)| (ns, event))
        .collect())
}

/// Read every event strictly AFTER `after_lsn` in the shard-wide
/// authoritative order.
pub fn read_events_after_lsn(
    log_path: impl AsRef<Path>,
    expected_dim: Option<u32>,
    after_lsn: u64,
) -> Result<Vec<(u16, KernelEvent)>> {
    let all = read_all_segments(log_path, expected_dim)?;
    let start = (after_lsn as usize).min(all.len());
    Ok(all[start..].to_vec())
}

/// Stream events from a logical StorageProvider across sealed WAL segments and an optional active segment.
/// Replaces raw filesystem discovery and streams only the requested LSN tail.
pub fn stream_events_from_provider(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
    shard_id: ShardId,
    expected_dim: Option<u32>,
    after_lsn: u64,
    active_segment_path: Option<&Path>,
) -> Result<Vec<(u16, KernelEvent)>> {
    let mut sealed_keys = provider.list(&ListPrefix::WalSegments {
        project_id,
        shard_id,
    })?;

    // Sort sealed segment keys by segment_seq
    sealed_keys.sort_by_key(|k| match k {
        StorageKey::WalSegment { segment_seq, .. } => *segment_seq,
        _ => 0,
    });

    let mut segments = Vec::new();
    for key in &sealed_keys {
        let bytes = provider.get(key)?;
        let seg = read_segment_bytes(&bytes, expected_dim)?;
        segments.push(seg);
    }

    if let Some(active_path) = active_segment_path {
        if active_path.exists() {
            if let Ok(metadata) = std::fs::metadata(active_path) {
                if metadata.len() >= 16 {
                    let seg = read_segment_full(active_path, expected_dim)?;
                    // Only add if not already in sealed segments
                    if !segments.iter().any(|s| s.segment_seq == seg.segment_seq) {
                        segments.push(seg);
                    }
                }
            }
        }
    }

    segments.sort_by_key(|s| s.segment_seq);

    let mut tail = Vec::new();
    let mut prev_close: Option<[u8; 32]> = None;
    let mut current_lsn = 0u64;

    for seg in segments {
        if let Some(prev) = prev_close {
            if seg.prev_segment_chain_head != prev {
                return Err(ReplayError::Corrupted { offset: 0 });
            }
        }
        prev_close = Some(seg.final_chain_head);

        for (ns, event, _ts) in seg.events {
            current_lsn += 1;
            if current_lsn > after_lsn {
                tail.push((ns, event));
            }
        }
    }

    Ok(tail)
}

/// Full recovery from the event log — replays every local segment (sealed
/// archives + the live file) so a rotated log recovers losslessly.
pub fn recover_from_event_log(
    log_path: impl AsRef<Path>,
) -> Result<(KernelState, EventJournal, u64)> {
    tracing::info!("Starting recovery from event log: {:?}", log_path.as_ref());

    let events_with_ts = read_all_segments_with_timestamps(log_path, None)?;
    let event_count = events_with_ts.len() as u64;

    tracing::info!("Loaded {} events across all segments", event_count);

    let mut events: Vec<(u16, KernelEvent)> = Vec::with_capacity(events_with_ts.len());
    let mut namespaces: Vec<u16> = Vec::with_capacity(events_with_ts.len());
    let mut timestamps: Vec<u64> = Vec::with_capacity(events_with_ts.len());
    for (ns, event, ts) in events_with_ts {
        events.push((ns, event.clone()));
        namespaces.push(ns);
        timestamps.push(ts);
    }

    let state = replay_events(&events)?;
    let journal = EventJournal::from_committed_with_namespaces_and_timestamps(
        events.into_iter().map(|(_, e)| e).collect(),
        namespaces,
        timestamps,
    );

    Ok((state, journal, event_count))
}

/// Verify snapshot against replayed state
pub fn verify_snapshot_consistency(
    snapshot_state: &KernelState,
    replayed_state: &KernelState,
) -> bool {
    let snapshot_hash = hash_state_blake3(snapshot_state);
    let replayed_hash = hash_state_blake3(replayed_state);

    let matches = snapshot_hash == replayed_hash;

    if !matches {
        tracing::warn!("Snapshot hash mismatch detected!");
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::event_log::EventLogWriter;
    use tempfile::tempdir;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;

    #[test]
    fn test_replay_from_log() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        {
            let mut writer = EventLogWriter::open(&log_path, Some(16)).unwrap();
            for i in 0..5 {
                let event = KernelEvent::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::new_zeros(16),
                    metadata: None,
                    tag: 0,
                };
                writer
                    .append(&crate::events::event_log::LogEntry::Event(event))
                    .unwrap();
            }
        }

        let (state, journal, count) = recover_from_event_log(&log_path).unwrap();

        assert_eq!(count, 5);
        assert_eq!(journal.committed_height(), 5);

        for i in 0..5 {
            assert!(state.get_record(RecordId(i)).is_some());
        }
    }

    #[test]
    fn read_events_after_lsn_returns_only_the_tail() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");
        {
            let mut writer = EventLogWriter::open(&log_path, Some(16)).unwrap();
            for i in 0..10 {
                writer
                    .append(&crate::events::event_log::LogEntry::Event(
                        KernelEvent::InsertRecord {
                            id: RecordId(i),
                            vector: FxpVector::new_zeros(16),
                            metadata: None,
                            tag: 0,
                        },
                    ))
                    .unwrap();
            }
        }

        let all = read_all_segments(&log_path, Some(16)).unwrap();
        assert_eq!(all.len(), 10);

        // Lsn(0) = "nothing committed yet" -> the whole stream.
        let tail0 = read_events_after_lsn(&log_path, Some(16), 0).unwrap();
        assert_eq!(tail0.len(), 10);

        // Lsn(6) = 6 events committed -> events 7..10 (4 events) remain.
        let tail6 = read_events_after_lsn(&log_path, Some(16), 6).unwrap();
        assert_eq!(tail6.len(), 4);
        for (i, (_, evt)) in tail6.iter().enumerate() {
            if let KernelEvent::InsertRecord { id, .. } = evt {
                assert_eq!(id.0, 6 + i as u32);
            } else {
                panic!("unexpected event");
            }
        }

        // A LSN at or past the end returns nothing, never an error or wraparound.
        assert!(read_events_after_lsn(&log_path, Some(16), 10)
            .unwrap()
            .is_empty());
        assert!(read_events_after_lsn(&log_path, Some(16), 999)
            .unwrap()
            .is_empty());
    }

    /// Regression guard for the EventJournal replay-timestamp bug: a
    /// replayed event must report the SAME timestamp it was actually
    /// committed with, not 0 (which the API renders as 1970-01-01) and not
    /// a fresh "now" stamped at recovery time. `EventLogWriter::append`
    /// stamps `wall_time_secs` with the real wall clock at write time (see
    /// `now_secs()`/`append_with_request_id` in event_log.rs) — this test
    /// captures that same window before writing and asserts recovery
    /// reports a timestamp inside it, then recovers the identical log a
    /// second time and asserts the two recoveries agree exactly (proving
    /// the value is genuinely persisted and stable, not regenerated).
    #[test]
    fn replayed_event_keeps_its_real_commit_timestamp_across_restart() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        {
            let mut writer = EventLogWriter::open(&log_path, Some(16)).unwrap();
            writer
                .append(&crate::events::event_log::LogEntry::Event(ev(0)))
                .unwrap();
        }

        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // "Restart" — recover from the on-disk log exactly as a real node
        // boot does.
        let (_state, journal, count) = recover_from_event_log(&log_path).unwrap();
        assert_eq!(count, 1);

        let recovered_ts = journal
            .event_timestamp(0)
            .expect("index 0 must have a timestamp after recovery");

        assert_ne!(
            recovered_ts, 0,
            "a replayed event must not report the epoch (1970-01-01) — \
             the real commit timestamp is persisted in the WAL frame and must survive recovery"
        );
        assert!(
            recovered_ts >= before && recovered_ts <= after,
            "recovered timestamp {recovered_ts} must fall within the real write window [{before}, {after}]"
        );

        // Recover the SAME log a second time (simulating a second restart)
        // and confirm the timestamp is stable — not re-derived from
        // whatever "now" happens to be at each recovery.
        let (_state2, journal2, _count2) = recover_from_event_log(&log_path).unwrap();
        assert_eq!(
            journal2.event_timestamp(0),
            journal.event_timestamp(0),
            "the same event must report the identical timestamp on every recovery"
        );
    }

    #[test]
    fn test_dimension_mismatch_rejected() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        {
            let _writer = EventLogWriter::open(&log_path, Some(16)).unwrap();
        }

        let result = read_all_segments(&log_path, Some(32));
        assert!(result.is_err());
    }

    fn ev(i: u32) -> KernelEvent {
        KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector::new_zeros(16),
            metadata: None,
            tag: 0,
        }
    }

    #[test]
    fn multi_segment_recovery_replays_archived_and_live_segments() {
        // Regression guard: before multi-segment recovery, a rotated log
        // recovered ONLY the live segment and silently dropped pre-rotation
        // history. Here 3 events are sealed into an archive and 2 more written
        // to the live segment; recovery must return all 5.
        use crate::events::event_log::LogEntry;
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");
        let archive = dir.path().join("events.log.0001");

        let mut w = EventLogWriter::open(&path, Some(16)).unwrap();
        for i in 0..3 {
            w.append(&LogEntry::Event(ev(i))).unwrap();
        }
        let sealed_head = *w.chain_head();
        w.rotate(
            &archive,
            Some(LogEntry::Checkpoint {
                event_count: 3,
                snapshot_hash: sealed_head,
                timestamp: 0,
            }),
        )
        .unwrap();
        for i in 3..5 {
            w.append(&LogEntry::Event(ev(i))).unwrap();
        }
        drop(w);

        let (state, journal, count) = recover_from_event_log(&path).unwrap();
        assert_eq!(count, 5, "must replay archived (3) + live (2) segments");
        assert_eq!(journal.committed_height(), 5);
        for i in 0..5 {
            assert!(
                state.get_record(RecordId(i)).is_some(),
                "record {i} lost across rotation"
            );
        }
    }

    #[test]
    fn namespaced_events_recover_into_their_own_collection() {
        // Phase S15 regression: before EventNs existed, a record written to a
        // non-default collection replayed into the DEFAULT namespace on
        // restart — the collection came back empty ("documents disappeared").
        // Here we write one record into namespace 1 via commit_event_ns, drop
        // the committer (flush), recover from scratch, and assert the record
        // landed back in namespace 1 — not namespace 0.
        use crate::events::event_commit::EventCommitter;
        use crate::events::event_journal::EventJournal;
        use valori_kernel::event::KernelEvent;

        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        {
            let writer = EventLogWriter::open(&log_path, Some(16)).unwrap();
            let mut committer =
                EventCommitter::new(writer, EventJournal::new(), KernelState::new());
            // A default-namespace record (id 0) and a namespace-1 record (id 1).
            committer
                .commit_event(KernelEvent::InsertRecord {
                    id: RecordId(0),
                    vector: FxpVector::new_zeros(16),
                    metadata: None,
                    tag: 0,
                })
                .unwrap();
            committer
                .commit_event_ns(
                    KernelEvent::InsertRecord {
                        id: RecordId(1),
                        vector: FxpVector::new_zeros(16),
                        metadata: None,
                        tag: 0,
                    },
                    1,
                )
                .unwrap();
            // Drop flushes the buffered writes to disk.
        }

        let (state, _journal, count) = recover_from_event_log(&log_path).unwrap();
        assert_eq!(count, 2);

        // Record 1 must be in namespace 1, NOT namespace 0.
        let ns0: Vec<u32> = state.iter_records_in_ns(0).map(|r| r.id.0).collect();
        let ns1: Vec<u32> = state.iter_records_in_ns(1).map(|r| r.id.0).collect();
        assert_eq!(
            ns0,
            vec![0],
            "only the default-namespace record belongs in ns 0"
        );
        assert_eq!(
            ns1,
            vec![1],
            "the namespaced record must recover into ns 1, not ns 0"
        );
    }

    // ── G0.1 — graph-inclusive crash/restart recovery (Phase 11) ─────────────
    //
    // Exercises the REAL production replay path (`recover_from_event_log` ->
    // `replay_events` in this file, namespace-aware) with node/edge events
    // spanning two namespaces, proving:
    //   1. Nodes and edges survive an event-log crash/restart round-trip.
    //   2. They land back in the namespace they were created in (not
    //      DEFAULT_NS) — the same class of bug `namespaced_events_recover_
    //      into_their_own_collection` regression-guards for records.
    //   3. A cross-namespace edge written directly into the log (bypassing
    //      the HTTP/API layer entirely) is still rejected during replay,
    //      because replay calls the same `apply_event_ns` that live writes
    //      do — proving the namespace invariant cannot be bypassed by
    //      handing replay a malformed/adversarial event log.
    #[test]
    fn graph_events_recover_into_their_own_namespace_and_reject_cross_ns_edges_on_replay() {
        use crate::events::event_log::LogEntry;
        use valori_kernel::event::KernelEvent;
        use valori_kernel::types::enums::{EdgeKind, NodeKind};
        use valori_kernel::types::id::{EdgeId, NodeId};

        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        {
            let mut writer = EventLogWriter::open(&log_path, Some(4)).unwrap();
            // Namespace 0: two nodes + one edge (self-loop, to also exercise
            // that self-loops survive a real disk round-trip).
            writer
                .append(&LogEntry::EventNs {
                    namespace_id: 0,
                    event: KernelEvent::CreateNode {
                        id: NodeId(0),
                        kind: NodeKind::Concept,
                        record: None,
                    },
                })
                .unwrap();
            writer
                .append(&LogEntry::EventNs {
                    namespace_id: 0,
                    event: KernelEvent::CreateEdge {
                        id: EdgeId(0),
                        from: NodeId(0),
                        to: NodeId(0),
                        kind: EdgeKind::Relation,
                    },
                })
                .unwrap();
            // Namespace 1: one node, unrelated to namespace 0's graph.
            writer
                .append(&LogEntry::EventNs {
                    namespace_id: 1,
                    event: KernelEvent::CreateNode {
                        id: NodeId(1),
                        kind: NodeKind::Document,
                        record: None,
                    },
                })
                .unwrap();
        }

        let (state, _journal, count) = recover_from_event_log(&log_path).unwrap();
        assert_eq!(count, 3, "all 3 graph events must replay");

        // Both nodes survived and are queryable.
        assert!(state.get_node(NodeId(0)).is_some());
        assert!(state.get_node(NodeId(1)).is_some());
        assert_eq!(
            state.get_node(NodeId(0)).unwrap().namespace_id,
            0,
            "node 0 must recover into namespace 0, not DEFAULT_NS by accident"
        );
        assert_eq!(
            state.get_node(NodeId(1)).unwrap().namespace_id,
            1,
            "node 1 must recover into namespace 1, not namespace 0"
        );

        // The self-loop edge round-tripped and is visible from both directions.
        let out: Vec<u32> = state
            .outgoing_edges(NodeId(0))
            .unwrap()
            .map(|e| e.id.0)
            .collect();
        let inc: Vec<u32> = state
            .incoming_edges(NodeId(0))
            .unwrap()
            .map(|e| e.id.0)
            .collect();
        assert_eq!(out, vec![0], "self-loop must appear in outgoing adjacency");
        assert_eq!(inc, vec![0], "self-loop must appear in incoming adjacency");

        // Now prove replay cannot be tricked into a cross-namespace edge:
        // append a THIRD event directly to a fresh log, spanning ns 0 -> ns 1,
        // and confirm the real recovery path rejects it exactly as live
        // `apply_event_ns` would (same code path, same invariant, no bypass
        // via the event log).
        let bad_log_path = dir.path().join("events-bad.log");
        {
            let mut writer = EventLogWriter::open(&bad_log_path, Some(4)).unwrap();
            writer
                .append(&LogEntry::EventNs {
                    namespace_id: 0,
                    event: KernelEvent::CreateNode {
                        id: NodeId(0),
                        kind: NodeKind::Concept,
                        record: None,
                    },
                })
                .unwrap();
            writer
                .append(&LogEntry::EventNs {
                    namespace_id: 1,
                    event: KernelEvent::CreateNode {
                        id: NodeId(1),
                        kind: NodeKind::Concept,
                        record: None,
                    },
                })
                .unwrap();
            // Cross-namespace edge: node 0 lives in ns 0, node 1 lives in ns
            // 1. The event itself carries no namespace opinion (`CreateEdge`
            // doesn't take one) -- the invariant is enforced by comparing the
            // two endpoint nodes' OWN namespace_id fields inside
            // `apply_event_ns`, so this must fail regardless of which
            // namespace_id the log entry wrapper claims.
            writer
                .append(&LogEntry::EventNs {
                    namespace_id: 0,
                    event: KernelEvent::CreateEdge {
                        id: EdgeId(0),
                        from: NodeId(0),
                        to: NodeId(1),
                        kind: EdgeKind::Relation,
                    },
                })
                .unwrap();
        }

        let result = recover_from_event_log(&bad_log_path);
        assert!(
            result.is_err(),
            "replay must reject a cross-namespace edge exactly as live apply does — \
             the event log is not a way to bypass the namespace invariant"
        );
    }

    #[test]
    fn broken_splice_is_detected_not_silently_skipped() {
        // A live segment whose header points at a chain head no local archive
        // closes with must fail recovery rather than replay a truncated history.
        use crate::events::event_log::LogEntry;
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.log");
        let archive = dir.path().join("events.log.0001");

        let mut w = EventLogWriter::open(&path, Some(16)).unwrap();
        for i in 0..3 {
            w.append(&LogEntry::Event(ev(i))).unwrap();
        }
        let head = *w.chain_head();
        w.rotate(
            &archive,
            Some(LogEntry::Checkpoint {
                event_count: 3,
                snapshot_hash: head,
                timestamp: 0,
            }),
        )
        .unwrap();
        w.append(&LogEntry::Event(ev(3))).unwrap();
        drop(w);

        // Corrupt the archive so its closing chain head no longer matches the
        // live segment's recorded splice point.
        let mut bytes = std::fs::read(&archive).unwrap();
        *bytes.last_mut().unwrap() ^= 0xFF;
        std::fs::write(&archive, &bytes).unwrap();

        assert!(
            recover_from_event_log(&path).is_err(),
            "a broken splice between segments must be detected"
        );
    }

    #[test]
    fn stream_events_from_provider_replays_sealed_and_active_segments() {
        use crate::events::event_commit::EventCommitter;
        use crate::provider::local::LocalStorageProvider;
        use std::sync::Arc;

        let dir = tempdir().unwrap();
        let storage_root = dir.path().join("storage");
        let provider = Arc::new(LocalStorageProvider::open(&storage_root).unwrap());
        let project_id = ProjectId::new();
        let shard_id = ShardId(0);

        let wal_dir = dir.path().join("wal");
        std::fs::create_dir_all(&wal_dir).unwrap();
        let log_path = wal_dir.join("events.log");

        let event_log = EventLogWriter::open(&log_path, Some(4)).unwrap();
        let journal = EventJournal::new();
        let live_state = KernelState::new();

        let mut committer = EventCommitter::new(event_log, journal, live_state)
            .with_flush_every(1)
            .with_storage_provider(provider.clone(), project_id, shard_id);

        // Commit 2 events in segment 0
        committer.commit_event_ns(ev(0), 1).unwrap();
        committer.commit_event_ns(ev(1), 1).unwrap();

        // Rotate segment 0
        let archive_path = wal_dir.join("events.log.000000");
        committer.rotate_log(&archive_path, None).unwrap();

        // Commit 2 events in segment 1 (active)
        committer.commit_event_ns(ev(2), 1).unwrap();
        committer.commit_event_ns(ev(3), 1).unwrap();

        // Stream all events (after_lsn = 0)
        let tail_all = stream_events_from_provider(
            provider.as_ref(),
            project_id,
            shard_id,
            Some(4),
            0,
            Some(&log_path),
        )
        .unwrap();
        assert_eq!(tail_all.len(), 4);

        // Stream tail (after_lsn = 2)
        let tail_after_2 = stream_events_from_provider(
            provider.as_ref(),
            project_id,
            shard_id,
            Some(4),
            2,
            Some(&log_path),
        )
        .unwrap();
        assert_eq!(tail_after_2.len(), 2);
        assert_eq!(tail_after_2[0].0, 1);
        assert_eq!(tail_after_2[1].0, 1);
    }
}
