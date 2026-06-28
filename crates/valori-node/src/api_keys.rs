// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Per-tenant API key management for Phase 3.5.
//!
//! Keys are stored hashed (BLAKE3, applied to a high-entropy random token)
//! in a JSON file.  The raw token is shown exactly once at creation time.
//! Three scope tiers: `read_only` < `read_write` < `admin`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Scope ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiScope {
    ReadOnly,
    ReadWrite,
    Admin,
}

impl ApiScope {
    /// Returns `true` when this scope is at least as permissive as `required`.
    pub fn satisfies(&self, required: &ApiScope) -> bool {
        match required {
            ApiScope::ReadOnly => true,
            ApiScope::ReadWrite => matches!(self, ApiScope::ReadWrite | ApiScope::Admin),
            ApiScope::Admin => matches!(self, ApiScope::Admin),
        }
    }
}

impl std::fmt::Display for ApiScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiScope::ReadOnly => write!(f, "read_only"),
            ApiScope::ReadWrite => write!(f, "read_write"),
            ApiScope::Admin => write!(f, "admin"),
        }
    }
}

// ── Stored record ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub id: String,
    pub scope: ApiScope,
    /// Optional collection lock.  `None` = unrestricted.
    pub collection: Option<String>,
    pub description: Option<String>,
    pub created_at: u64,
    /// BLAKE3 of the raw token string (`"vk_<64 hex>"`).
    pub token_hash: [u8; 32],
    /// First 8 characters of the raw token, for operator identification.
    pub prefix: String,
}

// ── API response types ────────────────────────────────────────────────────────

/// Returned only at key creation — contains the plain-text token.
#[derive(Serialize)]
pub struct ApiKeyCreated {
    pub id: String,
    /// Full plain-text token — shown exactly once; not stored.
    pub token: String,
    pub scope: ApiScope,
    pub collection: Option<String>,
    pub description: Option<String>,
    pub created_at: u64,
}

/// Safe representation for listing — token hash and raw bytes never exposed.
#[derive(Serialize)]
pub struct ApiKeyMasked {
    pub id: String,
    pub scope: ApiScope,
    pub collection: Option<String>,
    pub description: Option<String>,
    pub created_at: u64,
    /// First 8 chars of the token (e.g. `"vk_a3f2b1"`) for operator recognition.
    pub prefix: String,
}

impl From<&ApiKeyRecord> for ApiKeyMasked {
    fn from(r: &ApiKeyRecord) -> Self {
        ApiKeyMasked {
            id: r.id.clone(),
            scope: r.scope.clone(),
            collection: r.collection.clone(),
            description: r.description.clone(),
            created_at: r.created_at,
            prefix: r.prefix.clone(),
        }
    }
}

// ── Key store ─────────────────────────────────────────────────────────────────

pub struct KeyStore {
    /// Primary index: token_hash → record.
    by_hash: RwLock<HashMap<[u8; 32], ApiKeyRecord>>,
    /// Secondary index: id → token_hash (for O(1) revoke).
    id_to_hash: RwLock<HashMap<String, [u8; 32]>>,
    path: Option<PathBuf>,
}

impl KeyStore {
    pub fn new(path: Option<PathBuf>) -> Self {
        let ks = KeyStore {
            by_hash: RwLock::new(HashMap::new()),
            id_to_hash: RwLock::new(HashMap::new()),
            path,
        };
        ks.load();
        ks
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.read().unwrap().is_empty()
    }

    /// Look up a presented bearer token.  Returns `None` if not found.
    pub fn lookup(&self, raw_token: &str) -> Option<ApiKeyRecord> {
        let hash = hash_token(raw_token);
        self.by_hash.read().unwrap().get(&hash).cloned()
    }

    /// Create a new key and persist.
    pub fn create(
        &self,
        scope: ApiScope,
        collection: Option<String>,
        description: Option<String>,
    ) -> ApiKeyCreated {
        let raw = generate_token();
        let hash = hash_token(&raw);
        let id = simple_id();
        let prefix = raw.chars().take(8).collect::<String>();
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let record = ApiKeyRecord {
            id: id.clone(),
            scope: scope.clone(),
            collection: collection.clone(),
            description: description.clone(),
            created_at,
            token_hash: hash,
            prefix: prefix.clone(),
        };

        {
            let mut bh = self.by_hash.write().unwrap();
            let mut ih = self.id_to_hash.write().unwrap();
            bh.insert(hash, record);
            ih.insert(id.clone(), hash);
        }
        self.save();

        ApiKeyCreated { id, token: raw, scope, collection, description, created_at }
    }

    /// Revoke a key by ID.  Returns `true` if found and removed.
    pub fn revoke(&self, id: &str) -> bool {
        let mut ih = self.id_to_hash.write().unwrap();
        if let Some(hash) = ih.remove(id) {
            self.by_hash.write().unwrap().remove(&hash);
            drop(ih);
            self.save();
            true
        } else {
            false
        }
    }

    /// Snapshot of all keys for listing.
    pub fn list(&self) -> Vec<ApiKeyMasked> {
        let bh = self.by_hash.read().unwrap();
        let mut keys: Vec<ApiKeyMasked> = bh.values().map(ApiKeyMasked::from).collect();
        keys.sort_by_key(|k| k.created_at);
        keys
    }

    fn load(&self) {
        let Some(ref path) = self.path else { return };
        let Ok(data) = std::fs::read(path) else { return };
        let Ok(records) = serde_json::from_slice::<Vec<ApiKeyRecord>>(&data) else { return };
        let mut bh = self.by_hash.write().unwrap();
        let mut ih = self.id_to_hash.write().unwrap();
        for r in records {
            ih.insert(r.id.clone(), r.token_hash);
            bh.insert(r.token_hash, r);
        }
    }

    fn save(&self) {
        let Some(ref path) = self.path else { return };
        let bh = self.by_hash.read().unwrap();
        let records: Vec<&ApiKeyRecord> = bh.values().collect();
        let json = match serde_json::to_vec_pretty(&records) {
            Ok(j) => j,
            Err(e) => { tracing::error!("key store serialize failed: {e}"); return; }
        };
        // M-5: Atomic write (temp + rename) so a crash mid-write never corrupts the file.
        // Set 0600 permissions so other users on the system cannot read token hashes.
        let tmp = path.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, &json) {
            tracing::error!("key store write failed: {e}");
            return;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        if let Err(e) = std::fs::rename(&tmp, path) {
            tracing::error!("key store atomic rename failed: {e}");
        }
    }
}

// ── Scope classification (used by auth middleware) ─────────────────────────────

/// Determine the minimum scope required for a request based on method + path.
pub fn required_scope(method: &axum::http::Method, path: &str) -> ApiScope {
    // Admin-only: key management, snapshot operations, storage operations,
    // and replication endpoints (H-4: replication streams expose ALL namespaces).
    if path.starts_with("/v1/keys")
        || path.starts_with("/v1/snapshot")
        || path.starts_with("/v1/storage")
        || path.starts_with("/v1/replication")
    {
        return ApiScope::Admin;
    }
    // Read-only POSTs (search endpoints use POST for the query body).
    if path == "/search"
        || path.ends_with("/search")
        || path.starts_with("/v1/memory/search")
        || path.starts_with("/v1/proof")
        || path == "/timeline"
        || path == "/v1/timeline"
        || path == "/health"
        || path == "/metrics"
        || path == "/version"
    {
        return ApiScope::ReadOnly;
    }
    // GET is always read-only.
    if method == axum::http::Method::GET {
        return ApiScope::ReadOnly;
    }
    // All other POST/DELETE/PUT are write operations.
    ApiScope::ReadWrite
}

// ── Auth state (passed into middleware via closure) ────────────────────────────

pub struct AuthState {
    pub key_store: std::sync::Arc<KeyStore>,
    pub legacy_token: Option<String>,
}

impl AuthState {
    pub fn has_any_auth(&self) -> bool {
        self.legacy_token.is_some() || !self.key_store.is_empty()
    }
}

// ── Token utilities ───────────────────────────────────────────────────────────

fn bytes_to_hex(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hash_token(token: &str) -> [u8; 32] {
    *blake3::hash(token.as_bytes()).as_bytes()
}

fn generate_token() -> String {
    let raw = os_random_32();
    format!("vk_{}", bytes_to_hex(&raw))
}

fn simple_id() -> String {
    // Deterministic unique ID: BLAKE3 of token + time.
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let seed = os_random_32();
    let h = blake3::hash(&[&seed[..], &t.to_le_bytes()[..]].concat());
    format!("key_{}", &bytes_to_hex(h.as_bytes())[..16])
}

fn os_random_32() -> [u8; 32] {
    let mut buf = [0u8; 32];
    // getrandom uses the OS CSPRNG on all platforms (urandom, BCryptGenRandom,
    // getentropy, …). No fallback to time+pid (H-3).
    getrandom::getrandom(&mut buf)
        .expect("OS CSPRNG unavailable — cannot generate secure token");
    buf
}
