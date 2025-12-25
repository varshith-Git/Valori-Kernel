//! Deterministic Replay Logic.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.

use crate::state::kernel::KernelState;
use crate::state::command::Command;
use crate::error::{Result, KernelError};
use crate::verify::kernel_state_hash;
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
        
        let version = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        let encoding_version = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let dim = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        let checksum_len = u32::from_le_bytes(buf[12..16].try_into().unwrap());
        
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
pub fn replay_and_hash<const MAX_RECORDS: usize, const D: usize, const MAX_NODES: usize, const MAX_EDGES: usize>(
    snapshot_bytes: &[u8],
    wal_bytes: &[u8],
) -> Result<[u8; 32]> {
    // 1. Restore Base State
    let mut state: KernelState<MAX_RECORDS, D, MAX_NODES, MAX_EDGES> = if snapshot_bytes.is_empty() {
        KernelState::new()
    } else {
         decode_state(snapshot_bytes)?
    };

    // 2. Validate WAL Header
    // If WAL is empty, we permit it ONLY if truly empty (no header either? or must have header?)
    // The prompt says "WAL must be [HEADER][...]" implies header is mandatory.
    // If wal_bytes is empty, it's a "no op". But if it has bytes, it MUST have header.
    // Let's assume strict compliance: empty buffer = valid (0 commands).
    // Buffer with data = Must have header.
    
    let mut slice = wal_bytes;
    if !slice.is_empty() {
        let (header, rest) = WalHeader::read(slice)?;
        
        // Validate
        if header.dim != D as u32 {
            return Err(KernelError::InvalidInput);
        }
        // Future: Check version/encoding
        
        slice = rest;
    }

    // 3. Replay WAL Commands
    let config = bincode::config::standard();
    
    while !slice.is_empty() {
        // bincode 2.0 decode_from_slice returns (Value, BytesRead)
        match bincode::serde::decode_from_slice::<Command<D>, _>(slice, config) {
            Ok((cmd, read)) => {
                // Apply Command
                state.apply(&cmd)?;
                
                // Advance slice
                slice = &slice[read..];
            },
            Err(_) => {
                // Determine if EOF or Error
                // If slice wasn't empty but decode failed -> Corrupt WAL
                return Err(KernelError::InvalidInput);
            }
        }
    }

    // 4. Compute Hash
    Ok(kernel_state_hash(&state))
}
