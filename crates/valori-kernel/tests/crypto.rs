// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Crypto-shredding tests (Phase 3.6).
//!
//! Verifies that InsertRecordEncrypted and ShredKey events are fully
//! implemented: round-trip through bincode, apply to KernelState, and
//! set the correct flags on records.

use valori_kernel::crypto::{CryptoError, KeyId, KeyVault, NullVault};
use valori_kernel::event::KernelEvent;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::RecordId;

const TEST_KEY_ID: KeyId = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
    0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
];

// ── KeyId type ────────────────────────────────────────────────────────────────

#[test]
fn key_id_is_16_bytes() {
    assert_eq!(core::mem::size_of::<KeyId>(), 16);
}

// ── NullVault ─────────────────────────────────────────────────────────────────

#[test]
fn null_vault_key_exists_is_always_false() {
    let vault = NullVault;
    assert!(!vault.key_exists(&TEST_KEY_ID));
}

#[test]
#[should_panic(expected = "NullVault")]
fn null_vault_encrypt_panics() {
    let vault = NullVault;
    let _ = vault.encrypt(TEST_KEY_ID, b"plaintext");
}

#[test]
#[should_panic(expected = "NullVault")]
fn null_vault_decrypt_panics() {
    let vault = NullVault;
    let _ = vault.decrypt(TEST_KEY_ID, b"ciphertext");
}

#[test]
#[should_panic(expected = "NullVault")]
fn null_vault_shred_panics() {
    let vault = NullVault;
    let _ = vault.shred(TEST_KEY_ID);
}

// ── Reserved KernelEvent variants ─────────────────────────────────────────────

#[test]
fn insert_record_encrypted_variant_serializes() {
    let evt = KernelEvent::InsertRecordEncrypted {
        id: RecordId(0),
        key_id: TEST_KEY_ID,
        ciphertext: vec![0xAA, 0xBB, 0xCC],
        metadata_ciphertext: Some(vec![0xDD]),
        tag: 42,
    };
    let bytes = bincode::serde::encode_to_vec(&evt, bincode::config::standard()).unwrap();
    let (decoded, _): (KernelEvent, _) =
        bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
    assert_eq!(decoded.event_type(), "InsertRecordEncrypted");
}

#[test]
fn shred_key_variant_serializes() {
    let evt = KernelEvent::ShredKey { key_id: TEST_KEY_ID };
    let bytes = bincode::serde::encode_to_vec(&evt, bincode::config::standard()).unwrap();
    let (decoded, _): (KernelEvent, _) =
        bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
    assert_eq!(decoded.event_type(), "ShredKey");
}

#[test]
fn insert_record_encrypted_applies_and_sets_flag() {
    use valori_kernel::storage::record::{FLAG_ENCRYPTED, FLAG_SHREDDED};

    let mut state = KernelState::new();
    state.dim = Some(4); // pre-set dim so encrypted insert doesn't require a prior insert

    let evt = KernelEvent::InsertRecordEncrypted {
        id: RecordId(0),
        key_id: TEST_KEY_ID,
        ciphertext: vec![0xDE, 0xAD, 0xBE, 0xEF],
        metadata_ciphertext: None,
        tag: 42,
    };
    state.apply_event(&evt).expect("InsertRecordEncrypted must succeed");

    let rec = state.get_record(RecordId(0)).expect("record must be allocated");
    assert!(rec.flags & FLAG_ENCRYPTED != 0, "FLAG_ENCRYPTED must be set");
    assert!(rec.flags & FLAG_SHREDDED == 0, "FLAG_SHREDDED must NOT be set yet");
    assert!(rec.vector.data.iter().all(|fxp| fxp.0 == 0), "vector must be zeroed");
}

#[test]
fn shred_key_sets_shredded_flag_on_all_matching_records() {
    use valori_kernel::storage::record::{FLAG_ENCRYPTED, FLAG_SHREDDED};

    let mut state = KernelState::new();
    state.dim = Some(4);

    // Insert two records under the same key_id
    for i in 0u32..2 {
        let evt = KernelEvent::InsertRecordEncrypted {
            id: RecordId(i),
            key_id: TEST_KEY_ID,
            ciphertext: vec![i as u8, 0, 0, 0],
            metadata_ciphertext: None,
            tag: 0,
        };
        state.apply_event(&evt).unwrap();
    }

    // Shred
    let shred = KernelEvent::ShredKey { key_id: TEST_KEY_ID };
    state.apply_event(&shred).expect("ShredKey must succeed");

    for i in 0u32..2 {
        let rec = state.get_record(RecordId(i)).expect("slot must be present");
        assert!(rec.flags & FLAG_SHREDDED != 0, "record {i} must have FLAG_SHREDDED");
        assert!(rec.metadata.is_none(), "ciphertext must be wiped from memory");
    }
}

// ── CryptoError display ───────────────────────────────────────────────────────

#[test]
fn crypto_error_display_names_the_key() {
    let err = CryptoError::KeyNotFound(TEST_KEY_ID);
    let msg = err.to_string();
    assert!(msg.contains("key not found"), "unexpected: {msg}");
    assert!(msg.contains("0123456789abcdef"), "should hex the key: {msg}");
}

// ── Trait object safety ───────────────────────────────────────────────────────

#[test]
fn key_vault_is_object_safe() {
    let _: &dyn KeyVault = &NullVault;
}
