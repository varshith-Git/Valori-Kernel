// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Crash Recovery
//!
//! Provides deterministic recovery via:
//! - Event log replay (Phase 23+: canonical truth)
//! - WAL replay (legacy fallback)

use valori_kernel::state::kernel::KernelState;
use valori_kernel::snapshot::decode::decode_state;

use crate::wal_reader::WalReader;
use crate::errors::EngineError;
use crate::events::event_replay::{recover_from_event_log, verify_snapshot_consistency};
use crate::events::EventJournal;

use std::path::Path;

/// Replay WAL commands on top of existing kernel state
pub fn replay_wal(
    state: &mut KernelState,
    wal_path: &Path,
) -> Result<(usize, blake3::Hasher), EngineError> {
    let dim = state.dim.map(|d| d as u32);
    let reader = WalReader::open(wal_path, dim)
        .map_err(|e| EngineError::InvalidInput(format!("Failed to open WAL: {}", e)))?;
    
    let start = std::time::Instant::now();
    let mut commands_applied = 0;
    let mut hasher = blake3::Hasher::new();
    
    {
        let header_ver = 1u32;
        let enc_ver = 0u32;
        let dim_val = dim.unwrap_or(0);
        let crc_len = 0u32;
        
        hasher.update(&header_ver.to_le_bytes());
        hasher.update(&enc_ver.to_le_bytes());
        hasher.update(&dim_val.to_le_bytes());
        hasher.update(&crc_len.to_le_bytes());
    }

    for result in reader {
        let cmd = result
            .map_err(|e| EngineError::InvalidInput(format!("WAL read error: {}", e)))?;

        state.apply(&cmd)
            .map_err(EngineError::Kernel)?;
            
        let cmd_bytes = bincode::serde::encode_to_vec(&cmd, bincode::config::standard())
             .map_err(|e| EngineError::InvalidInput(format!("Hash Serialization failed: {}", e)))?;
        hasher.update(&cmd_bytes);

        commands_applied += 1;
    }

    metrics::histogram!("valori_replay_duration_seconds", start.elapsed().as_secs_f64());
    Ok((commands_applied, hasher))
}

pub fn has_wal(wal_path: &Path) -> bool {
    wal_path.exists() && std::fs::metadata(wal_path)
        .map(|m| m.len() >= 16)
        .unwrap_or(false)
}

/// Recover from event log (canonical truth)
pub fn recover_from_events(
    event_log_path: &Path
) -> Result<(KernelState, EventJournal, u64), EngineError> {
    tracing::info!("Recovering from event log: {:?}", event_log_path);
    
    recover_from_event_log(event_log_path)
        .map_err(|e| EngineError::InvalidInput(format!("Event replay failed: {:?}", e)))
}

/// Validate snapshot against replayed state
pub fn validate_snapshot(
    snapshot_path: &Path,
    replayed_state: &KernelState
) -> Result<bool, EngineError> {
    if !snapshot_path.exists() {
        tracing::debug!("No snapshot to validate");
        return Ok(true);
    }
    
    tracing::info!("Validating snapshot: {:?}", snapshot_path);
    
    let snapshot_data = std::fs::read(snapshot_path)
        .map_err(|e| EngineError::InvalidInput(format!("Failed to read snapshot: {}", e)))?;
    
    let snapshot_state: KernelState = decode_state(&snapshot_data)
        .map_err(|e| EngineError::InvalidInput(format!("Failed to decode snapshot: {:?}", e)))?;
    
    let is_consistent = verify_snapshot_consistency(&snapshot_state, replayed_state);
    
    if !is_consistent {
        tracing::warn!("Snapshot hash mismatch detected!");
    } else {
        tracing::info!("Snapshot validated successfully");
    }
    
    Ok(is_consistent)
}

pub fn has_event_log(event_log_path: &Path) -> bool {
    event_log_path.exists() && std::fs::metadata(event_log_path)
        .map(|m| m.len() >= 16)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use valori_kernel::state::command::Command;
    use crate::wal_writer::WalWriter;
    use tempfile::tempdir;

    #[test]
    fn test_replay_wal() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        {
            let mut writer = WalWriter::open(&wal_path, 16).unwrap();
            for i in 0..100 {
                let cmd = Command::InsertRecord {
                    id: RecordId(i),
                    vector: FxpVector::new_zeros(16),
                    metadata: None,
                    tag: 0,
                };
                writer.append_command(&cmd).unwrap();
            }
        }

        let mut state = KernelState::new();
        let (count, _hasher) = replay_wal(&mut state, &wal_path).unwrap();

        assert_eq!(count, 100);

        for i in 0..100 {
            assert!(state.get_record(RecordId(i)).is_some());
        }
    }
}
