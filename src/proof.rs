//! Deterministic Proof Structures.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use serde::{Serialize, Deserialize};

/// A cryptographic proof of the kernel's state and history.
///
/// This struct is designed to be serialized deterministically (e.g., via bincode or canonical JSON)
/// to serve as a receipt that a specific sequence of commands (WAL) applied to a specific
/// snapshot results in a specific final state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeterministicProof {
    /// The version of the kernel protocol (schema version).
    pub kernel_version: u64,
    
    /// BLAKE3 hash of the starting snapshot (canonical encoding).
    pub snapshot_hash: [u8; 32],
    
    /// BLAKE3 hash of the WAL file (command log).
    pub wal_hash: [u8; 32],
    
    /// BLAKE3 hash of the final kernel state after replay.
    pub final_state_hash: [u8; 32],
}

/// Computes the recursive Merkle tree root from leaf hashes.
/// Odd leaf is hashed with itself.
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    if leaves.len() == 1 {
        return leaves[0];
    }

    let next_level: Vec<[u8; 32]> = leaves
        .chunks(2)
        .map(|pair| {
            let mut hasher = blake3::Hasher::new();
            hasher.update(&pair[0]);
            hasher.update(pair.get(1).unwrap_or(&pair[0]));
            *hasher.finalize().as_bytes()
        })
        .collect();

    merkle_root(&next_level)
}

/// Generates a raw 32-byte BLAKE3 Merkle root from Q16.16 integers.
/// Single source of truth for Merkle logic.
pub fn generate_proof_bytes(fixed_values: &[i32]) -> Vec<u8> {
    if fixed_values.is_empty() {
        return vec![0u8; 32];
    }
    let leaves: Vec<[u8; 32]> = fixed_values
        .iter()
        .enumerate()
        .map(|(pos, &val)| {
            let mut buf = [0u8; 8];
            buf[..4].copy_from_slice(&(pos as u32).to_le_bytes());
            buf[4..].copy_from_slice(&val.to_le_bytes());
            *blake3::hash(&buf).as_bytes()
        })
        .collect();

    merkle_root(&leaves).to_vec()
}
