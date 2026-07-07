// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Snapshot catalog — durable record of every snapshot produced for a project.
//!
//! The catalog enables the object-store pruning policy (`VALORI_OBJECT_STORE_KEEP`)
//! and allows the bootstrap path to select the most recent snapshot without
//! scanning the filesystem.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Snapshot format version constants, matching `valori-kernel`'s snapshot encoder.
pub const SNAPSHOT_FORMAT_V5: u8 = 5;
pub const SNAPSHOT_FORMAT_V6: u8 = 6;

/// A single snapshot record in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    /// Unique identifier for this snapshot entry (ULID string).
    pub id: String,
    /// Name of the project this snapshot belongs to.
    pub project: String,
    /// Shard this snapshot covers (0-based).
    pub shard_id: u8,
    /// Absolute path to the snapshot file on disk.
    pub path: PathBuf,
    /// Snapshot file size in bytes at the time it was recorded.
    pub size_bytes: u64,
    /// Snapshot format version (5 = V5, 6 = V6, …).
    pub format_version: u8,
    /// Unix seconds when this snapshot was produced.
    pub produced_at: u64,
    /// BLAKE3 hash of `KernelState` at the time of the snapshot.
    pub state_hash: [u8; 32],
    /// Number of `KernelEvent`s applied before this snapshot.
    pub applied_height: u64,
}

impl SnapshotRecord {
    /// Returns `true` when the snapshot file still exists on disk.
    pub fn exists_on_disk(&self) -> bool {
        self.path.exists()
    }
}

/// An in-memory view of all snapshots for one (project, shard) pair,
/// ordered by `produced_at` ascending.
#[derive(Debug, Clone, Default)]
pub struct SnapshotCatalog {
    pub records: Vec<SnapshotRecord>,
}

impl SnapshotCatalog {
    /// Return the most recently produced snapshot, if any.
    pub fn latest(&self) -> Option<&SnapshotRecord> {
        self.records.iter().max_by_key(|r| r.produced_at)
    }

    /// Return the `keep` most recent records; everything else is purgeable.
    /// Returns the records that should be deleted.
    pub fn prunable(&self, keep: usize) -> Vec<&SnapshotRecord> {
        let mut sorted: Vec<_> = self.records.iter().collect();
        sorted.sort_by_key(|r| r.produced_at);
        if sorted.len() <= keep {
            return vec![];
        }
        sorted[..sorted.len() - keep].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(produced_at: u64) -> SnapshotRecord {
        SnapshotRecord {
            id: produced_at.to_string(),
            project: "p".into(),
            shard_id: 0,
            path: PathBuf::from("/tmp/x.snap"),
            size_bytes: 1024,
            format_version: SNAPSHOT_FORMAT_V6,
            produced_at,
            state_hash: [0u8; 32],
            applied_height: produced_at * 100,
        }
    }

    #[test]
    fn latest_returns_newest() {
        let cat = SnapshotCatalog {
            records: vec![record(100), record(300), record(200)],
        };
        assert_eq!(cat.latest().unwrap().produced_at, 300);
    }

    #[test]
    fn prunable_keeps_newest() {
        let cat = SnapshotCatalog {
            records: vec![record(1), record(2), record(3), record(4), record(5)],
        };
        let purgeable = cat.prunable(3);
        assert_eq!(purgeable.len(), 2);
        // oldest two should be purged
        assert!(purgeable.iter().any(|r| r.produced_at == 1));
        assert!(purgeable.iter().any(|r| r.produced_at == 2));
    }
}
