// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Tests for proof.rs — Merkle tree and DeterministicProof.

use valori_kernel::proof::{generate_proof_bytes, merkle_root, DeterministicProof, InsertReceipt};

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

// ─── InsertReceipt ──────────────────────────────────────────────────────────

#[test]
fn insert_receipt_verify_roundtrip() {
    let old_root = [0xAA; 32];
    let new_root = [0xBB; 32];
    let fxp = [65536i32, -65536, 32768, 0]; // 1.0, -1.0, 0.5, 0.0 in Q16.16
    let receipt = InsertReceipt::build(42, old_root, &fxp, new_root, 7, 1_700_000_000);
    assert!(receipt.verify(), "freshly built receipt must verify");
    assert_eq!(receipt.record_id, 42);
    assert_eq!(receipt.old_root, old_root);
    assert_eq!(receipt.new_root, new_root);
    assert_eq!(receipt.sequence, 7);
    assert_eq!(receipt.timestamp, 1_700_000_000);
}

#[test]
fn insert_receipt_verify_detects_tampering() {
    let receipt = InsertReceipt::build(1, [0u8; 32], &[65536], [1u8; 32], 0, 0);
    let mut tampered = receipt.clone();
    tampered.record_id = 99;
    assert!(
        !tampered.verify(),
        "tampering with record_id must fail verify"
    );
    let mut tampered2 = receipt.clone();
    tampered2.sequence = 999;
    assert!(
        !tampered2.verify(),
        "tampering with sequence must fail verify"
    );
    let mut tampered3 = receipt.clone();
    tampered3.new_root[0] ^= 0xFF;
    assert!(
        !tampered3.verify(),
        "tampering with new_root must fail verify"
    );
}

#[test]
fn insert_receipt_deterministic() {
    let fxp = [65536i32, 32768, -65536];
    let r1 = InsertReceipt::build(5, [0xCC; 32], &fxp, [0xDD; 32], 3, 1_000);
    let r2 = InsertReceipt::build(5, [0xCC; 32], &fxp, [0xDD; 32], 3, 1_000);
    assert_eq!(r1, r2, "same inputs must produce identical receipts");
}

#[test]
fn insert_receipt_proof_field_matches_generate_proof_bytes() {
    let fxp = [65536i32, 32768, -65536, 0];
    let receipt = InsertReceipt::build(1, [0u8; 32], &fxp, [1u8; 32], 0, 0);
    let expected_proof = generate_proof_bytes(&fxp);
    assert_eq!(&receipt.proof[..], expected_proof.as_slice());
}

#[test]
fn insert_receipt_state_hash_differs_from_old_and_new_root() {
    let old_root = [0xAA; 32];
    let new_root = [0xBB; 32];
    let receipt = InsertReceipt::build(1, old_root, &[65536], new_root, 0, 0);
    assert_ne!(
        receipt.state_hash, old_root,
        "state_hash must differ from old_root"
    );
    assert_ne!(
        receipt.state_hash, new_root,
        "state_hash must differ from new_root"
    );
}
