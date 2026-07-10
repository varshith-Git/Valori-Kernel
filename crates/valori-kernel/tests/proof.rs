// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Tests for proof.rs — Merkle tree and DeterministicProof.

use valori_kernel::proof::{merkle_root, generate_proof_bytes, DeterministicProof};

// ─── merkle_root ────────────────────────────────────────────────────────────

#[test]
fn merkle_root_empty_is_zero() {
    assert_eq!(merkle_root(&[]), [0u8; 32]);
}

#[test]
fn merkle_root_single_leaf_is_identity() {
    let leaf = [1u8; 32];
    assert_eq!(merkle_root(&[leaf]), leaf);
}

#[test]
fn merkle_root_two_leaves_is_deterministic() {
    let a = [1u8; 32];
    let b = [2u8; 32];
    let r1 = merkle_root(&[a, b]);
    let r2 = merkle_root(&[a, b]);
    assert_eq!(r1, r2);
    // Must differ from either leaf.
    assert_ne!(r1, a);
    assert_ne!(r1, b);
}

#[test]
fn merkle_root_odd_count_pads_last() {
    // Three leaves: the third is hashed with itself for the pair.
    let leaves: Vec<[u8; 32]> = (0u8..3).map(|i| [i; 32]).collect();
    let r = merkle_root(&leaves);
    assert_ne!(r, [0u8; 32]);
    // Recompute — determinism check.
    assert_eq!(merkle_root(&leaves), r);
}

#[test]
fn merkle_root_order_sensitive() {
    let a = [1u8; 32];
    let b = [2u8; 32];
    assert_ne!(merkle_root(&[a, b]), merkle_root(&[b, a]));
}

#[test]
fn merkle_root_large_even() {
    let leaves: Vec<[u8; 32]> = (0u8..8).map(|i| [i; 32]).collect();
    let r = merkle_root(&leaves);
    assert_ne!(r, [0u8; 32]);
    assert_eq!(merkle_root(&leaves), r);
}

// ─── generate_proof_bytes ───────────────────────────────────────────────────

#[test]
fn generate_proof_bytes_empty_is_zeros() {
    assert_eq!(generate_proof_bytes(&[]), vec![0u8; 32]);
}

#[test]
fn generate_proof_bytes_single_value() {
    let result = generate_proof_bytes(&[65536]); // 1.0 in Q16.16
    assert_eq!(result.len(), 32);
    assert_ne!(result, vec![0u8; 32]);
}

#[test]
fn generate_proof_bytes_deterministic() {
    let vals = [65536, -65536, 32768, 0];
    assert_eq!(generate_proof_bytes(&vals), generate_proof_bytes(&vals));
}

#[test]
fn generate_proof_bytes_position_sensitive() {
    // Same values in different positions must produce different proofs.
    let ab = generate_proof_bytes(&[1, 2]);
    let ba = generate_proof_bytes(&[2, 1]);
    assert_ne!(ab, ba);
}

// ─── DeterministicProof ─────────────────────────────────────────────────────

#[test]
fn deterministic_proof_roundtrip_bincode() {
    let proof = DeterministicProof {
        kernel_version: 6,
        snapshot_hash: [0xAA; 32],
        wal_hash: [0xBB; 32],
        final_state_hash: [0xCC; 32],
    };
    let encoded = bincode::serde::encode_to_vec(&proof, bincode::config::standard()).unwrap();
    let (decoded, _): (DeterministicProof, _) =
        bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
    assert_eq!(decoded, proof);
}

#[test]
fn deterministic_proof_equality() {
    let p1 = DeterministicProof {
        kernel_version: 1,
        snapshot_hash: [0u8; 32],
        wal_hash: [1u8; 32],
        final_state_hash: [2u8; 32],
    };
    let p2 = p1.clone();
    assert_eq!(p1, p2);
}
