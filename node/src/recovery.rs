// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Crash Recovery
//!
//! Provides deterministic recovery via:
//! - Event log replay (Phase 23+: canonical truth)
//! - WAL replay (legacy fallback)

use valori_kernel::state::command::Command;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::snapshot::decode::decode_state;

use crate::wal_reader::{WalReader, WalReaderError};
use crate::wal_writer::WalWriter; // Added for tests
use crate::errors::EngineError;
use crate::events::event_replay::{recover_from_event_log, verify_snapshot_consistency};
use crate::events::EventJournal;

use std::path::Path;

/// Replay WAL commands on top of existing kernel state
/// 
/// This function is deterministic: same snapshot + same WAL = same final state
/// Returns (commands_applied, Hasher)
pub fn replay_wal<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize>(
    state: &mut KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>,
    wal_path: &Path,
) -> Result<(usize, blake3::Hasher), EngineError> {
    // Explicit generic D to guide inference
    let reader = WalReader::<D>::open(wal_path)
        .map_err(|e| EngineError::InvalidInput(format!("Failed to open WAL: {}", e)))?;
    
    let start = std::time::Instant::now();
    let mut commands_applied = 0;
    
    // Maintain Hash Accumulator for Proof
    let mut hasher = blake3::Hasher::new();
    
    // 1. Hash Header (Reconstructed)
    // Must match exactly what Embedded ShadowKernel builds/validates.
    // [Ver:4][Enc:4][Dim:4][Crc:4]
    // WalHeader struct is in valori-kernel::replay.
    // We can manually build bytes to be safe/explicit.
    {
        let header_ver = 1u32;
        let enc_ver = 0u32;
        let dim = D as u32;
        let crc_len = 0u32;
        
        hasher.update(&header_ver.to_le_bytes());
        hasher.update(&enc_ver.to_le_bytes());
        hasher.update(&dim.to_le_bytes());
        hasher.update(&crc_len.to_le_bytes());
    }

    // reader directly implements IntoIterator
    for result in reader {
        let cmd = result
            .map_err(|e| EngineError::InvalidInput(format!("WAL read error: {}", e)))?;

        // Apply command to kernel
        state.apply(&cmd)
            .map_err(EngineError::Kernel)?;
            
        // Hash Command (Re-serialize to ensure canonical hash)
        let cmd_bytes = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
             .map_err(|e| EngineError::InvalidInput(format!("Hash Serialization failed: {}", e)))?;
        hasher.update(&cmd_bytes);

        commands_applied += 1;
    }

    metrics::histogram!("valori_replay_duration_seconds", start.elapsed().as_secs_f64());
    Ok((commands_applied, hasher))
}


/// Check if WAL file exists and is non-empty (VALID HEADER required)
pub fn has_wal(wal_path: &Path) -> bool {
    wal_path.exists() && std::fs::metadata(wal_path)
        .map(|m| m.len() >= 16) // Must have full header
        .unwrap_or(false)
}

// ============================================================================
// Phase 24: Event-First Recovery
// ============================================================================

/// Recover from event log (canonical truth)
///
/// This is the preferred recovery path. Event log is the source of truth,
/// snapshots are just optimizations that are validated against it.
///
/// Returns (KernelState, EventJournal, event_count)
pub fn recover_from_events<const M: usize, const D: usize, const N: usize, const E: usize>(
    event_log_path: &Path
) -> Result<(KernelState<M, D, N, E>, EventJournal<D>, u64), EngineError> {
    tracing::info!("Recovering from event log: {:?}", event_log_path);
    
    recover_from_event_log(event_log_path)
        .map_err(|e| EngineError::InvalidInput(format!("Event replay failed: {:?}", e)))
}

/// Validate snapshot against replayed state
///
/// Compares snapshot hash with replayed state hash.
/// If they diverge, logs warning but doesn't fail (event log wins).
pub fn validate_snapshot<const M: usize, const D: usize, const N: usize, const E: usize>(
    snapshot_path: &Path,
    replayed_state: &KernelState<M, D, N, E>
) -> Result<bool, EngineError> {
    if !snapshot_path.exists() {
        tracing::debug!("No snapshot to validate");
        return Ok(true); // No snapshot = nothing to validate
    }
    
    tracing::info!("Validating snapshot: {:?}", snapshot_path);
    
    let snapshot_data = std::fs::read(snapshot_path)
        .map_err(|e| EngineError::InvalidInput(format!("Failed to read snapshot: {}", e)))?;
    
    let snapshot_state: KernelState<M, D, N, E> = decode_state(&snapshot_data)
        .map_err(|e| EngineError::InvalidInput(format!("Failed to decode snapshot: {:?}", e)))?;
    
    // Use existing verify_snapshot_consistency from event_replay
    let is_consistent = verify_snapshot_consistency(&snapshot_state, replayed_state);
    
    if !is_consistent {
        tracing::warn!(
            "Snapshot hash mismatch detected! Snapshot will be discarded. \
             Event log state is canonical truth."
        );
    } else {
        tracing::info!("Snapshot validated successfully");
    }
    
    Ok(is_consistent)
}

/// Check if event log exists and is valid
pub fn has_event_log(event_log_path: &Path) -> bool {
    event_log_path.exists() && std::fs::metadata(event_log_path)
        .map(|m| m.len() >= 16) // Must have at least header
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use tempfile::tempdir;

    #[test]
    fn test_replay_wal() {
        const MAX_REC: usize = 1024;
        const DIM: usize = 16;
        const MAX_NODES: usize = 1024;
        const MAX_EDGES: usize = 2048;

        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        // Write WAL
        {
            let mut writer = WalWriter::<DIM>::open(&wal_path).unwrap();
            for i in 0..100 {
                let cmd = Command::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::<DIM>::new_zeros(),
                };
                writer.append_command(&cmd).unwrap();
            }
        }

        // Replay on fresh state
        let mut state = KernelState::<MAX_REC, DIM, MAX_NODES, MAX_EDGES>::new();
        let (count, _hasher) = replay_wal(&mut state, &wal_path).unwrap();

        assert_eq!(count, 100);

        // Verify records exist
        for i in 0..100 {
            assert!(state.get_record(RecordId(i)).is_some());
        }
    }

    #[test]
    fn test_has_wal() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        // No WAL yet
        assert!(!has_wal(&wal_path));

        // Create empty WAL (header only)
        {
            let _writer = WalWriter::<16>::open(&wal_path).unwrap();
        }

        // Empty WAL (header only) = true (technically has wal, just 0 commands).
        // Logic check: if len >= 16.
        // If writer.open writes header, len is 16.
        // has_wal should return true if file is valid for REPLAY.
        // Replay will read 0 commands. That's fine.
        assert!(has_wal(&wal_path));
    }
}
