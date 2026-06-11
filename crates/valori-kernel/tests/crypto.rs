// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Crypto-shredding seam (Phase 1.5).
//!
//! Verifies that the reserved KernelEvent variants exist in the schema,
//! round-trip through bincode, and are refused by apply_event with the
//! correct error — ensuring the seam is present but safe until the
//! encryption engine is wired in.

use valori_kernel::crypto::{CryptoError, KeyId, KeyVault, NullVault};
use valori_kernel::error::KernelError;
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
fn insert_record_encrypted_is_refused_by_apply() {
    let mut state = KernelState::new();
    let evt = KernelEvent::InsertRecordEncrypted {
        id: RecordId(0),
        key_id: TEST_KEY_ID,
        ciphertext: vec![0xDE, 0xAD],
        metadata_ciphertext: None,
        tag: 0,
    };
    let err = state.apply_event(&evt).unwrap_err();
    assert!(
        matches!(err, KernelError::NotImplemented),
        "expected NotImplemented, got {err:?}"
    );
    assert_eq!(state.record_count(), 0, "refused event must not mutate state");
}

#[test]
fn shred_key_is_refused_by_apply() {
    let mut state = KernelState::new();
    let evt = KernelEvent::ShredKey { key_id: TEST_KEY_ID };
    let err = state.apply_event(&evt).unwrap_err();
    assert!(
        matches!(err, KernelError::NotImplemented),
        "expected NotImplemented, got {err:?}"
    );
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
