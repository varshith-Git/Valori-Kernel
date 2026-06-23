// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Crypto-shredding seam — Phase 1.5.
//!
//! Design principle: erase = key destruction.
//!
//! Records encrypted under a `key_id` are stored in the event log as
//! `KernelEvent::InsertRecordEncrypted` forever. When `shred(key_id)` is
//! called, the key is destroyed. The log entry remains (preserving the audit
//! chain), but the ciphertext is permanently unreadable. To a verifier the
//! record is "present, unrecoverable" — the chain head still commits to it.
//!
//! Satisfies GDPR Article 17 ("right to erasure") without any log truncation,
//! chain gap, or hash discontinuity.
//!
//! # Current state (Phase 1.5)
//!
//! The trait and types are defined here. `NullVault` is the only
//! implementation — it panics if called, ensuring no encrypted records can be
//! written until a real vault is wired in. The actual AES-GCM / ChaCha20-Poly1305
//! implementation and the key-store backend land in a future phase.

extern crate alloc;

/// 128-bit key identifier. Typically a random UUID (version 4) assigned when
/// the record is written. Must be globally unique within the deployment.
pub type KeyId = [u8; 16];

/// Errors returned by `KeyVault` operations.
#[derive(Debug)]
pub enum CryptoError {
    /// The key was never registered or has already been shredded.
    KeyNotFound(KeyId),
    /// Encryption failed (e.g. nonce exhaustion, backend error).
    EncryptionFailed(alloc::string::String),
    /// Decryption failed — wrong key, truncated ciphertext, or authentication tag mismatch.
    DecryptionFailed(alloc::string::String),
    /// The vault backend returned an unexpected error.
    BackendError(alloc::string::String),
}

impl core::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CryptoError::KeyNotFound(k) => write!(f, "key not found: {}", crate::crypto::hex_key(k)),
            CryptoError::EncryptionFailed(s) => write!(f, "encryption failed: {s}"),
            CryptoError::DecryptionFailed(s) => write!(f, "decryption failed: {s}"),
            CryptoError::BackendError(s) => write!(f, "vault backend error: {s}"),
        }
    }
}

/// Key vault interface — the only abstraction boundary between the kernel and
/// the encryption backend (in-process AES-GCM, KMS, HSM, …).
///
/// Implementations must be `Send + Sync` so the vault can be held behind an
/// `Arc` and shared across async tasks.
///
/// # Invariants
///
/// - After `shred(key_id)` returns `Ok`, `decrypt(key_id, _)` must return
///   `Err(CryptoError::KeyNotFound)` — even if the process restarts.
/// - `encrypt` and `decrypt` must be deterministic for the same (key, plaintext)
///   only if the cipher is deterministic; for AEAD ciphers the nonce is
///   embedded in the ciphertext output and every call MAY return different bytes.
/// - `key_exists` is advisory only — a concurrent `shred` may race it.
pub trait KeyVault: Send + Sync {
    /// Encrypt `plaintext` under `key_id` and return the authenticated ciphertext
    /// (nonce prepended). Creates the key if it does not yet exist.
    fn encrypt(&self, key_id: KeyId, plaintext: &[u8]) -> Result<alloc::vec::Vec<u8>, CryptoError>;

    /// Decrypt and authenticate `ciphertext` (nonce prepended) under `key_id`.
    /// Returns `Err(KeyNotFound)` if the key has been shredded.
    fn decrypt(&self, key_id: KeyId, ciphertext: &[u8]) -> Result<alloc::vec::Vec<u8>, CryptoError>;

    /// Permanently destroy `key_id`. All records encrypted under it become
    /// unrecoverable. This operation must be durable: a process crash after
    /// `shred` returns must not resurrect the key.
    fn shred(&self, key_id: KeyId) -> Result<(), CryptoError>;

    /// Returns `true` if the key exists and has not been shredded.
    /// Advisory: may race with a concurrent `shred`.
    fn key_exists(&self, key_id: &KeyId) -> bool;
}

/// Stub vault — panics on any call.
///
/// Used as the default until a real vault is configured. Ensures that no
/// `InsertRecordEncrypted` event can reach the engine before encryption is
/// actually implemented; the panic surfaces the misconfiguration immediately
/// rather than silently writing unencrypted data under an "encrypted" event.
pub struct NullVault;

impl KeyVault for NullVault {
    fn encrypt(&self, _: KeyId, _: &[u8]) -> Result<alloc::vec::Vec<u8>, CryptoError> {
        panic!("NullVault: crypto-shredding is not yet implemented (Phase 1.5 stub)")
    }

    fn decrypt(&self, _: KeyId, _: &[u8]) -> Result<alloc::vec::Vec<u8>, CryptoError> {
        panic!("NullVault: crypto-shredding is not yet implemented (Phase 1.5 stub)")
    }

    fn shred(&self, _: KeyId) -> Result<(), CryptoError> {
        panic!("NullVault: crypto-shredding is not yet implemented (Phase 1.5 stub)")
    }

    fn key_exists(&self, _: &KeyId) -> bool {
        false
    }
}

fn hex_key(k: &KeyId) -> alloc::string::String {
    k.iter().map(|b| alloc::format!("{b:02x}")).collect()
}
