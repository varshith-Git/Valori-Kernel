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

/// Full recovery from event log
pub fn recover_from_event_log(
    log_path: impl AsRef<Path>
) -> Result<(KernelState, EventJournal, u64)> {
    tracing::info!("Starting recovery from event log: {:?}", log_path.as_ref());

    let events = read_event_log(log_path, None)?;
    let event_count = events.len() as u64;

    tracing::info!("Loaded {} events from log", event_count);

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
}
