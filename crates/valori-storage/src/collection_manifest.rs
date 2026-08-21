// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `CollectionManifest` — the durable, per-collection materialization of
//! storage/recovery metadata (Phase 2, collection-storage-foundation).
//!
//! # This is a materialization, not a second source of truth
//!
//! The authoritative record of a collection's `dim`/`metric` is (per Phase
//! 1) `KernelState.namespace_configs`, committed via `KernelEvent::ConfigureNamespace`
//! and replicated/replayed exactly like every other event. `CollectionManifest`
//! is a **durable, quickly-loadable copy** of that fact plus storage-specific
//! bookkeeping (snapshot generation, base LSN) that has no other home —
//! nothing in `KernelState` tracks "which snapshot generation is on disk."
//!
//! `validate_against` exists specifically so a stale or hand-edited manifest
//! can never be trusted blindly — see its doc comment and Phase 2's spec
//! §11 ("manifest must not become a second source of truth").

use serde::{Deserialize, Serialize};
use valori_core::NamespaceId;
use valori_domain::{IndexKind, Metric};

use crate::provider::{StorageError, StorageKey};

/// A position in the shard-wide authoritative WAL. Equal to
/// `EventJournal.committed_height` in standalone mode and to openraft's
/// `LogId.index` in cluster mode — this is a formalization of an existing
/// concept, not a new counter (see the phase report's audit section).
///
/// LSNs are **shard-wide**, never per-collection (Phase 2 §6: one
/// authoritative WAL ordering; collections are identified inside events via
/// `NamespaceId`, not via a separate log).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Lsn(pub u64);

impl Lsn {
    pub const ZERO: Lsn = Lsn(0);

    pub const fn next(self) -> Lsn {
        Lsn(self.0 + 1)
    }
}

impl std::fmt::Display for Lsn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub const COLLECTION_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The durable, per-collection manifest. See the module doc for the
/// authority relationship with `KernelState`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionManifest {
    pub schema_version: u32,
    pub collection_id: NamespaceId,
    /// Required, immutable — Phase 1.
    pub dimension: u32,
    /// Required, immutable — Phase 1. Only `SquaredL2` exists today.
    pub metric: Metric,
    /// The most recent successfully-written, self-consistent snapshot
    /// generation for this collection. `None` if the collection has never
    /// been snapshotted (its state exists only via WAL replay so far).
    pub snapshot_generation: Option<u32>,
    /// The LSN that `snapshot_generation` (if any) was taken at — i.e.
    /// "this snapshot contains all of this collection's state through this
    /// WAL position." Recovery replays the authoritative WAL strictly after
    /// this LSN on top of the restored snapshot.
    pub snapshot_base_lsn: Lsn,
    /// The highest LSN this manifest has observed being applied for this
    /// collection (may be ahead of `snapshot_base_lsn` between snapshots).
    pub latest_wal_lsn: Lsn,
    /// The desired index algorithm, if any — a bare value today (Phase 1's
    /// `CollectionRegistry.index_kind` equivalent), **not a lifecycle**.
    /// `None` = index = NONE, a first-class supported state. The
    /// BUILDING/READY/ACTIVE/FAILED lifecycle belongs to the index-lifecycle
    /// phase; this field exists now only so the manifest schema doesn't need
    /// a breaking change when that phase lands.
    pub desired_index: Option<IndexKind>,

    // ── Phase 4.1: active index generation tracking ────────────────────────
    //
    // These three fields together locate the durable artifact for the
    // collection's currently ACTIVE ANN index.  All three must be `Some` at
    // the same time; a partial set is treated as "no active index artifact"
    // at recovery time.
    //
    // `#[serde(default)]` ensures manifests written before Phase 4.1 decode
    // cleanly (all three default to `None` / `Lsn::ZERO`).
    /// Generation number of the active index artifact (`StorageKey::IndexArtifact`).
    /// `None` ⟹ no durable ANN artifact exists; fall back to brute-force.
    #[serde(default)]
    pub active_index_generation: Option<u32>,

    /// Index algorithm name that was serialised into the artifact
    /// (`"hnsw"`, `"ivf"`, `"bq"`).  Used to instantiate the right
    /// `VectorIndex` implementation before calling `restore()`.
    #[serde(default)]
    pub active_index_type: Option<String>,

    /// The shard-wide WAL position captured at the moment the artifact was
    /// written.  Recovery checks: if `active_index_base_lsn == current_lsn`
    /// the artifact is used as-is; otherwise the index is rebuilt from the
    /// in-memory `KernelState` record set (correct because `KernelState` is
    /// always current after snapshot+WAL recovery).
    #[serde(default)]
    pub active_index_base_lsn: Lsn,
}

impl CollectionManifest {
    pub fn new(collection_id: NamespaceId, dimension: u32, metric: Metric) -> Self {
        Self {
            schema_version: COLLECTION_MANIFEST_SCHEMA_VERSION,
            collection_id,
            dimension,
            metric,
            snapshot_generation: None,
            snapshot_base_lsn: Lsn::ZERO,
            latest_wal_lsn: Lsn::ZERO,
            desired_index: None,
            active_index_generation: None,
            active_index_type: None,
            active_index_base_lsn: Lsn::ZERO,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("CollectionManifest serialization is infallible")
    }

    pub fn decode(key: &StorageKey, bytes: &[u8]) -> Result<Self, StorageError> {
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|e| StorageError::InvalidManifest {
                key: key.clone(),
                reason: format!("malformed JSON: {e}"),
            })?;
        if manifest.schema_version > COLLECTION_MANIFEST_SCHEMA_VERSION {
            return Err(StorageError::UnsupportedVersion {
                key: key.clone(),
                version: manifest.schema_version,
            });
        }
        if manifest.dimension == 0 {
            return Err(StorageError::InvalidManifest {
                key: key.clone(),
                reason: "dimension must be nonzero".to_string(),
            });
        }
        Ok(manifest)
    }

    /// Reject a manifest that disagrees with the authoritative, currently
    /// committed configuration for this collection (`KernelState.namespace_configs`,
    /// via the kernel's own resolved dim/metric). This is what makes loading
    /// the manifest at startup safe rather than "blindly trust stale
    /// metadata" (Phase 2 §11) — a mismatch here means the manifest was
    /// written for a different collection generation or has been corrupted
    /// in a way the checksum alone can't catch (e.g. hand-edited before the
    /// checksum sidecar was regenerated), and callers must not proceed as if
    /// it were correct.
    pub fn validate_against(
        &self,
        authoritative_dim: u32,
        authoritative_metric: Metric,
    ) -> Result<(), String> {
        if self.dimension != authoritative_dim {
            return Err(format!(
                "manifest dimension {} disagrees with authoritative kernel state dimension {authoritative_dim}",
                self.dimension
            ));
        }
        if self.metric != authoritative_metric {
            return Err(format!(
                "manifest metric {:?} disagrees with authoritative kernel state metric {authoritative_metric:?}",
                self.metric
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let m = CollectionManifest::new(NamespaceId(3), 768, Metric::SquaredL2);
        let key = StorageKey::CollectionManifest {
            project_id: valori_domain::ProjectId::new(),
            collection_id: NamespaceId(3),
        };
        let bytes = m.encode();
        let decoded = CollectionManifest::decode(&key, &bytes).unwrap();
        assert_eq!(decoded, m);
    }

    #[test]
    fn decode_rejects_corrupt_json() {
        let key = StorageKey::CollectionManifest {
            project_id: valori_domain::ProjectId::new(),
            collection_id: NamespaceId(0),
        };
        let err = CollectionManifest::decode(&key, b"{not json").unwrap_err();
        assert!(matches!(err, StorageError::InvalidManifest { .. }));
    }

    #[test]
    fn decode_rejects_zero_dimension() {
        let key = StorageKey::CollectionManifest {
            project_id: valori_domain::ProjectId::new(),
            collection_id: NamespaceId(0),
        };
        let bad = r#"{"schema_version":1,"collection_id":0,"dimension":0,"metric":"squared_l2","snapshot_generation":null,"snapshot_base_lsn":0,"latest_wal_lsn":0,"desired_index":null}"#;
        let err = CollectionManifest::decode(&key, bad.as_bytes()).unwrap_err();
        assert!(matches!(err, StorageError::InvalidManifest { .. }));
    }

    #[test]
    fn decode_rejects_newer_schema_version() {
        let key = StorageKey::CollectionManifest {
            project_id: valori_domain::ProjectId::new(),
            collection_id: NamespaceId(0),
        };
        let future = r#"{"schema_version":99,"collection_id":0,"dimension":384,"metric":"squared_l2","snapshot_generation":null,"snapshot_base_lsn":0,"latest_wal_lsn":0,"desired_index":null}"#;
        let err = CollectionManifest::decode(&key, future.as_bytes()).unwrap_err();
        assert!(matches!(err, StorageError::UnsupportedVersion { .. }));
    }

    #[test]
    fn validate_against_catches_dimension_drift() {
        let m = CollectionManifest::new(NamespaceId(0), 384, Metric::SquaredL2);
        assert!(m.validate_against(384, Metric::SquaredL2).is_ok());
        assert!(m.validate_against(768, Metric::SquaredL2).is_err());
    }

    #[test]
    fn lsn_ordering_and_next() {
        assert!(Lsn(1) < Lsn(2));
        assert_eq!(Lsn(5).next(), Lsn(6));
        assert_eq!(Lsn::ZERO.0, 0);
    }
}
