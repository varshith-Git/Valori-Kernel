// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `StorageProvider` — the logical-artifact storage abstraction (Phase 2,
//! collection-storage-foundation).
//!
//! # What this is, and is not
//!
//! This is deliberately **not** a generic filesystem trait
//! (`open`/`read`/`write`/`delete` over paths). Valori's storage layer
//! understands *artifacts* — a project manifest, a collection manifest, a
//! WAL segment, a collection snapshot — not files. Business logic never
//! builds a path; it builds a [`StorageKey`], and a [`StorageProvider`]
//! translates that logical identity into wherever the bytes actually live.
//!
//! ```text
//! LOGICAL ARTIFACT (StorageKey)         PHYSICAL LOCATION (provider-owned)
//! CollectionSnapshot{proj,coll,gen} →   local:  <root>/projects/<p>/collections/<c>/snapshots/generation-<g>
//!                                       s3:     s3://bucket/projects/<p>/collections/<c>/snapshots/generation-<g>  (future)
//! ```
//!
//! # Immutable vs. mutable artifacts
//!
//! Two write operations, not one, because Valori's artifacts fall into two
//! categories that must never be conflated:
//!
//! - **Immutable** ([`StorageProvider::put_immutable`]) — WAL segments,
//!   collection snapshots, (future) index artifacts. Once written, a given
//!   key is never rewritten; a new generation/segment is created instead.
//!   `put_immutable` on an existing key is a [`StorageError::AlreadyExists`],
//!   not a silent overwrite — this is what makes the same abstraction work
//!   unmodified against S3/ADLS later, where objects are naturally
//!   write-once-friendly and overwriting has surprising consistency
//!   properties.
//! - **Mutable** ([`StorageProvider::put_manifest`]) — project and
//!   collection manifests. These genuinely change (`latest_snapshot_generation`
//!   advances), but every write is still atomic: readers never observe a
//!   partially-written manifest.
//!
//! # Not implemented in this phase
//!
//! No S3/ADLS/GCS provider exists yet — only [`local::LocalStorageProvider`].
//! The existing `object_store.rs` (Phase 3.1, `opendal`-backed, already
//! supports `s3://`/`b2://`/`file://`) is the natural implementation vehicle
//! for a future `S3StorageProvider` — reusing already-integrated
//! infrastructure, not proposing a new one. See the phase report's "Future
//! S3/ADLS mapping" section.

pub mod local;

use std::fmt;

use valori_core::{NamespaceId, ShardId};
use valori_domain::ProjectId;

/// A logical artifact identity. Never a path — see the module doc.
///
/// Every variant is scoped by `project_id` because a project is the
/// deployment/storage isolation boundary (Phase 1). `IndexArtifact` is a
/// placeholder identity only — no index build system exists yet (that is a
/// later phase); it exists here so the storage abstraction doesn't need a
/// breaking change when one is built.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StorageKey {
    /// The project-level manifest — deployment/infrastructure metadata only
    /// (Phase 1: never vector configuration).
    ProjectManifest { project_id: ProjectId },
    /// One collection's durable manifest — see [`crate::collection_manifest::CollectionManifest`].
    CollectionManifest {
        project_id: ProjectId,
        collection_id: NamespaceId,
    },
    /// One sealed (or, while still being written, active) WAL segment on the
    /// shard-wide authoritative log. Segments are per-shard, never
    /// per-collection — see the module doc on WAL ownership.
    WalSegment {
        project_id: ProjectId,
        shard_id: ShardId,
        segment_seq: u64,
    },
    /// One immutable, self-contained snapshot of exactly one collection's
    /// state at a known LSN. See [`crate::collection_snapshot`].
    CollectionSnapshot {
        project_id: ProjectId,
        collection_id: NamespaceId,
        generation: u32,
    },
    /// Placeholder identity for a future ANN index artifact. Not built,
    /// not readable, not writable in this phase — ownership/lifecycle for
    /// this belongs to the index-lifecycle phase.
    IndexArtifact {
        project_id: ProjectId,
        collection_id: NamespaceId,
        index_type: String,
        generation: u32,
    },
}

impl fmt::Display for StorageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageKey::ProjectManifest { project_id } => {
                write!(f, "project-manifest/{project_id}")
            }
            StorageKey::CollectionManifest {
                project_id,
                collection_id,
            } => write!(f, "collection-manifest/{project_id}/{}", collection_id.0),
            StorageKey::WalSegment {
                project_id,
                shard_id,
                segment_seq,
            } => write!(
                f,
                "wal-segment/{project_id}/shard-{}/segment-{segment_seq:06}",
                shard_id.0
            ),
            StorageKey::CollectionSnapshot {
                project_id,
                collection_id,
                generation,
            } => write!(
                f,
                "collection-snapshot/{project_id}/{}/generation-{generation:06}",
                collection_id.0
            ),
            StorageKey::IndexArtifact {
                project_id,
                collection_id,
                index_type,
                generation,
            } => write!(
                f,
                "index-artifact/{project_id}/{}/{index_type}/generation-{generation:06}",
                collection_id.0
            ),
        }
    }
}

/// A `list()` query — always scoped to one artifact family within one
/// project, never a bare wildcard. This keeps the abstraction cheap for a
/// future object-store backend (a prefix list, not a full-bucket scan).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListPrefix {
    /// Every sealed+active WAL segment for one shard, in `segment_seq` order.
    WalSegments {
        project_id: ProjectId,
        shard_id: ShardId,
    },
    /// Every snapshot generation stored for one collection.
    CollectionSnapshots {
        project_id: ProjectId,
        collection_id: NamespaceId,
    },
    /// Every collection manifest under one project (collection discovery).
    CollectionManifests { project_id: ProjectId },
}

/// Metadata about a stored (or about-to-be-stored) artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArtifactMeta {
    pub size_bytes: u64,
    /// BLAKE3 digest of the artifact's bytes — storage-layer integrity,
    /// deliberately distinct from `valori_kernel::snapshot::blake3`'s
    /// deterministic *state* hash (see module doc on why these stay
    /// separate concepts).
    pub checksum: [u8; 32],
    pub created_at_unix: u64,
}

/// Errors a [`StorageProvider`] can return. Deliberately only the variants
/// this repository's callers actually need to distinguish — see the phase
/// spec's own instruction against a giant generic error enum.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("artifact not found: {0}")]
    NotFound(StorageKey),

    /// `put_immutable` was called for a key that already has bytes stored —
    /// immutable artifacts are never silently overwritten. Create a new
    /// generation/segment instead.
    #[error("immutable artifact already exists: {0}")]
    AlreadyExists(StorageKey),

    /// The stored bytes' BLAKE3 checksum does not match what was recorded
    /// at write time — bit rot, truncation, or tampering.
    #[error("checksum mismatch for {key}: recorded {recorded}, computed {computed}")]
    ChecksumMismatch {
        key: StorageKey,
        recorded: String,
        computed: String,
    },

    /// The bytes decoded but failed manifest-level validation (e.g. a
    /// negative/zero dimension, an unknown schema version inside a
    /// structurally valid envelope).
    #[error("invalid manifest for {key}: {reason}")]
    InvalidManifest { key: StorageKey, reason: String },

    /// The artifact's schema/format version is newer (or otherwise
    /// unrecognized) than this build of Valori understands.
    #[error("unsupported artifact version for {key}: {version}")]
    UnsupportedVersion { key: StorageKey, version: u32 },

    /// A concurrent writer already advanced a mutable artifact past the
    /// caller's expected state (optimistic-concurrency conflict on a
    /// manifest publish). Not used by `LocalStorageProvider` in this phase
    /// (single-writer-per-process today) — reserved for a future
    /// object-store provider where conditional-put races are real.
    #[error("conflict publishing {0}: concurrent writer advanced the artifact first")]
    Conflict(StorageKey),

    #[error("storage I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type StorageResult<T> = Result<T, StorageError>;

/// The logical storage abstraction every artifact family goes through.
///
/// Implementations own all physical-location detail (paths, buckets, object
/// keys) — see the module doc. Synchronous/blocking, matching every other
/// durability primitive in this crate (`EventLogWriter`, `WalWriter`); a
/// future async object-store provider can still implement this by blocking
/// on its own runtime internally, exactly as `object_store.rs` already does
/// for its `opendal::Operator` calls.
pub trait StorageProvider: Send + Sync {
    /// Write a NEW immutable artifact. Fails with
    /// [`StorageError::AlreadyExists`] if `key` already has bytes stored —
    /// never a silent overwrite.
    fn put_immutable(&self, key: &StorageKey, bytes: &[u8]) -> StorageResult<ArtifactMeta>;

    /// Publish (create or overwrite) a mutable manifest. Always atomic: a
    /// concurrent reader observes either the old bytes or the new bytes in
    /// full, never a partial write.
    fn put_manifest(&self, key: &StorageKey, bytes: &[u8]) -> StorageResult<ArtifactMeta>;

    /// Read an artifact's bytes, verifying its recorded checksum.
    fn get(&self, key: &StorageKey) -> StorageResult<Vec<u8>>;

    fn exists(&self, key: &StorageKey) -> StorageResult<bool>;

    fn stat(&self, key: &StorageKey) -> StorageResult<ArtifactMeta>;

    /// List every key matching `prefix`, ascending by the artifact's own
    /// ordering field (segment_seq / generation).
    fn list(&self, prefix: &ListPrefix) -> StorageResult<Vec<StorageKey>>;

    /// Delete an artifact. Used for garbage-collecting obsolete
    /// snapshot/index generations — never for the currently-referenced
    /// generation (callers are responsible for checking the manifest first).
    fn delete(&self, key: &StorageKey) -> StorageResult<()>;
}
