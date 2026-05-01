// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Event Proof - Audit Trail Generation
//!
//! This module generates cryptographic proofs of system state
//! based on the event log (canonical truth).

use serde::{Serialize, Deserialize};

/// Event-sourced proof of system state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventProof {
    /// Kernel protocol version
    pub kernel_version: u32,
    
    /// Hash of the snapshot file (if exists)
    pub snapshot_hash: [u8; 32],
    
    /// Hash of the event log file
    pub event_log_hash: [u8; 32],
    
    /// Hash of the final kernel state (after replay)
    pub final_state_hash: [u8; 32],
    
    /// Number of events in the log
    pub event_count: u64,
    
    /// Committed height (last committed event index)
    pub committed_height: u64,
}

impl EventProof {
    /// Create a new event proof
    pub fn new(
        snapshot_hash: [u8; 32],
        event_log_hash: [u8; 32],
        final_state_hash: [u8; 32],
        event_count: u64,
        committed_height: u64,
    ) -> Self {
        Self {
            kernel_version: 1,
            snapshot_hash,
            event_log_hash,
            final_state_hash,
            event_count,
            committed_height,
        }
    }

    /// Verify two proofs match
    pub fn matches(&self, other: &EventProof) -> bool {
        self.event_log_hash == other.event_log_hash
            && self.final_state_hash == other.final_state_hash
            && self.event_count == other.event_count
            && self.committed_height == other.committed_height
    }

    /// Verify this proof matches expected values
    pub fn verify(
        &self,
        expected_event_log_hash: &[u8; 32],
        expected_hash_state_blake3: &[u8; 32],
        expected_count: u64,
    ) -> bool {
        self.event_log_hash == *expected_event_log_hash
            && self.final_state_hash == *expected_hash_state_blake3
            && self.event_count == expected_count
    }
}

/// Compute hash of event log file using BLAKE3
pub fn compute_event_log_hash(path: impl AsRef<std::path::Path>) -> std::io::Result<[u8; 32]> {
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(*hasher.finalize().as_bytes())
}

/// Generate a complete event proof from current system state
pub fn generate_proof(
    state: &valori_kernel::state::kernel::KernelState,
    snapshot_path: Option<&std::path::Path>,
    event_log_path: &std::path::Path,
    event_count: u64,
    committed_height: u64,
) -> std::io::Result<EventProof> {
    use valori_kernel::snapshot::blake3::{hash_state_blake3, hash_bytes};

    // Compute snapshot hash (if exists)
    let snapshot_hash = if let Some(path) = snapshot_path {
        if path.exists() {
            use std::fs;
            let bytes = fs::read(path)?;
            hash_bytes(&bytes)
        } else {
            [0u8; 32]
        }
    } else {
        [0u8; 32]
    };

    // Compute event log hash
    let event_log_hash = compute_event_log_hash(event_log_path)?;

    // Compute final state hash using canonical BLAKE3
    let final_state_hash = hash_state_blake3(state);

    Ok(EventProof::new(
        snapshot_hash,
        event_log_hash,
        final_state_hash,
        event_count,
        committed_height,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proof_equality() {
        let proof1 = EventProof::new(
            [1u8; 32],
            [2u8; 32],
            [3u8; 32],
            100,
            100,
        );

        let proof2 = EventProof::new(
            [99u8; 32],
            [2u8; 32],
            [3u8; 32],
            100,
            100,
        );

        assert!(proof1.matches(&proof2));
    }
}
