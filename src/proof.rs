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
