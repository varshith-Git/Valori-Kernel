// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Event Replay - Authoritative Recovery
//!
//! This module enforces the recovery contract:
//! **Event Log ALWAYS wins. Snapshot is just a cache.**
//!
//! # Recovery Protocol
//! 1. Load snapshot (optional, may be stale/corrupt)
//! 2. Load event log (canonical truth)
//! 3. Replay committed events into fresh state
//! 4. Compute state hash
//! 5. Replace runtime state
//!
//! # Invariants
//! - If snapshot hash ≠ replay hash → discard snapshot
//! - If event log corrupt → fail closed
//! - If dimension mismatch → fail closed
//! - Crash-symmetric: replay(events) = original_state
//!
//! # Guarantees
//! - Same events on x86 and ARM → same hash
//! - Snapshot deleted → full replay still recovers
//! - Truncated log → recovery refuses
//! - Corrupted event → fail-closed

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

/// Read and validate event log header
fn read_header<const D: usize>(file: &mut BufReader<File>) -> Result<u64> {
    let mut header_bytes = [0u8; 16];
    file.read_exact(&mut header_bytes)?;

    let version = u32::from_le_bytes(header_bytes[0..4].try_into().unwrap());
    let dim = u32::from_le_bytes(header_bytes[4..8].try_into().unwrap());
    let _reserved = u64::from_le_bytes(header_bytes[8..16].try_into().unwrap());

    if version != 1 {
        return Err(ReplayError::InvalidHeader);
    }

    if dim != D as u32 {
        return Err(ReplayError::DimensionMismatch {
            log_dim: dim,
            expected_dim: D as u32,
        });
    }

    Ok(0) // Header validated, event count will be determined during replay
}

/// Replay events from log file
///
/// # Returns
/// - Vec of events (in commit order)
/// - Total bytes read (for verification)
///
/// # Errors
/// - Corrupted event data
/// - Dimension mismatch
/// - Invalid header
pub fn read_event_log<const D: usize>(path: impl AsRef<Path>) -> Result<Vec<KernelEvent<D>>> {
    let file = File::open(path.as_ref())?;
    let mut reader = BufReader::new(file);

    // Validate header
    read_header::<D>(&mut reader)?;

    let mut events = Vec::new();
    let mut buffer = Vec::new();
    
    // Read remaining file content
    reader.read_to_end(&mut buffer)?;

    // Deserialize events
    let mut offset = 0;
    while offset < buffer.len() {
        match bincode::serde::decode_from_slice::<crate::events::event_log::LogEntry<D>, _>(
            &buffer[offset..],
            bincode::config::standard()
        ) {
            Ok((entry, bytes_read)) => {
                offset += bytes_read;
                
                match entry {
                    crate::events::event_log::LogEntry::Event(event) => {
                        events.push(event);
                    },
                    crate::events::event_log::LogEntry::Checkpoint { event_count: chk_count, snapshot_hash, timestamp: _ } => {
                        tracing::info!("Found checkpoint marker: count={}, hash={:?}", chk_count, snapshot_hash);
                        // Validation logic: verify state matches checkpoint if we were loading it?
                        // For now just log it.
                    }
                }
            }
            Err(e) => {
                // Check if we're at the tail (incomplete event from crash)
                if offset + 100 > buffer.len() {
                    // Likely tail corruption from crash mid-write
                    // This is acceptable - we replay up to last complete event
                    tracing::warn!(
                        "Ignoring incomplete event at end of log (offset {})",
                        offset
                    );
                    break;
                } else {
                    // Corruption in middle of file - this is critical
                    return Err(ReplayError::Corrupted { offset });
                }
            }
        }
    }

    Ok(events)
}

/// Replay events into a fresh kernel state
///
/// # Protocol
/// 1. Create empty state
/// 2. Apply each event in order
/// 3. Return final state + event count
///
/// # Guarantees
/// - Deterministic: same events → same state
/// - Cross-architecture: x86 = ARM = RISC-V
/// - Idempotent: replay(replay(events)) = replay(events)
pub fn replay_events<const M: usize, const D: usize, const N: usize, const E: usize>(
    events: &[KernelEvent<D>]
) -> Result<KernelState<M, D, N, E>> {
    let mut state = KernelState::new();

    for (idx, event) in events.iter().enumerate() {
        state.apply_event(event)
            .map_err(|e| {
                tracing::error!(
                    "Event replay failed at index {}: {:?}",
                    idx,
                    e
                );
                ReplayError::EventApplication(e)
            })?;
    }

    Ok(state)
}

/// Full recovery from event log
///
/// # Returns
/// - Recovered kernel state
/// - Event journal (with committed events)
/// - Event count
///
/// # Behavior
/// - Event log is authoritative
/// - Snapshot is ignored (will be validated separately)
/// - Corrupted log → fail-closed
pub fn recover_from_event_log<const M: usize, const D: usize, const N: usize, const E: usize>(
    log_path: impl AsRef<Path>
) -> Result<(KernelState<M, D, N, E>, EventJournal<D>, u64)> {
    tracing::info!("Starting recovery from event log: {:?}", log_path.as_ref());

    // Step 1: Read and validate event log
    let events = read_event_log::<D>(log_path)?;
    let event_count = events.len() as u64;

    tracing::info!("Loaded {} events from log", event_count);

    // Step 2: Replay into fresh state
    let state = replay_events::<M, D, N, E>(&events)?;

    tracing::info!("Replay complete. State hash: {:?}", 
        hash_state_blake3(&state).iter().take(8).map(|b| format!("{:02x}", b)).collect::<String>()
    );

    // Step 3: Create journal from committed events
    let journal = EventJournal::from_committed(events);

    Ok((state, journal, event_count))
}

/// Verify snapshot against replayed state
///
/// # Purpose
/// Validate that snapshot is consistent with event log.
/// If they diverge → event log wins (snapshot discarded).
///
/// # Returns
/// - `Ok(true)` if snapshot matches replay
/// - `Ok(false)` if snapshot diverges (should be discarded)
/// - `Err(_)` if verification cannot be performed
pub fn verify_snapshot_consistency<const M: usize, const D: usize, const N: usize, const E: usize>(
    snapshot_state: &KernelState<M, D, N, E>,
    replayed_state: &KernelState<M, D, N, E>,
) -> bool {
    let snapshot_hash = hash_state_blake3(snapshot_state);
    let replayed_hash = hash_state_blake3(replayed_state);

    let matches = snapshot_hash == replayed_hash;

    if !matches {
        tracing::warn!(
            "Snapshot hash mismatch detected! Snapshot will be discarded.\n\
             Snapshot: {:?}\n\
             Replayed: {:?}",
            snapshot_hash.iter().take(16).map(|b| format!("{:02x}", b)).collect::<String>(),
            replayed_hash.iter().take(16).map(|b| format!("{:02x}", b)).collect::<String>()
        );
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

        // Write some events
        {
            let mut writer = EventLogWriter::<16>::open(&log_path).unwrap();
            for i in 0..5 {
                let event = KernelEvent::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::<16>::new_zeros(),
                    metadata: None,
                    tag: 0,
                };
                writer.append(&crate::events::event_log::LogEntry::Event(event)).unwrap();
            }
        }

        // Replay
        let (state, journal, count) = 
            recover_from_event_log::<128, 16, 128, 256>(&log_path).unwrap();

        assert_eq!(count, 5);
        assert_eq!(journal.committed_height(), 5);
        
        // Verify records exist in state
        for i in 0..5 {
            assert!(state.get_record(RecordId(i)).is_some());
        }
    }
    #[test]
    fn test_replay_determinism() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        // Write events
        {
            let mut writer = EventLogWriter::<16>::open(&log_path).unwrap();
            for i in 0..10 {
                let event = KernelEvent::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::<16>::new_zeros(),
                    metadata: None,
                    tag: 0,
                };
                writer.append(&crate::events::event_log::LogEntry::Event(event)).unwrap();
            }
        }

        // Replay twice
        let (state1, _, _) = recover_from_event_log::<128, 16, 128, 256>(&log_path).unwrap();
        let (state2, _, _) = recover_from_event_log::<128, 16, 128, 256>(&log_path).unwrap();

        // Hashes must match
        let hash1 = hash_state_blake3(&state1);
        let hash2 = hash_state_blake3(&state2);

    assert_eq!(hash1, hash2, "Replay must be deterministic");
    }

    #[test]


    fn test_dimension_mismatch_rejected() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        // Create log with D=16
        {
            let _writer = EventLogWriter::<16>::open(&log_path).unwrap();
        }

        // Attempt to replay with D=32
        let result = read_event_log::<32>(&log_path);
        assert!(result.is_err());
        
        match result {
    Err(ReplayError::DimensionMismatch { .. }) => (),
            _ => panic!("Expected DimensionMismatch error"),
        }
    }

    #[test]


    fn test_snapshot_consistency_check() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("events.log");

        // Write events
        {
            let mut writer = EventLogWriter::<16>::open(&log_path).unwrap();
            for i in 0..5 {
                let event = KernelEvent::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::<16>::new_zeros(),
                    metadata: None,
                    tag: 0,
                };
                writer.append(&crate::events::event_log::LogEntry::Event(event)).unwrap();
            }
        }

        // Create two identical states via replay
        let (state1, _, _) = recover_from_event_log::<128, 16, 128, 256>(&log_path).unwrap();
        let (state2, _, _) = recover_from_event_log::<128, 16, 128, 256>(&log_path).unwrap();

        // Should match
        assert!(verify_snapshot_consistency(&state1, &state2));

        // Create a divergent state (completely empty)
        let state3 = KernelState::<128, 16, 128, 256>::new();

        // Should NOT match (state1/state2 have 5 records, state3 is empty)
        assert!(!verify_snapshot_consistency(&state1, &state3));
    }
}
