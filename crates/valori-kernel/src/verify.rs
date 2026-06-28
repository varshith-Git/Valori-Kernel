// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Snapshot and WAL byte-level hash helpers.
//!
//! State hashing is handled exclusively by [`crate::snapshot::blake3::hash_state_blake3`]
//! — the single canonical function used by the consensus layer, the verifier,
//! and all proof endpoints. Do not add a second state-hash function here.

/// BLAKE3 hash of raw snapshot bytes (used by the verifier to check snapshot integrity).
pub fn snapshot_hash(snapshot_bytes: &[u8]) -> [u8; 32] {
    blake3::hash(snapshot_bytes).into()
}

/// BLAKE3 hash of raw WAL bytes (used by the verifier to check WAL integrity).
pub fn wal_hash(wal_bytes: &[u8]) -> [u8; 32] {
    blake3::hash(wal_bytes).into()
}
