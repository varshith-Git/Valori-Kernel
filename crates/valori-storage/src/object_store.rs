// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Object-store backend for Phase 3.1 — snapshot offload and WAL archival.
//!
//! ## Backends
//!
//! | URL | Service |
//! |---|---|
//! | `s3://bucket/prefix` | AWS S3 — or any S3-compatible service (MinIO, Localstack, Cloudflare R2) once `VALORI_OBJECT_STORE_ENDPOINT` points at it. |
//! | `b2://bucket/prefix` | Backblaze B2, via its S3-compatible API. |
//! | `file:///path` | Local filesystem — dev/test without cloud credentials. |
//!
//! ### Why B2 goes through the S3 API, not opendal's native `services-b2`
//!
//! Backblaze's own recommendation is the S3-compatible API, and it's the
//! better fit here for concrete reasons, not just convention: the native B2
//! API additionally needs a `bucket_id` (an opaque value distinct from the
//! bucket NAME, requiring a separate lookup to obtain), whereas the S3 API
//! takes the name directly — so `b2://my-bucket` can mean what it looks
//! like it means. It also keeps one code path for every S3-compatible
//! service, so a bug fixed for AWS is fixed for B2 too.
//!
//! `b2://` is therefore a thin alias over the same S3 client, existing for
//! exactly one reason: it derives the endpoint from the region
//! (`https://s3.{region}.backblazeb2.com`) instead of making the operator
//! spell it out. A wrong endpoint/region pairing is the most common way a
//! B2 setup fails, and it fails with an opaque signing error that says
//! nothing about the real cause.
//!
//! ## Auth
//!
//! Credentials resolve in priority order via the opendal AWS credential
//! chain, for every S3-compatible backend including B2:
//!   1. `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` env vars
//!   2. IAM instance profile / EKS pod identity
//!   3. `~/.aws/credentials` file
//!
//! For **AWS**, attach the right IAM role in production and set env vars in
//! dev/CI — no Valori-specific credential management needed.
//!
//! For **B2**, create an S3-compatible application key and set its
//! `keyID` as `AWS_ACCESS_KEY_ID` and `applicationKey` as
//! `AWS_SECRET_ACCESS_KEY`. B2 issues them in exactly that shape. Only
//! steps 1 and 3 of the chain apply — there's no instance-profile
//! equivalent.

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

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalEntry {
    /// Full object key.
    pub key: String,
    /// Segment size in bytes.
    pub size_bytes: u64,
}

/// Current schema version for [`SnapshotManifest`]. Bump only when the
/// manifest's own field shape changes in a way old readers can't tolerate
/// (field removed/renamed, not just added) — same policy the wire-format
/// version constants elsewhere in this codebase already follow.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
/// `manifest.json` — the entry point for disaster recovery. Written
/// alongside every snapshot upload (see [`ObjectStoreBackend::
/// upload_snapshot_and_update_manifest`]), it names the ONE snapshot that
/// is current (out of however many timestamped `.snap` objects exist under
/// `snapshots/` — old ones aren't deleted until `prune_snapshots` runs) plus
/// the WAL segments archived since, so a restore tool has a single object
/// to fetch instead of listing-and-sorting `snapshots/`/`wal/` and hoping
/// the newest filename really is the right one.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotManifest {
    pub schema_version: u32,
    /// `CARGO_PKG_VERSION` of whatever wrote this manifest (valori-node,
    /// normally) — lets a restore tool detect "this snapshot was written by
    /// an older/newer node than the one about to restore it."
    pub node_version: String,
    /// `None` only if a manifest was written before any snapshot ever
    /// succeeded — shouldn't happen in practice since
    /// `upload_snapshot_and_update_manifest` always has a just-uploaded
    /// snapshot to point at, but kept optional rather than a fabricated
    /// placeholder entry.
    pub current_snapshot: Option<SnapshotEntry>,
    pub wal_segments: Vec<WalEntry>,
    /// Unix epoch seconds when this manifest was last written.
    pub updated_at: u64,
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

    /// Splits `bucket/optional/prefix` into its two halves. Trailing
    /// slashes on the prefix are stripped so `full_key` never produces a
    /// doubled separator.
    fn split_bucket_prefix(rest: &str) -> (&str, String) {
        match rest.find('/') {
            Some(slash) => (
                &rest[..slash],
                rest[slash + 1..].trim_end_matches('/').to_string(),
            ),
            None => (rest, String::new()),
        }
    }

    /// Backblaze B2's S3-compatible endpoint for a region. B2 regions look
    /// like `us-west-004` / `eu-central-003` (the numeric suffix is part of
    /// the region, not a typo).
    fn b2_endpoint(region: &str) -> String {
        format!("https://s3.{region}.backblazeb2.com")
    }

    /// Shared S3 builder for every S3-compatible backend — real AWS S3,
    /// Backblaze B2, MinIO, Localstack, Cloudflare R2. They differ only in
    /// endpoint and region, never in protocol.
    fn s3_operator(
        bucket: &str,
        region: &str,
        endpoint: Option<&str>,
    ) -> Result<Operator, ObjectStoreError> {
        // Method chaining — opendal builder methods move Self.
        let mut builder = opendal::services::S3::default()
            .bucket(bucket)
            .region(region);

        if let Some(endpoint) = endpoint {
            builder = builder.endpoint(endpoint);
        }

        // Explicit credentials override the credential chain. For B2 these
        // are the application keyID / applicationKey from an S3-compatible
        // key — B2 issues them in exactly this shape, which is the whole
        // reason its S3 API is preferable to its native one here.
        if let (Ok(key), Ok(secret)) = (
            std::env::var("AWS_ACCESS_KEY_ID"),
            std::env::var("AWS_SECRET_ACCESS_KEY"),
        ) {
            builder = builder.access_key_id(&key).secret_access_key(&secret);
        }

        Operator::new(builder).map_err(|e| ObjectStoreError::Build(e.to_string()))
    }

    /// Region from `VALORI_OBJECT_STORE_REGION`, falling back to the
    /// standard AWS vars.
    fn configured_region() -> Option<String> {
        std::env::var("VALORI_OBJECT_STORE_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .or_else(|_| std::env::var("AWS_REGION"))
            .ok()
            .filter(|s| !s.is_empty())
    }

    /// Build from a URL string.
    ///
    /// Supported formats:
    /// - `s3://bucket-name/optional/prefix` — AWS S3, or any S3-compatible
    ///   service when `VALORI_OBJECT_STORE_ENDPOINT` is set (MinIO,
    ///   Localstack, Cloudflare R2).
    /// - `b2://bucket-name/optional/prefix` — Backblaze B2 via its
    ///   S3-compatible API. Identical protocol to `s3://`; the only reason
    ///   it's its own scheme is that the endpoint is derived from the
    ///   region (`https://s3.{region}.backblazeb2.com`) instead of having
    ///   to be spelled out, which is the single most common way a B2 setup
    ///   is misconfigured. `VALORI_OBJECT_STORE_ENDPOINT` still overrides
    ///   if you need to point somewhere else.
    /// - `file:///absolute/path`
    /// - `file://relative/path`
    pub fn from_url(url: &str) -> Result<Self, ObjectStoreError> {
        if let Some(rest) = url.strip_prefix("s3://") {
            let (bucket, prefix) = Self::split_bucket_prefix(rest);
            // us-east-1 is the conventional default AWS itself assumes for
            // a region-less client, and is harmless for S3-compatible
            // services that ignore the field entirely (MinIO).
            let region = Self::configured_region().unwrap_or_else(|| "us-east-1".to_string());
            let endpoint = std::env::var("VALORI_OBJECT_STORE_ENDPOINT")
                .ok()
                .filter(|s| !s.is_empty());
            let op = Self::s3_operator(bucket, &region, endpoint.as_deref())?;
            Ok(Self { op, prefix })
        } else if let Some(rest) = url.strip_prefix("b2://") {
            let (bucket, prefix) = Self::split_bucket_prefix(rest);

            // No default region for B2, unlike S3: every B2 endpoint is
            // region-specific, so guessing one would produce a client that
            // authenticates against the wrong host and fails with a signing
            // error that says nothing about the real cause. Fail here, with
            // the fix in the message.
            let region = Self::configured_region().ok_or_else(|| {
                ObjectStoreError::Build(
                    "b2:// requires VALORI_OBJECT_STORE_REGION (e.g. us-west-004) — \
                     find it in the Backblaze bucket's Endpoint field, which reads \
                     s3.<region>.backblazeb2.com"
                        .to_string(),
                )
            })?;

            let endpoint = std::env::var("VALORI_OBJECT_STORE_ENDPOINT")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| Self::b2_endpoint(&region));

            let op = Self::s3_operator(bucket, &region, Some(&endpoint))?;
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
                "unsupported object-store URL (want s3://, b2://, or file://): {url}"
            )))
        }
    }

    /// Write-then-read a small canary object and delete it again. Used at
    /// node startup to fail fast (before the node accepts traffic) if the
    /// configured bucket/credentials/region are wrong, rather than only
    /// discovering it hours later when the first scheduled snapshot upload
    /// silently fails.
    pub async fn check_connectivity(&self) -> Result<(), ObjectStoreError> {
        let key = if self.prefix.is_empty() {
            ".valori-healthcheck".to_string()
        } else {
            format!("{}/.valori-healthcheck", self.prefix)
        };
        let payload = format!(
            "valori healthcheck {}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );

        self.op
            .write(&key, Bytes::from(payload.clone().into_bytes()))
            .await?;

        let read_back = self.op.read(&key).await?;
        if read_back.to_vec() != payload.into_bytes() {
            return Err(ObjectStoreError::Io(
                "healthcheck read-back did not match what was written".to_string(),
            ));
        }

        self.op.delete(&key).await?;
        Ok(())
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

    fn manifest_key(&self) -> String {
        if self.prefix.is_empty() {
            "manifest.json".to_string()
        } else {
            format!("{}/manifest.json", self.prefix)
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

    /// Uploads a snapshot exactly like [`Self::upload_snapshot`], then
    /// rewrites `manifest.json` to point at it — this is what callers
    /// should use going forward (see `valori-node`'s `upload_snapshot_to_
    /// store` handler); `upload_snapshot` alone is kept because
    /// `finish_shadow`'s blue/green restore path only ever reads a key it
    /// already has and never needs the manifest.
    ///
    /// The manifest's WAL list is whatever `list_wal_segments` currently
    /// reports — best-effort (an empty list on error rather than failing
    /// the whole upload over a WAL-listing hiccup).
    pub async fn upload_snapshot_and_update_manifest(
        &self,
        data: &[u8],
        state_hash: &str,
        node_version: &str,
    ) -> Result<SnapshotEntry, ObjectStoreError> {
        let key = self.upload_snapshot(data, state_hash).await?;
        let epoch_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let entry = SnapshotEntry {
            key,
            state_hash: state_hash.to_string(),
            epoch_secs,
            size_bytes: data.len() as u64,
        };

        let wal_segments = self.list_wal_segments().await.unwrap_or_default();
        self.write_manifest(Some(&entry), wal_segments, node_version)
            .await?;

        Ok(entry)
    }

    /// Overwrites `manifest.json` wholesale — not a merge/patch. Callers
    /// that only changed one field (e.g. a fresh WAL archive, snapshot
    /// unchanged) must pass the current `current_snapshot` back in.
    pub async fn write_manifest(
        &self,
        current_snapshot: Option<&SnapshotEntry>,
        wal_segments: Vec<WalEntry>,
        node_version: &str,
    ) -> Result<(), ObjectStoreError> {
        let manifest = SnapshotManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            node_version: node_version.to_string(),
            current_snapshot: current_snapshot.cloned(),
            wal_segments,
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let bytes = serde_json::to_vec(&manifest)
            .map_err(|e| ObjectStoreError::Io(format!("encoding manifest.json: {e}")))?;
        self.op
            .write(&self.manifest_key(), Bytes::from(bytes))
            .await?;
        tracing::info!(
            snapshot_key = ?manifest.current_snapshot.as_ref().map(|s| &s.key),
            wal_segments = manifest.wal_segments.len(),
            "manifest.json updated"
        );
        Ok(())
    }

    /// `Ok(None)` if no manifest has ever been written (a store that only
    /// ever used the older bare `upload_snapshot`, or a brand-new bucket) —
    /// distinct from an error, since callers that fall back to listing +
    /// sorting `snapshots/` on a missing manifest need to tell "not written
    /// yet" apart from "object store unreachable."
    pub async fn read_manifest(&self) -> Result<Option<SnapshotManifest>, ObjectStoreError> {
        match self.op.read(&self.manifest_key()).await {
            Ok(bytes) => {
                let manifest: SnapshotManifest = serde_json::from_slice(&bytes.to_vec())
                    .map_err(|e| ObjectStoreError::Io(format!("decoding manifest.json: {e}")))?;
                Ok(Some(manifest))
            }
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
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
        snaps.sort_by_key(|s| std::cmp::Reverse(s.epoch_secs)); // newest first
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

#[cfg(test)]
mod tests {
    use super::*;

    // file:// stands in for S3 here (same equivalence the module doc comment
    // already draws) — no network/credentials needed to exercise the same
    // opendal write/read/delete code path a real S3 healthcheck would take.

    #[tokio::test]
    async fn check_connectivity_succeeds_against_a_writable_backend() {
        let dir = tempfile::tempdir().unwrap();
        let backend =
            ObjectStoreBackend::from_url(&format!("file://{}", dir.path().display())).unwrap();

        assert!(backend.check_connectivity().await.is_ok());
    }

    #[tokio::test]
    async fn check_connectivity_leaves_no_canary_object_behind() {
        let dir = tempfile::tempdir().unwrap();
        let backend =
            ObjectStoreBackend::from_url(&format!("file://{}", dir.path().display())).unwrap();

        backend.check_connectivity().await.unwrap();

        assert!(!dir.path().join(".valori-healthcheck").exists());
    }

    // Not run as root (e.g. inside some Docker CI images) — root bypasses
    // Unix directory permissions, which would make this false-fail instead
    // of exercising the "storage unreachable" path it's testing.
    #[cfg(unix)]
    #[tokio::test]
    async fn check_connectivity_fails_against_an_unwritable_backend() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("readonly");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();

        let backend = ObjectStoreBackend::from_url(&format!("file://{}", root.display())).unwrap();
        let result = backend.check_connectivity().await;

        // Restore write perms first so tempdir's Drop can remove it,
        // regardless of what the assertion below does.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).ok();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn upload_snapshot_then_list_finds_it() {
        // End-to-end proxy for "insert vectors -> wait -> snapshot appears
        // in S3": upload_snapshot is exactly what the scheduled backup sweep
        // calls (see valori-node's upload_snapshot_to_store handler), and
        // list_snapshots is exactly what backup/mod.rs's BackupService polls
        // to confirm an upload landed.
        let dir = tempfile::tempdir().unwrap();
        let backend =
            ObjectStoreBackend::from_url(&format!("file://{}", dir.path().display())).unwrap();

        let key = backend
            .upload_snapshot(b"fake snapshot bytes", "deadbeef")
            .await
            .unwrap();

        let snaps = backend.list_snapshots().await.unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].key, key);
        assert_eq!(snaps[0].state_hash, "deadbeef");

        let downloaded = backend.download_snapshot(&key).await.unwrap();
        assert_eq!(downloaded, b"fake snapshot bytes");
    }

    // ── URL parsing ───────────────────────────────────────────────────────
    // These don't touch the network: `Operator::new` only builds a client.

    #[test]
    fn split_bucket_prefix_handles_both_shapes() {
        assert_eq!(
            ObjectStoreBackend::split_bucket_prefix("my-bucket"),
            ("my-bucket", String::new())
        );
        assert_eq!(
            ObjectStoreBackend::split_bucket_prefix("my-bucket/projects/abc"),
            ("my-bucket", "projects/abc".to_string())
        );
        // Trailing slash stripped so full_key never doubles the separator.
        assert_eq!(
            ObjectStoreBackend::split_bucket_prefix("my-bucket/projects/abc/"),
            ("my-bucket", "projects/abc".to_string())
        );
    }

    #[test]
    fn b2_endpoint_is_derived_from_the_region() {
        // B2 regions carry a numeric suffix — it's part of the region, not
        // a typo, and it must appear in the endpoint verbatim.
        assert_eq!(
            ObjectStoreBackend::b2_endpoint("us-west-004"),
            "https://s3.us-west-004.backblazeb2.com"
        );
        assert_eq!(
            ObjectStoreBackend::b2_endpoint("eu-central-003"),
            "https://s3.eu-central-003.backblazeb2.com"
        );
    }

    #[test]
    fn unsupported_scheme_names_every_supported_one() {
        let Err(err) = ObjectStoreBackend::from_url("gs://bucket") else {
            panic!("gs:// must not be accepted");
        };
        let msg = err.to_string();
        assert!(msg.contains("s3://"), "{msg}");
        assert!(msg.contains("b2://"), "{msg}");
        assert!(msg.contains("file://"), "{msg}");
    }

    #[test]
    fn b2_without_a_region_fails_with_an_actionable_message() {
        // Guessing a region would build a client that signs against the
        // wrong host and fails with an opaque error — see from_url.
        // SAFETY: no other test in this binary reads these vars.
        unsafe {
            std::env::remove_var("VALORI_OBJECT_STORE_REGION");
            std::env::remove_var("AWS_DEFAULT_REGION");
            std::env::remove_var("AWS_REGION");
        }

        let Err(err) = ObjectStoreBackend::from_url("b2://my-bucket/prefix") else {
            panic!("b2:// without a region must not be accepted");
        };
        let msg = err.to_string();
        assert!(msg.contains("VALORI_OBJECT_STORE_REGION"), "{msg}");
        assert!(msg.contains("backblazeb2.com"), "{msg}");
    }

    #[tokio::test]
    async fn read_manifest_is_none_before_anything_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let backend =
            ObjectStoreBackend::from_url(&format!("file://{}", dir.path().display())).unwrap();

        assert!(backend.read_manifest().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn upload_snapshot_and_update_manifest_makes_it_the_entry_point() {
        let dir = tempfile::tempdir().unwrap();
        let backend =
            ObjectStoreBackend::from_url(&format!("file://{}", dir.path().display())).unwrap();

        let entry = backend
            .upload_snapshot_and_update_manifest(b"snap bytes", "cafebabe", "9.9.9")
            .await
            .unwrap();

        let manifest = backend
            .read_manifest()
            .await
            .unwrap()
            .expect("manifest must exist");
        assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
        assert_eq!(manifest.node_version, "9.9.9");
        let current = manifest
            .current_snapshot
            .expect("must name a current snapshot");
        assert_eq!(current.key, entry.key);
        assert_eq!(current.state_hash, "cafebabe");
        assert!(manifest.wal_segments.is_empty());
        assert!(manifest.updated_at > 0);
    }

    #[tokio::test]
    async fn second_upload_makes_manifest_point_at_the_newer_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let backend =
            ObjectStoreBackend::from_url(&format!("file://{}", dir.path().display())).unwrap();

        let first = backend
            .upload_snapshot_and_update_manifest(b"v1", "aaaa1111", "1.0.0")
            .await
            .unwrap();
        let second = backend
            .upload_snapshot_and_update_manifest(b"v2", "bbbb2222", "1.0.0")
            .await
            .unwrap();
        assert_ne!(
            first.key, second.key,
            "two uploads must not collide on the same key"
        );

        let manifest = backend.read_manifest().await.unwrap().unwrap();
        assert_eq!(manifest.current_snapshot.unwrap().key, second.key);

        // Both snapshots still exist — versioned, not overwritten.
        let snaps = backend.list_snapshots().await.unwrap();
        assert_eq!(snaps.len(), 2);
    }

    #[tokio::test]
    async fn write_manifest_includes_wal_segments() {
        let dir = tempfile::tempdir().unwrap();
        let backend =
            ObjectStoreBackend::from_url(&format!("file://{}", dir.path().display())).unwrap();

        let wal_dir = dir.path().join("wal-src");
        std::fs::create_dir_all(&wal_dir).unwrap();
        let seg_path = wal_dir.join("events.log.000001");
        std::fs::write(&seg_path, b"segment bytes").unwrap();
        backend.archive_wal_segment(&seg_path).await.unwrap();

        let entry = backend
            .upload_snapshot_and_update_manifest(b"snap", "deadbeef", "1.0.0")
            .await
            .unwrap();

        let manifest = backend.read_manifest().await.unwrap().unwrap();
        assert_eq!(manifest.current_snapshot.unwrap().key, entry.key);
        // Not asserting an exact count here — deliberately loose w.r.t.
        // list_wal_segments's own listing behavior (a pre-existing,
        // unrelated concern of that method, not this manifest feature).
        assert!(
            manifest
                .wal_segments
                .iter()
                .any(|s| s.key.ends_with("events.log.000001")),
            "expected the archived segment in the manifest's wal_segments: {:?}",
            manifest.wal_segments
        );
    }
}
