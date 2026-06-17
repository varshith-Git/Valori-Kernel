// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Event Replay - Authoritative Recovery
//!
//! This module enforces the recovery contract:
//! **Event Log ALWAYS wins. Snapshot is just a cache.**

use valori_kernel::state::kernel::KernelState;
use valori_kernel::event::KernelEvent;
use valori_kernel::error::KernelError;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use crate::events::event_journal::EventJournal;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReplayError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
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

/// Replay events from log file (any supported wire version — v2 or v3).
pub fn read_event_log(path: impl AsRef<Path>, expected_dim: Option<u32>) -> Result<Vec<KernelEvent>> {
    let file = File::open(path.as_ref())?;
    let mut reader = BufReader::new(file);

    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;

    let header = valori_wire::parse_header(&buffer).map_err(|_| ReplayError::InvalidHeader)?;
    if let Some(expected) = expected_dim {
        if header.dim != expected {
            return Err(ReplayError::DimensionMismatch {
                log_dim: header.dim,
                expected_dim: expected,
            });
        }
    }

    let mut events = Vec::new();
    let mut offset = header.header_len;
    // Recovery validates the hash chain as it replays: any in-place edit to
    // a non-final entry breaks the next entry's prev_hash, so corruption is
    // detected even when the damaged bytes still decode structurally.
    let mut chain_head = header.prev_segment_chain_head;
    while offset < buffer.len() {
        match valori_wire::decode_entry(header.version, &buffer[offset..]) {
            Ok((decoded, bytes_read)) => {
                if decoded.prev_hash != chain_head {
                    return Err(ReplayError::Corrupted { offset });
                }
                chain_head = valori_wire::chain_advance(header.version, &chain_head, &decoded)
                    .map_err(|e| ReplayError::Deserialization(e.to_string()))?;
                offset += bytes_read;

                match decoded.entry {
                    crate::events::event_log::LogEntry::Event(event) => {
                        events.push(event);
                    },
                    crate::events::event_log::LogEntry::Checkpoint { event_count: chk_count, snapshot_hash, timestamp: _ } => {
                        tracing::info!("Found checkpoint marker: count={}, hash={:?}", chk_count, snapshot_hash);
                    }
                    // Admin events (membership history) are chain-verified
                    // above but never applied to kernel state.
                    crate::events::event_log::LogEntry::Admin(admin) => {
                        tracing::info!("Admin event in log: {}", admin.describe());
                    }
                }
            }
            Err(_e) => {
                if offset + 100 > buffer.len() {
                    tracing::warn!("Ignoring incomplete event at end of log (offset {})", offset);
                    break;
                } else {
                    return Err(ReplayError::Corrupted { offset });
                }
            }
        }
    }

    Ok(events)
}

/// Replay events into a fresh kernel state
pub fn replay_events(
    events: &[KernelEvent]
) -> Result<KernelState> {
    let mut state = KernelState::new();

    for (idx, event) in events.iter().enumerate() {
        state.apply_event(event)
            .map_err(|e| {
                tracing::error!("Event replay failed at index {}: {:?}", idx, e);
                ReplayError::EventApplication(e)
            })?;
    }

    Ok(state)
}

/// One segment's replay result: its sequence number, the events it carries,
/// the chain head it splices FROM (header), and the chain head it closes WITH.
struct SegmentReplay {
    segment_seq: u32,
    prev_segment_chain_head: [u8; 32],
    final_chain_head: [u8; 32],
    events: Vec<KernelEvent>,
}

/// Read one segment file, validating its internal hash chain, and report the
/// splice endpoints so multi-segment recovery can verify continuity.
fn read_segment_full(path: impl AsRef<Path>, expected_dim: Option<u32>) -> Result<SegmentReplay> {
    let mut buffer = Vec::new();
    BufReader::new(File::open(path.as_ref())?).read_to_end(&mut buffer)?;

    let header = valori_wire::parse_header(&buffer).map_err(|_| ReplayError::InvalidHeader)?;
    if let Some(expected) = expected_dim {
        if header.dim != expected {
            return Err(ReplayError::DimensionMismatch { log_dim: header.dim, expected_dim: expected });
        }
    }

    let mut events = Vec::new();
    let mut offset = header.header_len;
    let mut chain_head = header.prev_segment_chain_head;
    while offset < buffer.len() {
        match valori_wire::decode_entry(header.version, &buffer[offset..]) {
            Ok((decoded, bytes_read)) => {
                if decoded.prev_hash != chain_head {
                    return Err(ReplayError::Corrupted { offset });
                }
                chain_head = valori_wire::chain_advance(header.version, &chain_head, &decoded)
                    .map_err(|e| ReplayError::Deserialization(e.to_string()))?;
                offset += bytes_read;
                if let crate::events::event_log::LogEntry::Event(event) = decoded.entry {
                    events.push(event);
                }
            }
            Err(_) if offset + 100 > buffer.len() => break, // trailing partial write
            Err(_) => return Err(ReplayError::Corrupted { offset }),
        }
    }

    Ok(SegmentReplay {
        segment_seq: header.segment_seq,
        prev_segment_chain_head: header.prev_segment_chain_head,
        final_chain_head: chain_head,
        events,
    })
}

/// Discover and replay every local segment for `live_path` in order.
///
/// Rotation seals `events.log` to `events.log.<suffix>` and opens a fresh
/// segment whose header splices from the sealed one's final chain head. This
/// gathers the live file plus all sibling archives, orders them by segment
/// sequence, verifies each splice point, and returns the full event history.
/// A single-segment log (no rotation has happened) reads exactly as before.
pub fn read_all_segments(
    live_path: impl AsRef<Path>,
    expected_dim: Option<u32>,
) -> Result<Vec<KernelEvent>> {
    let live_path = live_path.as_ref();

    // The live file plus any `events.log.<suffix>` archives in the same dir.
    let mut paths = vec![live_path.to_path_buf()];
    if let (Some(dir), Some(fname)) =
        (live_path.parent(), live_path.file_name().and_then(|n| n.to_str()))
    {
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

    // Concatenate in sequence order, verifying each segment splices onto the
    // previous one's closing chain head (a missing or substituted archive
    // breaks the splice and is caught here, not silently skipped).
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

/// Full recovery from the event log — replays every local segment (sealed
/// archives + the live file) so a rotated log recovers losslessly.
pub fn recover_from_event_log(
    log_path: impl AsRef<Path>
) -> Result<(KernelState, EventJournal, u64)> {
    tracing::info!("Starting recovery from event log: {:?}", log_path.as_ref());

    let events = read_all_segments(log_path, None)?;
    let event_count = events.len() as u64;

    tracing::info!("Loaded {} events across all segments", event_count);

    let state = replay_events(&events)?;
    let journal = EventJournal::from_committed(events);

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
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use tempfile::tempdir;
    use crate::events::event_log::EventLogWriter;

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
                writer.append(&crate::events::event_log::LogEntry::Event(event)).unwrap();
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
    fn test_dimension_mismatch_rejected() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        {
            let _writer = EventLogWriter::open(&log_path, Some(16)).unwrap();
        }

        let result = read_event_log(&log_path, Some(32));
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
            Some(LogEntry::Checkpoint { event_count: 3, snapshot_hash: sealed_head, timestamp: 0 }),
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
            assert!(state.get_record(RecordId(i)).is_some(), "record {i} lost across rotation");
        }
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
        w.rotate(&archive, Some(LogEntry::Checkpoint { event_count: 3, snapshot_hash: head, timestamp: 0 })).unwrap();
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
}
