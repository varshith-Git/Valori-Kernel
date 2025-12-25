// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Crash Recovery via WAL Replay
//!
//! Provides deterministic recovery by replaying WAL on top of snapshot.

use valori_kernel::state::command::Command;
use valori_kernel::state::kernel::KernelState;
use crate::wal_reader::{WalReader, WalReaderError};
use crate::errors::EngineError;
use std::path::Path;

/// Replay WAL commands on top of existing kernel state
/// 
/// This function is deterministic: same snapshot + same WAL = same final state
pub fn replay_wal<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize>(
    state: &mut KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES>,
    wal_path: &Path,
) -> Result<usize, EngineError> {
    let reader = WalReader::open(wal_path)
        .map_err(|e| EngineError::InvalidInput(format!("Failed to open WAL: {}", e)))?;

    let mut commands_applied = 0;

    for result in reader.commands::<D>() {
        let cmd = result
            .map_err(|e| EngineError::InvalidInput(format!("WAL read error: {}", e)))?;

        // Apply command to kernel
        state.apply(&cmd)
            .map_err(EngineError::Kernel)?;

        commands_applied += 1;
    }

    Ok(commands_applied)
}

/// Check if WAL file exists and is non-empty
pub fn has_wal(wal_path: &Path) -> bool {
    wal_path.exists() && std::fs::metadata(wal_path)
        .map(|m| m.len() > 1) // More than just version byte
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::vector::FxpVector;
    use crate::wal_writer::WalWriter;
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
            let mut writer = WalWriter::open(&wal_path).unwrap();
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

        // Create empty WAL
        {
            let _writer = WalWriter::open(&wal_path).unwrap();
        }

        // Empty WAL (only version byte) = false
        assert!(!has_wal(&wal_path));

        // Write one command
        {
            let mut writer = WalWriter::open(&wal_path).unwrap();
            let cmd = Command::InsertRecord {
                id: RecordId(0),
                vector: FxpVector::<16>::new_zeros(),
            };
            writer.append_command(&cmd).unwrap();
        }

        // Now has WAL
        assert!(has_wal(&wal_path));
    }
}
