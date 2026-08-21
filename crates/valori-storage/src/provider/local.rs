// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `LocalStorageProvider` — the first (and, this phase, only)
//! [`StorageProvider`] implementation, backed by the local filesystem.
//!
//! Contains no local-disk-specific vocabulary in its public surface (no
//! `inode`/`directory handle`/etc. leaks out — see the parent module's
//! `StorageProvider` trait) and, symmetrically, no S3/ADLS vocabulary either
//! (no `bucket`/`ETag`/`multipart` — those belong to a future provider).
//! Everything filesystem-specific is private to this file.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{ArtifactMeta, ListPrefix, StorageError, StorageKey, StorageProvider, StorageResult};

/// Sidecar extension holding the BLAKE3 checksum of the artifact it sits
/// beside, as lowercase hex. Kept as a separate small file rather than a
/// header prefix inside the artifact bytes so every artifact format
/// (bincode WAL segments, the collection-snapshot format, plain JSON
/// manifests) gets integrity checking without each needing to reserve its
/// own header layout for it.
const CHECKSUM_EXT: &str = "b3";

pub struct LocalStorageProvider {
    root: PathBuf,
}

impl LocalStorageProvider {
    /// Open (creating if absent) a local storage root. Deterministic
    /// physical layout — see [`Self::physical_path`].
    pub fn open(root: impl Into<PathBuf>) -> StorageResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// The deterministic on-disk path for a logical key. Private: nothing
    /// outside this file may depend on this layout, which is the entire
    /// point of the abstraction.
    fn physical_path(&self, key: &StorageKey) -> PathBuf {
        match key {
            StorageKey::ProjectManifest { project_id } => self
                .root
                .join("projects")
                .join(project_id.to_string())
                .join("manifest")
                .join("project"),
            StorageKey::CollectionManifest {
                project_id,
                collection_id,
            } => self
                .root
                .join("projects")
                .join(project_id.to_string())
                .join("collections")
                .join(collection_id.0.to_string())
                .join("manifest")
                .join("collection"),
            StorageKey::WalSegment {
                project_id,
                shard_id,
                segment_seq,
            } => self
                .root
                .join("projects")
                .join(project_id.to_string())
                .join("shards")
                .join(shard_id.0.to_string())
                .join("wal")
                .join(format!("segment-{segment_seq:06}")),
            StorageKey::CollectionSnapshot {
                project_id,
                collection_id,
                generation,
            } => self
                .root
                .join("projects")
                .join(project_id.to_string())
                .join("collections")
                .join(collection_id.0.to_string())
                .join("snapshots")
                .join(format!("generation-{generation:06}")),
            StorageKey::IndexArtifact {
                project_id,
                collection_id,
                index_type,
                generation,
            } => self
                .root
                .join("projects")
                .join(project_id.to_string())
                .join("collections")
                .join(collection_id.0.to_string())
                .join("indexes")
                .join(format!("{index_type}-generation-{generation:06}")),
        }
    }

    fn checksum_path(path: &Path) -> PathBuf {
        let mut p = path.as_os_str().to_owned();
        p.push(".");
        p.push(CHECKSUM_EXT);
        PathBuf::from(p)
    }

    /// Write `bytes` to `path` atomically: write to a sibling temp file,
    /// fsync it, rename into place, then fsync the parent directory (the
    /// rename itself is only durable once the directory entry is synced —
    /// the same reasoning `valori-daemon`'s manifest writer and
    /// `EventLogWriter`'s rotation already apply). The checksum sidecar is
    /// written and renamed the same way, and — critically — written AFTER
    /// the artifact's own rename, so a crash can never leave a checksum
    /// file that describes bytes which don't exist yet.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> StorageResult<ArtifactMeta> {
        let parent = path.parent().expect("physical_path always has a parent");
        fs::create_dir_all(parent)?;

        let checksum = blake3::hash(bytes);

        let tmp_path = Self::tmp_path(path);
        {
            let mut f = fs::File::create(&tmp_path)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp_path, path)?;
        Self::fsync_dir(parent)?;

        let checksum_final = Self::checksum_path(path);
        let checksum_tmp = Self::tmp_path(&checksum_final);
        fs::write(&checksum_tmp, checksum.to_hex().as_bytes())?;
        fs::rename(&checksum_tmp, &checksum_final)?;
        Self::fsync_dir(parent)?;

        Ok(ArtifactMeta {
            size_bytes: bytes.len() as u64,
            checksum: *checksum.as_bytes(),
            created_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        })
    }

    fn tmp_path(path: &Path) -> PathBuf {
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let mut p = path.as_os_str().to_owned();
        p.push(format!(".tmp-{pid}-{nanos}"));
        PathBuf::from(p)
    }

    #[cfg(unix)]
    fn fsync_dir(dir: &Path) -> std::io::Result<()> {
        fs::File::open(dir)?.sync_all()
    }

    #[cfg(not(unix))]
    fn fsync_dir(_dir: &Path) -> std::io::Result<()> {
        // Directory-entry fsync isn't a portable operation on non-Unix
        // targets; the file-level sync_all above already covers the data
        // itself. Not a regression — the rest of this crate's durability
        // primitives (EventLogWriter) carry the same platform scope.
        Ok(())
    }

    fn read_and_verify(&self, key: &StorageKey, path: &Path) -> StorageResult<Vec<u8>> {
        let bytes = fs::read(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.clone())
            } else {
                StorageError::Io(e)
            }
        })?;

        let checksum_path = Self::checksum_path(path);
        if let Ok(recorded_hex) = fs::read_to_string(&checksum_path) {
            let computed = blake3::hash(&bytes);
            if recorded_hex.trim() != computed.to_hex().as_str() {
                return Err(StorageError::ChecksumMismatch {
                    key: key.clone(),
                    recorded: recorded_hex.trim().to_string(),
                    computed: computed.to_hex().to_string(),
                });
            }
        }
        // Absent sidecar (e.g. hand-placed test fixture) is tolerated, not
        // an error — the same "old data has no checksum, don't invent one"
        // stance the kernel snapshot format already takes.

        Ok(bytes)
    }
}

impl StorageProvider for LocalStorageProvider {
    fn put_immutable(&self, key: &StorageKey, bytes: &[u8]) -> StorageResult<ArtifactMeta> {
        let path = self.physical_path(key);
        if path.exists() {
            return Err(StorageError::AlreadyExists(key.clone()));
        }
        self.write_atomic(&path, bytes)
    }

    fn put_manifest(&self, key: &StorageKey, bytes: &[u8]) -> StorageResult<ArtifactMeta> {
        let path = self.physical_path(key);
        self.write_atomic(&path, bytes)
    }

    fn get(&self, key: &StorageKey) -> StorageResult<Vec<u8>> {
        let path = self.physical_path(key);
        self.read_and_verify(key, &path)
    }

    fn exists(&self, key: &StorageKey) -> StorageResult<bool> {
        Ok(self.physical_path(key).exists())
    }

    fn stat(&self, key: &StorageKey) -> StorageResult<ArtifactMeta> {
        let path = self.physical_path(key);
        let meta = fs::metadata(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.clone())
            } else {
                StorageError::Io(e)
            }
        })?;
        let bytes = self.read_and_verify(key, &path)?;
        let checksum = blake3::hash(&bytes);
        let created_at_unix = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Ok(ArtifactMeta {
            size_bytes: meta.len(),
            checksum: *checksum.as_bytes(),
            created_at_unix,
        })
    }

    fn list(&self, prefix: &ListPrefix) -> StorageResult<Vec<StorageKey>> {
        let (dir, mk_key): (PathBuf, Box<dyn Fn(u64) -> StorageKey>) = match prefix {
            ListPrefix::WalSegments {
                project_id,
                shard_id,
            } => {
                let dir = self
                    .root
                    .join("projects")
                    .join(project_id.to_string())
                    .join("shards")
                    .join(shard_id.0.to_string())
                    .join("wal");
                let project_id = *project_id;
                let shard_id = *shard_id;
                (
                    dir,
                    Box::new(move |seq| StorageKey::WalSegment {
                        project_id,
                        shard_id,
                        segment_seq: seq,
                    }),
                )
            }
            ListPrefix::CollectionSnapshots {
                project_id,
                collection_id,
            } => {
                let dir = self
                    .root
                    .join("projects")
                    .join(project_id.to_string())
                    .join("collections")
                    .join(collection_id.0.to_string())
                    .join("snapshots");
                let project_id = *project_id;
                let collection_id = *collection_id;
                (
                    dir,
                    Box::new(move |gen| StorageKey::CollectionSnapshot {
                        project_id,
                        collection_id,
                        generation: gen as u32,
                    }),
                )
            }
            ListPrefix::CollectionManifests { project_id } => {
                return self.list_collection_manifests(*project_id);
            }
        };

        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut ordinals: Vec<u64> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name();
                let name = name.to_str()?;
                if name.ends_with(&format!(".{CHECKSUM_EXT}")) || name.contains(".tmp-") {
                    return None;
                }
                name.rsplit('-').next()?.parse::<u64>().ok()
            })
            .collect();
        ordinals.sort_unstable();
        Ok(ordinals.into_iter().map(mk_key).collect())
    }

    fn delete(&self, key: &StorageKey) -> StorageResult<()> {
        let path = self.physical_path(key);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StorageError::NotFound(key.clone()))
            }
            Err(e) => return Err(StorageError::Io(e)),
        }
        let _ = fs::remove_file(Self::checksum_path(&path));
        Ok(())
    }
}

impl LocalStorageProvider {
    fn list_collection_manifests(
        &self,
        project_id: valori_domain::ProjectId,
    ) -> StorageResult<Vec<StorageKey>> {
        let dir = self
            .root
            .join("projects")
            .join(project_id.to_string())
            .join("collections");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut ids: Vec<u16> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str()?.parse::<u16>().ok())
            .collect();
        ids.sort_unstable();
        Ok(ids
            .into_iter()
            .map(|id| StorageKey::CollectionManifest {
                project_id,
                collection_id: valori_core::NamespaceId(id),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_core::{NamespaceId, ShardId};
    use valori_domain::ProjectId;

    fn provider() -> (LocalStorageProvider, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let p = LocalStorageProvider::open(dir.path()).unwrap();
        (p, dir)
    }

    #[test]
    fn put_get_exists_stat_delete_roundtrip() {
        let (p, _dir) = provider();
        let key = StorageKey::CollectionManifest {
            project_id: ProjectId::new(),
            collection_id: NamespaceId(3),
        };
        assert!(!p.exists(&key).unwrap());
        assert!(matches!(p.get(&key), Err(StorageError::NotFound(_))));

        let meta = p.put_manifest(&key, b"hello").unwrap();
        assert_eq!(meta.size_bytes, 5);
        assert!(p.exists(&key).unwrap());
        assert_eq!(p.get(&key).unwrap(), b"hello");

        let stat = p.stat(&key).unwrap();
        assert_eq!(stat.size_bytes, 5);
        assert_eq!(stat.checksum, meta.checksum);

        p.delete(&key).unwrap();
        assert!(!p.exists(&key).unwrap());
        assert!(matches!(p.delete(&key), Err(StorageError::NotFound(_))));
    }

    #[test]
    fn put_immutable_refuses_to_overwrite() {
        let (p, _dir) = provider();
        let key = StorageKey::CollectionSnapshot {
            project_id: ProjectId::new(),
            collection_id: NamespaceId(0),
            generation: 1,
        };
        p.put_immutable(&key, b"v1").unwrap();
        let err = p.put_immutable(&key, b"v2").unwrap_err();
        assert!(matches!(err, StorageError::AlreadyExists(_)));
        // Original bytes must be untouched by the refused write.
        assert_eq!(p.get(&key).unwrap(), b"v1");
    }

    #[test]
    fn put_manifest_allows_repeated_overwrite() {
        let (p, _dir) = provider();
        let key = StorageKey::ProjectManifest {
            project_id: ProjectId::new(),
        };
        p.put_manifest(&key, b"v1").unwrap();
        p.put_manifest(&key, b"v2").unwrap();
        p.put_manifest(&key, b"v3").unwrap();
        assert_eq!(p.get(&key).unwrap(), b"v3");
    }

    #[test]
    fn corrupted_bytes_are_detected_via_checksum() {
        let (p, dir) = provider();
        let project_id = ProjectId::new();
        let key = StorageKey::CollectionSnapshot {
            project_id,
            collection_id: NamespaceId(1),
            generation: 1,
        };
        p.put_immutable(&key, b"genuine bytes").unwrap();

        // Corrupt the artifact on disk directly, bypassing the provider —
        // simulates bit rot / a hand-edited file.
        let path = dir
            .path()
            .join("projects")
            .join(project_id.to_string())
            .join("collections")
            .join("1")
            .join("snapshots")
            .join("generation-000001");
        std::fs::write(&path, b"tampered!!!!!").unwrap();

        let err = p.get(&key).unwrap_err();
        assert!(matches!(err, StorageError::ChecksumMismatch { .. }));
    }

    #[test]
    fn list_returns_ascending_generations() {
        let (p, _dir) = provider();
        let project_id = ProjectId::new();
        let collection_id = NamespaceId(0);
        for gen in [3u32, 1, 2] {
            p.put_immutable(
                &StorageKey::CollectionSnapshot {
                    project_id,
                    collection_id,
                    generation: gen,
                },
                format!("gen{gen}").as_bytes(),
            )
            .unwrap();
        }
        let listed = p
            .list(&ListPrefix::CollectionSnapshots {
                project_id,
                collection_id,
            })
            .unwrap();
        let gens: Vec<u32> = listed
            .into_iter()
            .map(|k| match k {
                StorageKey::CollectionSnapshot { generation, .. } => generation,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(gens, vec![1, 2, 3]);
    }

    #[test]
    fn list_wal_segments_is_empty_for_unknown_shard() {
        let (p, _dir) = provider();
        let listed = p
            .list(&ListPrefix::WalSegments {
                project_id: ProjectId::new(),
                shard_id: ShardId(0),
            })
            .unwrap();
        assert!(listed.is_empty());
    }

    /// §18 of the phase spec: the "active segment / sealed segment"
    /// concept, expressed through the storage abstraction. A sealed
    /// segment is `put_immutable`'d once and never rewritten; "sealing
    /// segment N and opening segment N+1" is exactly "write a new,
    /// higher-numbered `WalSegment` key" — no special sealing operation is
    /// needed because immutability + monotonic segment numbers already
    /// express it. This does not replace the production
    /// `EventLogWriter`/`maybe_rotate` mechanism (which already rotates
    /// segments physically) — it demonstrates that the storage abstraction
    /// can represent that same lifecycle for a future migration.
    #[test]
    fn wal_segments_are_immutable_once_sealed_and_list_in_order() {
        let (p, _dir) = provider();
        let project_id = ProjectId::new();
        let shard_id = ShardId(0);

        for seq in 1..=3u64 {
            p.put_immutable(
                &StorageKey::WalSegment {
                    project_id,
                    shard_id,
                    segment_seq: seq,
                },
                format!("segment {seq} sealed bytes").as_bytes(),
            )
            .unwrap();
        }

        // Segment 2 is sealed — it must never be rewritten in place.
        let err = p
            .put_immutable(
                &StorageKey::WalSegment {
                    project_id,
                    shard_id,
                    segment_seq: 2,
                },
                b"attempted rewrite",
            )
            .unwrap_err();
        assert!(matches!(err, StorageError::AlreadyExists(_)));

        // "Opening segment 4" is just writing a new, higher key — no
        // special API, matching the module doc's framing.
        p.put_immutable(
            &StorageKey::WalSegment {
                project_id,
                shard_id,
                segment_seq: 4,
            },
            b"segment 4 (active)",
        )
        .unwrap();

        let listed = p
            .list(&ListPrefix::WalSegments {
                project_id,
                shard_id,
            })
            .unwrap();
        let seqs: Vec<u64> = listed
            .into_iter()
            .map(|k| match k {
                StorageKey::WalSegment { segment_seq, .. } => segment_seq,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(seqs, vec![1, 2, 3, 4]);
    }

    #[test]
    fn collection_a_and_b_snapshots_are_independently_addressable() {
        let (p, _dir) = provider();
        let project_id = ProjectId::new();
        let a = StorageKey::CollectionSnapshot {
            project_id,
            collection_id: NamespaceId(1),
            generation: 1,
        };
        let b = StorageKey::CollectionSnapshot {
            project_id,
            collection_id: NamespaceId(2),
            generation: 1,
        };
        p.put_immutable(&a, b"collection A bytes, dim 384").unwrap();
        p.put_immutable(&b, b"collection B bytes, dim 768").unwrap();

        // Reading B must never return A's bytes, and vice versa — the
        // mandatory "Collection A cannot accidentally restore Collection B"
        // isolation guarantee, exercised at the raw storage layer.
        assert_eq!(p.get(&a).unwrap(), b"collection A bytes, dim 384");
        assert_eq!(p.get(&b).unwrap(), b"collection B bytes, dim 768");
    }
}
