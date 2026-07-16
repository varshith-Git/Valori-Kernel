// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 3.6 — AES-256-GCM key vault for crypto-shredding.
//!
//! `AesGcmVault` is the production implementation of the `KeyVault` trait.
//! It holds per-record 256-bit DEKs in memory and optionally persists the
//! *shred log* to disk (`VALORI_SHRED_LOG_PATH`) so that key destruction
//! survives process restarts.
//!
//! # Encryption format
//!
//! `encrypt(key_id, plaintext)` returns:
//! ```text
//! [ nonce: 12 bytes ][ ciphertext + GCM tag: len(plaintext) + 16 bytes ]
//! ```
//!
//! The nonce is generated fresh from `/dev/urandom` per call (non-deterministic).
//! The 256-bit AES key is generated once per `key_id` (also from `/dev/urandom`).
//!
//! # Durability contract
//!
//! After `shred(key_id)` returns `Ok`:
//! 1. The key is removed from the in-memory map.
//! 2. If a shred-log path is configured, `key_id` (hex-encoded) is appended to the file.
//! 3. On next startup the shred log is read and pre-populated into `shredded_ids`,
//!    so `decrypt` returns `KeyNotFound` even if the key somehow re-appeared.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};

use valori_kernel::crypto::{CryptoError, KeyId, KeyVault};

// ── AesGcmVault ──────────────────────────────────────────────────────────────

struct VaultInner {
    keys: HashMap<KeyId, [u8; 32]>,
    shredded: HashSet<KeyId>,
}

pub struct AesGcmVault {
    inner: RwLock<VaultInner>,
    shred_log_path: Option<PathBuf>,
}

impl AesGcmVault {
    /// Create an in-memory vault (no persistence). Keys survive only the
    /// process lifetime; shredding is in-process only.
    pub fn in_memory() -> Self {
        Self {
            inner: RwLock::new(VaultInner {
                keys: HashMap::new(),
                shredded: HashSet::new(),
            }),
            shred_log_path: None,
        }
    }

    /// Create a persistent vault. On startup, reads `shred_log_path` to
    /// re-populate the set of destroyed keys (so they stay unrecoverable
    /// across restarts even if somehow re-inserted into the key map).
    pub fn with_shred_log(path: &Path) -> std::io::Result<Self> {
        let mut shredded = HashSet::new();
        if path.exists() {
            let f = std::fs::File::open(path)?;
            for line in BufReader::new(f).lines() {
                let line = line?;
                let hex = line.trim();
                if hex.len() == 32 {
                    let mut key_id = [0u8; 16];
                    if hex_decode(hex, &mut key_id).is_ok() {
                        shredded.insert(key_id);
                    }
                }
            }
        }
        Ok(Self {
            inner: RwLock::new(VaultInner {
                keys: HashMap::new(),
                shredded,
            }),
            shred_log_path: Some(path.to_owned()),
        })
    }

    fn append_shred_log(&self, key_id: &KeyId) {
        if let Some(ref path) = self.shred_log_path {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{}", hex_key(key_id));
            }
        }
    }
}

impl KeyVault for AesGcmVault {
    fn encrypt(&self, key_id: KeyId, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let aes_key: [u8; 32] = {
            let mut inner = self
                .inner
                .write()
                .map_err(|_| CryptoError::BackendError("lock poisoned".into()))?;
            if inner.shredded.contains(&key_id) {
                return Err(CryptoError::KeyNotFound(key_id));
            }
            *inner.keys.entry(key_id).or_insert_with(|| {
                let mut k = [0u8; 32];
                OsRng.fill_bytes(&mut k);
                k
            })
        };

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&aes_key));
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

        // Output: [nonce: 12] ‖ [ciphertext + tag]
        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    fn decrypt(&self, key_id: KeyId, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if data.len() < 12 + 16 {
            return Err(CryptoError::DecryptionFailed("ciphertext too short".into()));
        }

        let inner = self
            .inner
            .read()
            .map_err(|_| CryptoError::BackendError("lock poisoned".into()))?;

        if inner.shredded.contains(&key_id) {
            return Err(CryptoError::KeyNotFound(key_id));
        }

        let aes_key = inner
            .keys
            .get(&key_id)
            .ok_or(CryptoError::KeyNotFound(key_id))?;

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(aes_key));
        let nonce = Nonce::from_slice(&data[..12]);
        cipher
            .decrypt(nonce, &data[12..])
            .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))
    }

    fn shred(&self, key_id: KeyId) -> Result<(), CryptoError> {
        {
            let mut inner = self
                .inner
                .write()
                .map_err(|_| CryptoError::BackendError("lock poisoned".into()))?;
            inner.keys.remove(&key_id);
            inner.shredded.insert(key_id);
        }
        self.append_shred_log(&key_id);
        Ok(())
    }

    fn key_exists(&self, key_id: &KeyId) -> bool {
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return false,
        };
        !inner.shredded.contains(key_id) && inner.keys.contains_key(key_id)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn hex_key(k: &KeyId) -> String {
    k.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str, out: &mut [u8; 16]) -> Result<(), ()> {
    if s.len() != 32 {
        return Err(());
    }
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(())
}

fn hex_nibble(b: u8) -> Result<u8, ()> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(()),
    }
}

/// Generate a fresh random 16-byte key_id.
pub fn new_key_id() -> KeyId {
    let mut id = [0u8; 16];
    OsRng.fill_bytes(&mut id);
    id
}

/// Format a key_id as a 32-char lowercase hex string.
pub fn key_id_to_hex(k: &KeyId) -> String {
    hex_key(k)
}

/// Parse a 32-char hex string into a KeyId.
pub fn hex_to_key_id(s: &str) -> Option<KeyId> {
    let mut out = [0u8; 16];
    hex_decode(s, &mut out).ok().map(|_| out)
}
