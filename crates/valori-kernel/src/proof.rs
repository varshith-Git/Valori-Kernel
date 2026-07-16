//! Deterministic Proof Structures.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use alloc::vec;
use alloc::vec::Vec;
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

const DOMAIN_LEAF: &[u8] = b"VALORI_LEAF";
const DOMAIN_NODE: &[u8] = b"VALORI_NODE";

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
            hasher.update(DOMAIN_NODE);
            hasher.update(&pair[0]);
            hasher.update(pair.get(1).unwrap_or(&pair[0]));
            *hasher.finalize().as_bytes()
        })
        .collect();

    merkle_root(&next_level)
}

/// Tamper-evident receipt for a single vector insert.
///
/// `state_hash` is `BLAKE3("valori-insert-receipt-v1" || record_id || old_root || new_root || proof || sequence || timestamp)`.
/// Callers can call `verify()` to confirm the receipt has not been altered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InsertReceipt {
    /// Record ID allocated for this insert.
    pub record_id: u32,
    /// BLAKE3 state hash before this insert.
    pub old_root: [u8; 32],
    /// BLAKE3 state hash after this insert.
    pub new_root: [u8; 32],
    /// Merkle root of the vector's Q16.16 FXP values (`generate_proof_bytes`).
    pub proof: [u8; 32],
    /// Event-log height (WAL sequence) after this insert.
    pub sequence: u64,
    /// Unix seconds when the insert was committed.
    pub timestamp: u64,
    /// Tamper-evident self-hash of this receipt.
    pub state_hash: [u8; 32],
}

impl InsertReceipt {
    /// Build a receipt. `vector_fxp` is the Q16.16 `i32` representation of each dimension.
    pub fn build(
        record_id: u32,
        old_root: [u8; 32],
        vector_fxp: &[i32],
        new_root: [u8; 32],
        sequence: u64,
        timestamp: u64,
    ) -> Self {
        let proof_vec = generate_proof_bytes(vector_fxp);
        let mut proof = [0u8; 32];
        if proof_vec.len() == 32 {
            proof.copy_from_slice(&proof_vec);
        }
        let state_hash = Self::compute_self_hash(record_id, &old_root, &new_root, &proof, sequence, timestamp);
        InsertReceipt { record_id, old_root, new_root, proof, sequence, timestamp, state_hash }
    }

    /// Returns `true` iff the `state_hash` field is consistent with the other fields.
    pub fn verify(&self) -> bool {
        let expected = Self::compute_self_hash(
            self.record_id, &self.old_root, &self.new_root, &self.proof,
            self.sequence, self.timestamp,
        );
        self.state_hash == expected
    }

    fn compute_self_hash(
        record_id: u32,
        old_root: &[u8; 32],
        new_root: &[u8; 32],
        proof: &[u8; 32],
        sequence: u64,
        timestamp: u64,
    ) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"valori-insert-receipt-v1");
        h.update(&record_id.to_le_bytes());
        h.update(old_root);
        h.update(new_root);
        h.update(proof);
        h.update(&sequence.to_le_bytes());
        h.update(&timestamp.to_le_bytes());
        *h.finalize().as_bytes()
    }
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
            let mut hasher = blake3::Hasher::new();
            hasher.update(DOMAIN_LEAF);
            hasher.update(&(pos as u32).to_le_bytes());
            hasher.update(&val.to_le_bytes());
            *hasher.finalize().as_bytes()
        })
        .collect();

    merkle_root(&leaves).to_vec()
}
