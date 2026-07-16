// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Ed25519-signed chain-head anchors.
//!
//! An anchor is a compact, signed statement: "at `anchored_at`, the event log
//! had `event_count` entries, its chain head was `chain_head`, and the
//! replayed state hash was `state_hash`."  The statement is signed with an
//! Ed25519 key whose public half can be held by any third party (auditor,
//! customer, notary).
//!
//! ## Why this breaks circular trust
//! Without anchoring, a malicious operator can rewrite the log, recompute
//! all chain hashes, and produce a new state hash — `valori-verify` would
//! pass.  With anchoring, the operator also needs the *private* signing key.
//! If that key is held by someone other than the operator, rewriting becomes
//! detectable.
//!
//! ## Anchor file format (JSON)
//! ```json
//! {
//!   "schema_version": 1,
//!   "chain_head":          "<64 hex chars>",
//!   "event_count":         2007,
//!   "state_hash":          "<64 hex chars>",
//!   "anchored_at":         "2026-06-10T14:02:11Z",
//!   "anchored_at_unix":    1749561731,
//!   "public_key_ed25519":  "<64 hex chars — 32 bytes>",
//!   "signature_ed25519":   "<128 hex chars — 64 bytes>"
//! }
//! ```
//!
//! ## Signed message
//! ```text
//! b"valori-anchor-v1\0"   (17 bytes, domain separator)
//! || chain_head            (32 bytes)
//! || event_count_le8       (8 bytes, little-endian u64)
//! || state_hash            (32 bytes)
//! || anchored_at_unix_le8  (8 bytes, little-endian u64)
//! ```
//! Total: 97 bytes.  Ed25519 signs this verbatim (SHA-512 is applied
//! internally by the library).

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde_json::{json, Value};
use std::path::Path;
use valori_wire::format_utc;

const DOMAIN_SEP: &[u8] = b"valori-anchor-v1\0";

// ── helpers ──────────────────────────────────────────────────────────────────

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        bail!("odd-length hex string");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("invalid hex digit"))
        .collect()
}

fn json_hex(v: &Value, field: &str) -> Result<Vec<u8>> {
    let s = v[field]
        .as_str()
        .with_context(|| format!("anchor field '{field}' missing or not a string"))?;
    from_hex(s).with_context(|| format!("anchor field '{field}' is not valid hex"))
}

fn json_u64(v: &Value, field: &str) -> Result<u64> {
    v[field]
        .as_u64()
        .with_context(|| format!("anchor field '{field}' missing or not a u64"))
}

// ── anchor payload ────────────────────────────────────────────────────────────

/// The data that gets signed.
pub struct AnchorPayload {
    pub chain_head: [u8; 32],
    pub event_count: u64,
    pub state_hash: [u8; 32],
    pub anchored_at_unix: u64,
}

impl AnchorPayload {
    /// Returns the 97-byte message that is passed to Ed25519 sign/verify.
    pub fn message(&self) -> [u8; 97] {
        let mut msg = [0u8; 97];
        msg[..17].copy_from_slice(DOMAIN_SEP);
        msg[17..49].copy_from_slice(&self.chain_head);
        msg[49..57].copy_from_slice(&self.event_count.to_le_bytes());
        msg[57..89].copy_from_slice(&self.state_hash);
        msg[89..97].copy_from_slice(&self.anchored_at_unix.to_le_bytes());
        msg
    }

    /// Sign and serialize to a JSON anchor value.
    pub fn sign_to_json(&self, signing_key: &SigningKey, note: Option<&str>) -> Value {
        let sig: Signature = signing_key.sign(&self.message());
        let vk = signing_key.verifying_key();
        let mut obj = json!({
            "schema_version": 1,
            "chain_head":         to_hex(&self.chain_head),
            "event_count":        self.event_count,
            "state_hash":         to_hex(&self.state_hash),
            "anchored_at":        format_utc(self.anchored_at_unix),
            "anchored_at_unix":   self.anchored_at_unix,
            "public_key_ed25519": to_hex(vk.as_bytes()),
            "signature_ed25519":  to_hex(&sig.to_bytes()),
        });
        if let Some(n) = note {
            obj["note"] = json!(n);
        }
        obj
    }

    /// Parse an anchor JSON blob and verify the signature.
    /// Returns the payload and the public key if verification passes.
    pub fn verify_json(v: &Value) -> Result<(Self, VerifyingKey)> {
        let chain_head_vec = json_hex(v, "chain_head")?;
        let state_hash_vec = json_hex(v, "state_hash")?;
        let pk_vec = json_hex(v, "public_key_ed25519")?;
        let sig_vec = json_hex(v, "signature_ed25519")?;
        let event_count = json_u64(v, "event_count")?;
        let anchored_at_unix = json_u64(v, "anchored_at_unix")?;

        let chain_head: [u8; 32] = chain_head_vec
            .try_into()
            .map_err(|_| anyhow::anyhow!("chain_head must be 32 bytes"))?;
        let state_hash: [u8; 32] = state_hash_vec
            .try_into()
            .map_err(|_| anyhow::anyhow!("state_hash must be 32 bytes"))?;
        let pk_bytes: [u8; 32] = pk_vec
            .try_into()
            .map_err(|_| anyhow::anyhow!("public_key_ed25519 must be 32 bytes"))?;
        let sig_bytes: [u8; 64] = sig_vec
            .try_into()
            .map_err(|_| anyhow::anyhow!("signature_ed25519 must be 64 bytes"))?;

        let payload = Self {
            chain_head,
            event_count,
            state_hash,
            anchored_at_unix,
        };

        let vk = VerifyingKey::from_bytes(&pk_bytes).context("invalid Ed25519 public key")?;
        let sig = Signature::from_bytes(&sig_bytes);
        vk.verify(&payload.message(), &sig)
            .context("Ed25519 signature verification failed — anchor has been tampered with")?;

        Ok((payload, vk))
    }
}

// ── key I/O ───────────────────────────────────────────────────────────────────

/// Generate a fresh Ed25519 keypair and write `signing.key` + `verify.pub`
/// into `out_dir`.  The signing key is 32 hex-encoded bytes; keep it secret.
pub fn generate_keypair(out_dir: &Path) -> Result<()> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let sk_path = out_dir.join("signing.key");
    let vk_path = out_dir.join("verify.pub");

    std::fs::write(&sk_path, to_hex(signing_key.as_bytes()))
        .with_context(|| format!("cannot write {}", sk_path.display()))?;
    // H-1: restrict signing key to owner-read-only immediately after writing.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sk_path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("cannot chmod 0600 {}", sk_path.display()))?;
    }
    std::fs::write(&vk_path, to_hex(verifying_key.as_bytes()))
        .with_context(|| format!("cannot write {}", vk_path.display()))?;

    println!("signing key → {}", sk_path.display());
    println!("public key  → {}", vk_path.display());
    println!();
    println!("IMPORTANT: signing.key is chmod 0600 (owner-read-only).");
    println!("           Keep it secret — anyone with this file can forge anchors.");
    println!("           Distribute verify.pub to auditors/customers so they");
    println!("           can verify anchors without access to your private key.");
    Ok(())
}

/// Load a 64-hex-char signing key from a file.
pub fn load_signing_key(path: &Path) -> Result<SigningKey> {
    let hex_str = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read signing key from {}", path.display()))?;
    let bytes = from_hex(hex_str.trim())
        .with_context(|| format!("signing key at {} is not valid hex", path.display()))?;
    let key_bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("signing key must be 32 bytes (64 hex chars)"))?;
    Ok(SigningKey::from_bytes(&key_bytes))
}

/// Load a 64-hex-char verifying key from a file.
pub fn load_verifying_key(path: &Path) -> Result<VerifyingKey> {
    let hex_str = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read public key from {}", path.display()))?;
    let bytes = from_hex(hex_str.trim())
        .with_context(|| format!("public key at {} is not valid hex", path.display()))?;
    let key_bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes (64 hex chars)"))?;
    VerifyingKey::from_bytes(&key_bytes).context("invalid Ed25519 public key bytes")
}
