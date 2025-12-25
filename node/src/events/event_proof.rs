// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Event Proof - Audit Trail Generation
//!
//! This module generates cryptographic proofs of system state
//! based on the event log (canonical truth).
//!
//! # Purpose
//! - Forensic auditing
//! - Cross-system verification (Cloud ↔ Embedded ↔ Verifier)
//! - Deterministic replay validation
//! - Legal compliance
//!
//! # Guarantee
//! Same events → Same proof (across any architecture)

use serde::{Serialize, Deserialize};

/// Event-sourced proof of system state
///
/// This proof is generated from the authoritative event log
/// and can be used to verify state consistency across:
/// - Cloud nodes
/// - Embedded devices
/// - Offline verifiers
/// - Different architectures
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventProof {
    /// Kernel protocol version
    pub kernel_version: u32,
    
    /// Hash of the snapshot file (if exists)
    /// This is the hash of the snapshot *container*, not the state
    pub snapshot_hash: [u8; 32],
    
    /// Hash of the event log file
    /// BLAKE3 hash of the entire log (header + events)
    pub event_log_hash: [u8; 32],
    
    /// Hash of the final kernel state (after replay)
    /// This is the deterministic state hash
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

    /// Verify two proofs match (for cross-system validation)
    ///
    /// Returns true if:
    /// - event_log_hash matches
    /// - final_state_hash matches
    /// - event_count matches
    /// - committed_height matches
    ///
    /// Note: snapshot_hash may differ (snapshots are optimization)
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
///
/// This hashes the entire file (header + all events)
/// for tamper detection and cross-system verification
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
///
/// This is the primary proof generation function for the Node.
pub fn generate_proof<const M: usize, const D: usize, const N: usize, const E: usize>(
    state: &valori_kernel::state::kernel::KernelState<M, D, N, E>,
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
            [0u8; 32] // No snapshot
        }
    } else {
        [0u8; 32] // No snapshot
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
            [99u8; 32], // Different snapshot hash
            [2u8; 32],  // Same event log
            [3u8; 32],  // Same state
            100,
            100,
        );

        // Should match (snapshot hash doesn't matter)
        assert!(proof1.matches(&proof2));
    }

    #[test]
    fn test_proof_inequality() {
        let proof1 = EventProof::new(
            [1u8; 32],
            [2u8; 32],
            [3u8; 32],
            100,
            100,
        );

        let proof2 = EventProof::new(
            [1u8; 32],
            [2u8; 32],
            [3u8; 32],
            101, // Different count
            101,
        );

        assert!(!proof1.matches(&proof2));
    }

    #[test]
    fn test_proof_serialization() {
        let proof = EventProof::new(
            [1u8; 32],
            [2u8; 32],
            [3u8; 32],
            100,
            100,
        );

        let json = serde_json::to_string(&proof).unwrap();
        let decoded: EventProof = serde_json::from_str(&json).unwrap();

        assert_eq!(proof, decoded);
    }
}
