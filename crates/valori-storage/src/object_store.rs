// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Object-store backend for Phase 3.1 — snapshot offload and WAL archival.
//!
//! Supports S3 (and any S3-compatible service — MinIO, Localstack, R2) via
//! `s3://bucket/prefix`, and a local filesystem backend via `file:///path`
//! for dev/test without cloud credentials.
//!
//! ## Auth
//!
//! S3 credentials are resolved in priority order by the underlying opendal
//! AWS credential chain:
//!   1. `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` env vars
//!   2. IAM instance profile / EKS pod identity
//!   3. `~/.aws/credentials` file
//!
//! No Valori-specific credential management is needed — just attach the right
//! IAM role in production and set env vars in dev/CI.

use bytes::Bytes;
use opendal::Operator;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ObjectStoreError {
    #[error("build error: {0}")]
    Build(String),
    #[error("I/O error: {0}")]
    Io(String),
}

impl From<opendal::Error> for ObjectStoreError {
    fn from(e: opendal::Error) -> Self {
        Self::Io(e.to_string())
    }
}

// ── Returned types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct SnapshotEntry {
    /// Full object key (e.g. `"prefix/snapshots/00000001750000000_abc12345.snap"`).
    pub key: String,
    /// Hex BLAKE3 state hash recorded alongside the snapshot.
    pub state_hash: String,
    /// Unix epoch seconds extracted from the key name — used for sorting.
    pub epoch_secs: u64,
    /// Snapshot size in bytes.
    pub size_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WalEntry {
    /// Full object key.
    pub key: String,
    /// Segment size in bytes.
    pub size_bytes: u64,
}

// ── Backend ───────────────────────────────────────────────────────────────────

pub struct ObjectStoreBackend {
    op: Operator,
    /// Optional key prefix — everything is stored under `{prefix}/snapshots/`
    /// and `{prefix}/wal/`. Empty string means store at root.
    prefix: String,
}

impl ObjectStoreBackend {
    /// Build from `VALORI_OBJECT_STORE_URL`.  Returns `None` if the env var is
    /// absent (object store disabled), logs + returns `None` on bad config.
    pub fn from_env() -> Option<Arc<Self>> {
        let url = std::env::var("VALORI_OBJECT_STORE_URL").ok()?;
        match Self::from_url(&url) {
            Ok(b) => {
                tracing::info!("object store configured: {url}");
                Some(Arc::new(b))
            }
            Err(e) => {
                tracing::error!("object store init failed for {url}: {e}");
                None
            }
        }
    }

    /// Build from a URL string.
    ///
    /// Supported formats:
    /// - `s3://bucket-name/optional/prefix`
    /// - `s3://bucket-name`
    /// - `file:///absolute/path`
    /// - `file://relative/path`
    pub fn from_url(url: &str) -> Result<Self, ObjectStoreError> {
        if let Some(rest) = url.strip_prefix("s3://") {
            let (bucket, prefix) = if let Some(slash) = rest.find('/') {
                (
                    &rest[..slash],
                    rest[slash + 1..].trim_end_matches('/').to_string(),
                )
            } else {
                (rest, String::new())
            };

            // Region: prefer VALORI-specific override, then standard AWS vars.
            let region = std::env::var("VALORI_OBJECT_STORE_REGION")
                .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                .or_else(|_| std::env::var("AWS_REGION"))
                .unwrap_or_else(|_| "us-east-1".to_string());

            // Build with method chaining (opendal builder methods move Self).
            let mut builder = opendal::services::S3::default()
                .bucket(bucket)
                .region(&region);

            // Custom endpoint for MinIO / Localstack / Cloudflare R2.
            if let Ok(endpoint) = std::env::var("VALORI_OBJECT_STORE_ENDPOINT") {
                builder = builder.endpoint(&endpoint);
            }

            // Explicit credentials (override the credential chain — useful in tests).
            if let (Ok(key), Ok(secret)) = (
                std::env::var("AWS_ACCESS_KEY_ID"),
                std::env::var("AWS_SECRET_ACCESS_KEY"),
            ) {
                builder = builder.access_key_id(&key).secret_access_key(&secret);
            }

            let op = Operator::new(builder).map_err(|e| ObjectStoreError::Build(e.to_string()))?;
            Ok(Self { op, prefix })
        } else if let Some(root) = url.strip_prefix("file://") {
            std::fs::create_dir_all(root)
                .map_err(|e| ObjectStoreError::Build(format!("create_dir_all {root}: {e}")))?;

            let builder = opendal::services::Fs::default().root(root);

            let op = Operator::new(builder).map_err(|e| ObjectStoreError::Build(e.to_string()))?;
            Ok(Self {
                op,
                prefix: String::new(),
            })
        } else {
            Err(ObjectStoreError::Build(format!(
                "unsupported object-store URL (want s3:// or file://): {url}"
            )))
        }
    }

    // ── Key helpers ───────────────────────────────────────────────────────────

    fn full_key(&self, folder: &str, name: &str) -> String {
        if self.prefix.is_empty() {
            format!("{folder}/{name}")
        } else {
            format!("{}/{folder}/{name}", self.prefix)
        }
    }

    fn snap_dir(&self) -> String {
        if self.prefix.is_empty() {
            "snapshots/".to_string()
        } else {
            format!("{}/snapshots/", self.prefix)
        }
    }

    fn wal_dir(&self) -> String {
        if self.prefix.is_empty() {
            "wal/".to_string()
        } else {
            format!("{}/wal/", self.prefix)
        }
    }

    // ── Snapshot operations ───────────────────────────────────────────────────

    /// Upload `data` to object store.  Writes two objects:
    ///
    /// - `snapshots/{epoch}_{hash8}.snap` — the raw snapshot binary
    /// - `snapshots/{epoch}_{hash8}.hash` — the hex state hash (for verification)
    ///
    /// Returns the `.snap` object key.
    pub async fn upload_snapshot(
        &self,
        data: &[u8],
        state_hash: &str,
    ) -> Result<String, ObjectStoreError> {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let hash_tag = if state_hash.len() >= 8 {
            &state_hash[..8]
        } else {
            state_hash
        };
        let snap_key = self.full_key("snapshots", &format!("{epoch:020}_{hash_tag}.snap"));
        let hash_key = snap_key.replace(".snap", ".hash");

        self.op
            .write(&snap_key, Bytes::copy_from_slice(data))
            .await?;
        self.op
            .write(&hash_key, Bytes::copy_from_slice(state_hash.as_bytes()))
            .await?;

        tracing::info!(key = %snap_key, bytes = data.len(), "snapshot uploaded to object store");
        Ok(snap_key)
    }

    /// List all snapshots, sorted newest-first.
    pub async fn list_snapshots(&self) -> Result<Vec<SnapshotEntry>, ObjectStoreError> {
        let dir = self.snap_dir();
        let entries = match self.op.list(&dir).await {
            Ok(e) => e,
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };

        let mut snaps: Vec<SnapshotEntry> = Vec::new();
        for entry in &entries {
            let path = entry.path();
            if !path.ends_with(".snap") {
                continue;
            }
            let size_bytes = self
                .op
                .stat(path)
                .await
                .map(|m| m.content_length())
                .unwrap_or(0);
            let hash_key = path.replace(".snap", ".hash");
            let state_hash = self
                .op
                .read(&hash_key)
                .await
                .map(|b| String::from_utf8_lossy(&b.to_vec()).trim().to_string())
                .unwrap_or_default();
            // Key name: `{prefix}/snapshots/{epoch:020}_{hash8}.snap`
            let fname = path.rsplit('/').next().unwrap_or(path);
            let epoch_secs = fname
                .split('_')
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            snaps.push(SnapshotEntry {
                key: path.to_string(),
                state_hash,
                epoch_secs,
                size_bytes,
            });
        }
        snaps.sort_by(|a, b| b.epoch_secs.cmp(&a.epoch_secs)); // newest first
        Ok(snaps)
    }

    /// Download snapshot bytes by key.
    pub async fn download_snapshot(&self, key: &str) -> Result<Vec<u8>, ObjectStoreError> {
        let buf = self.op.read(key).await?;
        Ok(buf.to_vec())
    }

    /// Delete oldest snapshots, keeping the `keep` most recent.
    /// Returns the number of snapshots deleted.
    pub async fn prune_snapshots(&self, keep: usize) -> Result<usize, ObjectStoreError> {
        let mut snaps = self.list_snapshots().await?;
        snaps.sort_by_key(|s| s.epoch_secs); // oldest first
        let to_delete = snaps.len().saturating_sub(keep);
        for entry in &snaps[..to_delete] {
            if let Err(e) = self.op.delete(&entry.key).await {
                tracing::warn!("failed to delete old snapshot {}: {e}", entry.key);
            }
            let hash_key = entry.key.replace(".snap", ".hash");
            self.op.delete(&hash_key).await.ok();
        }
        tracing::info!("pruned {to_delete} old snapshot(s) from object store (keep={keep})");
        Ok(to_delete)
    }

    // ── WAL operations ────────────────────────────────────────────────────────

    /// Upload a sealed WAL segment (`events.log.000001`, etc.) to object storage.
    ///
    /// The segment is read from disk and uploaded to `wal/{filename}`.
    /// Returns the object key.
    pub async fn archive_wal_segment(&self, local_path: &Path) -> Result<String, ObjectStoreError> {
        let name = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ObjectStoreError::Io(format!("invalid path: {:?}", local_path)))?;
        let key = self.full_key("wal", name);
        let data = std::fs::read(local_path)
            .map_err(|e| ObjectStoreError::Io(format!("read {local_path:?}: {e}")))?;
        self.op.write(&key, Bytes::from(data)).await?;
        tracing::info!(key = %key, "WAL segment archived to object store");
        Ok(key)
    }

    /// List archived WAL segments, sorted by name (= segment sequence order).
    pub async fn list_wal_segments(&self) -> Result<Vec<WalEntry>, ObjectStoreError> {
        let dir = self.wal_dir();
        let entries = match self.op.list(&dir).await {
            Ok(e) => e,
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };

        let mut result: Vec<WalEntry> = Vec::new();
        for entry in &entries {
            let path = entry.path();
            let size_bytes = self
                .op
                .stat(path)
                .await
                .map(|m| m.content_length())
                .unwrap_or(0);
            result.push(WalEntry {
                key: path.to_string(),
                size_bytes,
            });
        }
        result.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(result)
    }
}
