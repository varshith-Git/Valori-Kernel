//! Deterministic Replay Logic.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.

use crate::state::kernel::KernelState;
use crate::state::command::Command;
use crate::error::{Result, KernelError};
use crate::snapshot::blake3::hash_state_blake3;
use crate::snapshot::decode::decode_state;

/// WAL Header structure (16 bytes)
/// [Version: u32][Encoding: u32][Dim: u32][ChecksumLen: u32]
pub struct WalHeader {
    pub version: u32,
    pub encoding_version: u32,
    pub dim: u32,
    pub checksum_len: u32,
}

impl WalHeader {
    pub const SIZE: usize = 16;
    
    pub fn read(buf: &[u8]) -> Result<(Self, &[u8])> {
        if buf.len() < Self::SIZE {
            return Err(KernelError::InvalidInput);
        }
        
        let version      = u32::from_le_bytes([buf[0],  buf[1],  buf[2],  buf[3]]);
        let encoding_version = u32::from_le_bytes([buf[4],  buf[5],  buf[6],  buf[7]]);
        let dim          = u32::from_le_bytes([buf[8],  buf[9],  buf[10], buf[11]]);
        let checksum_len = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
        
        Ok((Self {
            version,
            encoding_version,
            dim,
            checksum_len,
        }, &buf[Self::SIZE..]))
    }
}

/// Replays a WAL on top of a base snapshot and returns the final state hash.
///
/// This function verifies that:
/// 1. The snapshot is valid.
/// 2. The WAL Header is valid (Version, Dimensions match).
/// 3. The WAL sequence is valid and applicable.
/// 4. The final state is deterministically computed.
///
/// # Arguments
/// - `snapshot_bytes`: Valid snapshot buffer (canonical encoding).
/// - `wal_bytes`: WAL buffer including Header + Sequence of bincode-encoded `Command`s.
pub fn replay_and_hash(
    snapshot_bytes: &[u8],
    wal_bytes: &[u8],
) -> Result<[u8; 32]> {
    // 1. Restore Base State
    let mut state: KernelState = if snapshot_bytes.is_empty() {
        KernelState::new()
    } else {
         decode_state(snapshot_bytes)?
    };

    // 2. Validate WAL Header
    let mut slice = wal_bytes;
    if !slice.is_empty() {
        let (header, rest) = WalHeader::read(slice)?;
        
        // Validate dimension if kernel already has one locked
        if let Some(locked_dim) = state.dim {
            if header.dim != locked_dim as u32 {
                return Err(KernelError::InvalidInput);
            }
        }
        
        slice = rest;
    }

    // 3. Replay WAL Commands
    let config = bincode::config::standard();
    
    while !slice.is_empty() {
        match bincode::serde::decode_from_slice::<Command, _>(slice, config) {
            Ok((cmd, read)) => {
                // Apply Command
                state.apply(&cmd)?;
                
                // Advance slice
                slice = &slice[read..];
            },
            Err(_) => {
                return Err(KernelError::InvalidInput);
            }
        }
    }

    // 4. Compute Hash — use the canonical function (domain-separated, covers tag + metadata)
    Ok(hash_state_blake3(&state))
}
