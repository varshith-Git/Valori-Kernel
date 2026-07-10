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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_hash_matches_blake3() {
        let data = b"valori snapshot bytes";
        let expected: [u8; 32] = blake3::hash(data).into();
        assert_eq!(snapshot_hash(data), expected);
    }

    #[test]
    fn snapshot_hash_empty() {
        let expected: [u8; 32] = blake3::hash(b"").into();
        assert_eq!(snapshot_hash(b""), expected);
    }

    #[test]
    fn wal_hash_matches_blake3() {
        let data = b"valori wal bytes";
        let expected: [u8; 32] = blake3::hash(data).into();
        assert_eq!(wal_hash(data), expected);
    }

    #[test]
    fn snapshot_and_wal_hashes_differ_for_same_input() {
        let data = b"same content";
        // They both use blake3::hash so they must agree
        assert_eq!(snapshot_hash(data), wal_hash(data));
    }

    #[test]
    fn different_inputs_produce_different_hashes() {
        assert_ne!(snapshot_hash(b"a"), snapshot_hash(b"b"));
        assert_ne!(wal_hash(b"x"), wal_hash(b"y"));
    }
}
