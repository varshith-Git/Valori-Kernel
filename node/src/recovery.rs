// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Crash Recovery via WAL Replay
//!
//! Provides deterministic recovery by replaying WAL on top of snapshot.

use valori_kernel::state::command::Command;
use valori_kernel::state::kernel::KernelState;
use crate::wal_reader::{WalReader, WalReaderError};
use crate::wal_writer::WalWriter; // Added for tests
use crate::errors::EngineError;
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

    Ok((commands_applied, hasher))
}

/// Check if WAL file exists and is non-empty (VALID HEADER required)
pub fn has_wal(wal_path: &Path) -> bool {
    wal_path.exists() && std::fs::metadata(wal_path)
        .map(|m| m.len() >= 16) // Must have full header
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
        let count = replay_wal(&mut state, &wal_path).unwrap();

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
