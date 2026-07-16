// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Canonical BLAKE3 Hashing
//!
//! This module defines the CANONICAL hash primitive for Valori:
//! **BLAKE3 = Valori's cryptographic hash standard**
//!
//! # Why BLAKE3?
//! - Cryptographically sound
//! - Deterministic across architectures
//! - Fast (SIMD optimized)
//! - Incremental-friendly
//! - Industry standard for verifiable systems
//!
//! # Usage
//! ALL externally-visible proofs MUST use BLAKE3:
//! - State proofs
//! - Event log proofs
//! - Snapshot proofs
//! - WAL proofs
//! - Replication validation
//!
//! # Guarantee
//! Same state → Same hash (x86 = ARM = RISC-V = WASM)

use crate::state::kernel::KernelState;
use blake3;

/// Compute BLAKE3 hash of kernel state
///
/// This is the CANONICAL state hash for all proof generation.
///
/// # Determinism
/// - Iterates state in fixed order
/// - Uses deterministic serialization
/// - No timestamps, no randomness
/// - Cross-architecture guarantee
///
/// # Hash Input Structure
/// ```text
/// domain: "valori-state" || domain_version (u8) || format_id (u8)
/// ↓
/// version (u64 LE)
/// ↓
/// For each record (in pool order):
///   id (u32 LE)
///   flags (u8)
///   vector[0..D] (i32 LE each)
///   tag (u64 LE)
///   metadata length (u32 LE, None = u32::MAX) + metadata bytes
/// ↓
/// For each node (in pool order):
///   id (u32 LE)
///   kind (u8)
///   record_id (Option<u32> LE, None = u32::MAX)
///   first_out_edge (Option<u32> LE, None = u32::MAX)
/// ↓
/// For each edge (in pool order):
///   id (u32 LE)
///   kind (u8)
///   from (u32 LE)
///   to (u32 LE)
///   next_out (Option<u32> LE, None = u32::MAX)
/// ```
///
/// Returns: [u8; 32] - BLAKE3 hash
/// Version of the hash-input schema itself. Bumped whenever the structure
/// below changes (v2 = added domain separation + tag/metadata coverage).
/// A state hashed under one domain version can never collide with the
/// same bytes hashed under another — hash changes are versioned, visible
/// events, not silent drift.
pub const STATE_HASH_DOMAIN_VERSION: u8 = 2;

pub fn hash_state_blake3(state: &KernelState) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();

    // Domain separation: a Q8.8 state must never hash-collide with a
    // Q16.16 state, and schema changes must be distinguishable.
    hasher.update(b"valori-state");
    hasher.update(&[
        STATE_HASH_DOMAIN_VERSION,
        crate::fxp::format::ACTIVE_FORMAT_ID,
    ]);

    // Version
    hasher.update(&state.version.0.to_le_bytes());

    // Records (iteration order is deterministic by pool implementation)
    for record in state.records.iter() {
        hasher.update(&record.id.0.to_le_bytes());
        hasher.update(&[record.flags]);
        for scalar in record.vector.data.iter() {
            hasher.update(&scalar.0.to_le_bytes());
        }
        // Tag and metadata are state: tags drive filtered search and
        // metadata carries per-record proofs. Leaving them out of the
        // hash would let replicas diverge invisibly (length prefix keeps
        // None / Some(empty) / adjacent-bytes cases unambiguous).
        hasher.update(&record.tag.to_le_bytes());
        match &record.metadata {
            Some(bytes) => {
                hasher.update(&(bytes.len() as u32).to_le_bytes());
                hasher.update(bytes);
            }
            None => {
                hasher.update(&u32::MAX.to_le_bytes());
            }
        }
    }

    // Nodes (in pool order - deterministic)
    for slot in state.nodes.raw_nodes().iter() {
        if let Some(node) = slot {
            hasher.update(&node.id.0.to_le_bytes());
            hasher.update(&[node.kind as u8]);

            // Record ID (None = sentinel u32::MAX)
            match node.record {
                Some(id) => {
                    hasher.update(&id.0.to_le_bytes());
                }
                None => {
                    hasher.update(&u32::MAX.to_le_bytes());
                }
            }

            // First out edge (None = sentinel u32::MAX)
            match node.first_out_edge {
                Some(id) => {
                    hasher.update(&id.0.to_le_bytes());
                }
                None => {
                    hasher.update(&u32::MAX.to_le_bytes());
                }
            }
        }
    }

    // Edges (in pool order - deterministic)
    for slot in state.edges.raw_edges().iter() {
        if let Some(edge) = slot {
            hasher.update(&edge.id.0.to_le_bytes());
            hasher.update(&[edge.kind as u8]);
            hasher.update(&edge.from.0.to_le_bytes());
            hasher.update(&edge.to.0.to_le_bytes());

            // Next out edge (None = sentinel u32::MAX)
            match edge.next_out {
                Some(id) => {
                    hasher.update(&id.0.to_le_bytes());
                }
                None => {
                    hasher.update(&u32::MAX.to_le_bytes());
                }
            }
        }
    }

    *hasher.finalize().as_bytes()
}

/// Compute BLAKE3 hash of a byte slice
///
/// Generic helper for hashing snapshots, event logs, WAL, etc.
pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::kernel::KernelState;

    #[test]
    fn test_blake3_determinism() {
        let state1 = KernelState::new();
        let state2 = KernelState::new();

        let hash1 = hash_state_blake3(&state1);
        let hash2 = hash_state_blake3(&state2);

        assert_eq!(hash1, hash2, "Empty states must hash identically");
    }

    #[test]
    fn test_blake3_output_length() {
        let state = KernelState::new();
        let hash = hash_state_blake3(&state);

        assert_eq!(hash.len(), 32, "BLAKE3 must produce 32 bytes");
    }

    #[test]
    fn test_blake3_bytes_hash() {
        let data = b"test data";
        let hash1 = hash_bytes(data);
        let hash2 = hash_bytes(data);

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 32);
    }
}
