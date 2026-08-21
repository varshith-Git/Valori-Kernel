// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Collection-scoped recovery orchestration (Phase 2,
//! collection-storage-foundation) — the `StorageProvider`-backed sibling of
//! [`crate::bootstrap`]'s whole-process event-log/WAL/snapshot recovery.
//!
//! # Recovery flow (Phase 2.1 spec §10) — the live protocol as of this phase
//!
//! ```text
//! 1. Discover Collections          discover_collections() — ListPrefix::CollectionManifests
//! 2. Load Collection manifests     CollectionManifest::decode() per collection
//! 3. For every Collection:
//!      a. locate snapshot            manifest.snapshot_generation
//!      b. validate snapshot          checksum (StorageProvider::get), schema version
//!      c. restore materialized state configure_namespace + collect records
//! 4. Determine replay starting point  min(snapshot_base_lsn) across collections
//!    (a never-snapshotted collection contributes Lsn(0) — replay everything
//!    for it, per §6's "manifest missing -> no hidden fallback" instruction)
//! 5. Stream authoritative WAL in order  read_events_after_lsn() — ONE read
//!    at the global minimum, not one read per collection (§10's "correctness
//!    over optimization" plus the explicit minimum-base_lsn preference)
//! 6. Skip events already covered by the EVENT'S OWN namespace's snapshot
//!    (abs_lsn <= that namespace's manifest.snapshot_base_lsn) — never a
//!    global skip threshold, which would silently drop a not-yet-snapshotted
//!    collection's earlier events (§9's mandatory per-namespace filtering)
//! 7. Apply later events, strictly in the WAL's own order (§8: never restore
//!    "all of A, then all of B" — see collection_snapshot::restore_project_into's
//!    doc comment for why RecordId ordering makes that generally wrong)
//! 8. Return the reconstructed KernelState + the highest LSN actually reached
//! ```
//!
//! # Engine integration status
//!
//! `recover_project_with_wal_tail` below is REAL and is what
//! `valori_engine::Engine::try_recover` calls when a node has been
//! configured with a `StorageProvider` + `ProjectId`
//! (`Engine::configure_storage_provider`) — see that method's doc comment.
//! It is not yet the *default*: `valori-node`'s `NodeConfig`/`main.rs` do
//! not yet construct and inject a `StorageProvider` for a normal
//! `VALORI_EVENT_LOG_PATH`-configured node, so today's live deployments
//! still take the pre-existing whole-process path. This is the explicit,
//! bounded compatibility path the phase spec allows — see the phase
//! report's Known Limitations for exactly what remains to flip the default.

use std::collections::HashMap;
use std::path::Path;

use valori_core::{NamespaceId, ShardId};
use valori_domain::{Metric, ProjectId, ProjectName, ProjectTopology};
use valori_kernel::state::kernel::KernelState;
use valori_storage::collection_manifest::{CollectionManifest, Lsn};
use valori_storage::collection_snapshot::{
    self, CollectionSnapshotEdge, CollectionSnapshotMeta, CollectionSnapshotNode,
    CollectionSnapshotRecord,
};
use valori_storage::events::event_replay::stream_events_from_provider;
use valori_storage::project_manifest::ProjectManifest;
use valori_storage::provider::{ListPrefix, StorageKey, StorageProvider};

use crate::error::StateResult;

/// Publish (create or overwrite) the project-level manifest — the
/// discovery root every recovery starts from (Phase 2.2 §5/§6). Never
/// carries `dimension`/`metric`/`index` — see `ProjectManifest`'s own doc
/// comment.
pub fn publish_project_manifest(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
    name: ProjectName,
    topology: ProjectTopology,
    created_at_unix: u64,
) -> StateResult<()> {
    let manifest = ProjectManifest::new(project_id, name, topology, created_at_unix);
    provider.put_manifest(
        &StorageKey::ProjectManifest { project_id },
        &manifest.encode(),
    )?;
    Ok(())
}

/// Read the project manifest, if published. `None` means "this project was
/// never initialized through the storage-provider path" — genuinely
/// different from an initialized-but-empty project (§27: an empty project
/// must mean there was genuinely no durable state, never a swallowed
/// failure).
pub fn discover_project(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
) -> StateResult<Option<ProjectManifest>> {
    let key = StorageKey::ProjectManifest { project_id };
    match provider.get(&key) {
        Ok(bytes) => Ok(Some(ProjectManifest::decode(&key, &bytes)?)),
        Err(valori_storage::provider::StorageError::NotFound(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Which generation of which collection to restore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectionRecoverySpec {
    pub collection_id: NamespaceId,
    pub generation: u32,
}

fn kernel_metric(m: Metric) -> valori_kernel::index::Metric {
    valori_kernel::index::Metric::from_u8(m.as_u8())
        .expect("valori_domain::Metric and valori_kernel::index::Metric share the same wire tags")
}

/// Restore multiple collections' snapshots into one unified `KernelState`.
pub fn recover_project_from_snapshots(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
    specs: &[CollectionRecoverySpec],
) -> StateResult<KernelState> {
    let mut state = KernelState::new();
    let mut per_collection: Vec<(
        CollectionSnapshotMeta,
        Vec<CollectionSnapshotRecord>,
        Vec<CollectionSnapshotNode>,
        Vec<CollectionSnapshotEdge>,
    )> = Vec::with_capacity(specs.len());

    for spec in specs {
        let key = StorageKey::CollectionSnapshot {
            project_id,
            collection_id: spec.collection_id,
            generation: spec.generation,
        };
        let bytes = provider.get(&key)?;
        let (meta, records, nodes, edges) = collection_snapshot::decode(&key, &bytes)?;

        state.configure_namespace(
            spec.collection_id.0,
            meta.dimension,
            kernel_metric(meta.metric),
            0,
        )?;
        per_collection.push((meta, records, nodes, edges));
    }

    let refs: Vec<(
        &CollectionSnapshotMeta,
        &[CollectionSnapshotRecord],
        &[CollectionSnapshotNode],
        &[CollectionSnapshotEdge],
    )> = per_collection
        .iter()
        .map(|(meta, recs, nodes, edges)| {
            (meta, recs.as_slice(), nodes.as_slice(), edges.as_slice())
        })
        .collect();
    collection_snapshot::restore_project_into(&mut state, &refs)?;

    Ok(state)
}

/// Take and durably persist a snapshot of one collection's current state,
/// as the next generation, recording `base_lsn` (the shard-wide WAL
/// position this snapshot is consistent as-of).
/// §14/§15's mandatory ordering: write the immutable snapshot bytes FIRST
/// (durable — `put_immutable` only returns `Ok` after the atomic
/// write+fsync+rename+checksum sequence completes), and only THEN publish a
/// manifest that points at it. If the process crashes between these two
/// calls, the manifest still points at the previous (still-valid)
/// generation — the new, unreferenced snapshot bytes are simply orphaned,
/// safely garbage-collectable later, never mistaken for "current."
///
/// Reads the collection's existing manifest first (published at collection
/// creation — see `publish_collection_manifest`) and republishes it with
/// the advanced `snapshot_generation`/`snapshot_base_lsn`/`latest_wal_lsn`;
/// if no manifest exists yet (a collection snapshotted before this phase's
/// manifest-publication wiring existed), one is created fresh rather than
/// failing — snapshotting must never be blocked by a missing manifest for a
/// collection whose dim/metric we already know from `state` itself.
pub fn snapshot_collection(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
    state: &KernelState,
    collection_id: NamespaceId,
    generation: u32,
    base_lsn: Lsn,
    metric: Metric,
) -> StateResult<()> {
    let Some((meta, records, nodes, edges)) = collection_snapshot::extract_from_kernel_state(
        state,
        collection_id,
        generation,
        base_lsn,
        metric,
    ) else {
        return Err(crate::error::StateError::InvalidInput(format!(
            "collection {} has no known dimension — nothing to snapshot",
            collection_id.0
        )));
    };
    let bytes = collection_snapshot::encode(&meta, &records, &nodes, &edges);
    let snapshot_key = StorageKey::CollectionSnapshot {
        project_id,
        collection_id,
        generation,
    };
    // Step 1: durable, immutable snapshot bytes — must succeed before the
    // manifest is ever touched.
    provider.put_immutable(&snapshot_key, &bytes)?;

    // Step 2: publish the manifest pointing at the now-durable snapshot.
    let manifest_key = StorageKey::CollectionManifest {
        project_id,
        collection_id,
    };
    let mut manifest = match provider.get(&manifest_key) {
        Ok(existing_bytes) => CollectionManifest::decode(&manifest_key, &existing_bytes)?,
        Err(_) => CollectionManifest::new(collection_id, meta.dimension, metric),
    };
    manifest.snapshot_generation = Some(generation);
    manifest.snapshot_base_lsn = base_lsn;
    if manifest.latest_wal_lsn < base_lsn {
        manifest.latest_wal_lsn = base_lsn;
    }
    provider.put_manifest(&manifest_key, &manifest.encode())?;

    Ok(())
}

/// Publish a fresh manifest for a newly-created collection — no snapshot
/// yet (`snapshot_generation: None`), just the required, immutable
/// dim/metric. Called from the live collection-creation path
/// (`valori_engine::Engine::create_collection_with_config`, when a storage
/// provider is configured) so `CollectionManifest` genuinely materializes
/// live writes instead of only existing in isolated tests (Phase 2's
/// disclosed gap #2).
pub fn publish_collection_manifest(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
    collection_id: NamespaceId,
    dimension: u32,
    metric: Metric,
) -> StateResult<()> {
    let manifest = CollectionManifest::new(collection_id, dimension, metric);
    let key = StorageKey::CollectionManifest {
        project_id,
        collection_id,
    };
    provider.put_manifest(&key, &manifest.encode())?;
    Ok(())
}

/// Discover every collection with a published manifest under `project_id`.
/// This is what makes recovery independent of a caller who "happens to
/// know" which collections exist (Phase 2.1 §6) — the manifest itself is
/// the source of collection discovery.
pub fn discover_collections(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
) -> StateResult<Vec<CollectionManifest>> {
    let keys = provider.list(&ListPrefix::CollectionManifests { project_id })?;
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        let bytes = provider.get(&key)?;
        out.push(CollectionManifest::decode(&key, &bytes)?);
    }
    Ok(out)
}

/// Recover a project from `StorageProvider` by restoring snapshots and streaming logical WAL segments.
pub fn recover_project_from_storage(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
    shard_id: ShardId,
    active_wal_path: Option<&Path>,
) -> StateResult<(KernelState, Lsn)> {
    discover_project(provider, project_id)?;

    let manifests = discover_collections(provider, project_id)?;

    let mut state = KernelState::new();
    let mut per_collection: Vec<(
        CollectionSnapshotMeta,
        Vec<CollectionSnapshotRecord>,
        Vec<CollectionSnapshotNode>,
        Vec<CollectionSnapshotEdge>,
    )> = Vec::new();
    let mut manifest_by_ns: HashMap<u16, CollectionManifest> = HashMap::new();
    let mut min_base_lsn = Lsn(u64::MAX);

    for m in &manifests {
        state.configure_namespace(m.collection_id.0, m.dimension, kernel_metric(m.metric), 0)?;

        let base_lsn = if let Some(generation) = m.snapshot_generation {
            let key = StorageKey::CollectionSnapshot {
                project_id,
                collection_id: m.collection_id,
                generation,
            };
            let bytes = provider.get(&key)?;
            let (snap_meta, records, nodes, edges) = collection_snapshot::decode(&key, &bytes)?;
            per_collection.push((snap_meta, records, nodes, edges));
            m.snapshot_base_lsn
        } else {
            Lsn(0)
        };
        if base_lsn < min_base_lsn {
            min_base_lsn = base_lsn;
        }
        manifest_by_ns.insert(m.collection_id.0, m.clone());
    }
    if manifests.is_empty() {
        min_base_lsn = Lsn(0);
    }

    let refs: Vec<(
        &CollectionSnapshotMeta,
        &[CollectionSnapshotRecord],
        &[CollectionSnapshotNode],
        &[CollectionSnapshotEdge],
    )> = per_collection
        .iter()
        .map(|(meta, recs, nodes, edges)| {
            (meta, recs.as_slice(), nodes.as_slice(), edges.as_slice())
        })
        .collect();
    collection_snapshot::restore_project_into(&mut state, &refs)?;

    // Stream WAL tail through StorageProvider's sealed segments and active WAL
    let tail = stream_events_from_provider(
        provider,
        project_id,
        shard_id,
        None,
        min_base_lsn.0,
        active_wal_path,
    )?;

    let mut highest_lsn = min_base_lsn;
    for (i, (ns, event)) in tail.iter().enumerate() {
        let abs_lsn = Lsn(min_base_lsn.0 + i as u64 + 1);
        if let Some(m) = manifest_by_ns.get(ns) {
            if abs_lsn.0 <= m.snapshot_base_lsn.0 {
                continue;
            }
        }
        state.apply_event_ns(event, *ns)?;
        highest_lsn = abs_lsn;
    }

    Ok((state, highest_lsn))
}

/// The full manifest-driven snapshot + WAL-tail recovery protocol.
pub fn recover_project_with_wal_tail(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
    wal_log_path: &Path,
) -> StateResult<(KernelState, Lsn)> {
    let active_path = if wal_log_path.exists() {
        Some(wal_log_path)
    } else {
        None
    };
    recover_project_from_storage(provider, project_id, ShardId(0), active_path)
}

/// Seal an already-rotated (archived) WAL segment file into the
/// `StorageProvider` as an immutable artifact — the standalone piece of
/// §16's "integrate production rotation with StorageProvider" that does
/// NOT require modifying `EventCommitter::maybe_rotate`'s internals
/// (see the phase report's Known Limitations for why: no hook point
/// exists yet in that hot commit path to call this automatically). A
/// caller (e.g. `Engine`, after observing a rotation) reads the just-sealed
/// archive file's bytes and calls this to make it durable in the logical
/// storage model, without touching the archival mechanism itself — segment
/// sequence, hash chaining, CRC, and format versioning inside the bytes are
/// entirely untouched; this only adds a second, additive durability copy.
pub fn seal_wal_segment_to_storage(
    provider: &dyn StorageProvider,
    project_id: ProjectId,
    shard_id: valori_core::ShardId,
    segment_seq: u64,
    sealed_segment_bytes: &[u8],
) -> StateResult<()> {
    let key = StorageKey::WalSegment {
        project_id,
        shard_id,
        segment_seq,
    };
    provider.put_immutable(&key, sealed_segment_bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::event::KernelEvent;
    use valori_kernel::types::id::RecordId;
    use valori_kernel::types::scalar::FxpScalar;
    use valori_kernel::types::vector::FxpVector;
    use valori_storage::provider::local::LocalStorageProvider;

    #[test]
    fn discover_project_is_none_when_never_published() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        assert!(discover_project(&provider, ProjectId::new())
            .unwrap()
            .is_none());
    }

    #[test]
    fn publish_then_discover_project_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();
        publish_project_manifest(
            &provider,
            project_id,
            ProjectName::parse("demo").unwrap(),
            ProjectTopology::STANDALONE,
            1_000,
        )
        .unwrap();
        let found = discover_project(&provider, project_id).unwrap().unwrap();
        assert_eq!(found.project_id, project_id);
    }

    /// §27: a CORRUPT project manifest (durable state exists but can't be
    /// trusted) must fail recovery loudly — never silently proceed as if
    /// the project were fresh/empty.
    #[test]
    fn corrupt_project_manifest_fails_recovery_safely() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();
        publish_project_manifest(
            &provider,
            project_id,
            ProjectName::parse("demo").unwrap(),
            ProjectTopology::STANDALONE,
            1_000,
        )
        .unwrap();

        let path = dir
            .path()
            .join("projects")
            .join(project_id.to_string())
            .join("manifest")
            .join("project");
        std::fs::write(&path, b"not a valid project manifest").unwrap();

        let wal_path = dir.path().join("nonexistent.log");
        let result = recover_project_with_wal_tail(&provider, project_id, &wal_path);
        assert!(
            result.is_err(),
            "recovery must fail on a corrupt project manifest, not proceed as if fresh"
        );
    }

    fn seed(state: &mut KernelState, ns: u16, dim: usize, count: u32) {
        state
            .configure_namespace(ns, dim as u32, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();
        for i in 0..count {
            let id = state.next_record_id();
            let data = (0..dim)
                .map(|d| FxpScalar((i * 10 + d as u32) as i32))
                .collect();
            state
                .apply_event_ns(
                    &KernelEvent::InsertRecord {
                        id,
                        vector: FxpVector { data },
                        metadata: None,
                        tag: 0,
                    },
                    ns,
                )
                .unwrap();
        }
    }

    /// §31 of the phase spec: the critical future-server-migration scenario.
    /// "Worker A" writes state and persists it; "Worker B" (simulated here
    /// as a second, independent `KernelState` built ONLY from the
    /// `StorageProvider`) must reproduce it exactly, with zero access to
    /// Worker A's in-memory state.
    #[test]
    fn worker_b_restores_from_storage_alone_after_worker_a_disappears() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();

        // Worker A: two mixed-dimension collections, interleaved inserts.
        let worker_a_state = {
            let mut state = KernelState::new();
            state
                .configure_namespace(1, 384, valori_kernel::index::Metric::SquaredL2, 0)
                .unwrap();
            state
                .configure_namespace(2, 768, valori_kernel::index::Metric::SquaredL2, 0)
                .unwrap();
            // Interleaved: A, B, A, B — exercises the global-id-ordering
            // requirement `restore_project_into` exists for.
            for round in 0..3u32 {
                for (ns, dim) in [(1u16, 384usize), (2, 768)] {
                    let id = state.next_record_id();
                    let data = (0..dim)
                        .map(|d| FxpScalar((round * 100 + d as u32) as i32))
                        .collect();
                    state
                        .apply_event_ns(
                            &KernelEvent::InsertRecord {
                                id,
                                vector: FxpVector { data },
                                metadata: Some(vec![round as u8]),
                                tag: round as u64,
                            },
                            ns,
                        )
                        .unwrap();
                }
            }
            state
        };
        assert_eq!(worker_a_state.record_count(), 6);

        // Worker A persists — snapshot both collections at the same LSN
        // (natural given one shard-wide WAL, per the phase spec's §7).
        snapshot_collection(
            &provider,
            project_id,
            &worker_a_state,
            NamespaceId(1),
            1,
            Lsn(6),
            Metric::SquaredL2,
        )
        .unwrap();
        snapshot_collection(
            &provider,
            project_id,
            &worker_a_state,
            NamespaceId(2),
            1,
            Lsn(6),
            Metric::SquaredL2,
        )
        .unwrap();

        // Worker A "disappears" — dropped, no reference survives.
        drop(worker_a_state);

        // Worker B: a NEW process (simulated: a function call with no
        // access to any prior variable), restoring purely from the
        // StorageProvider.
        let worker_b_state = recover_project_from_snapshots(
            &provider,
            project_id,
            &[
                CollectionRecoverySpec {
                    collection_id: NamespaceId(1),
                    generation: 1,
                },
                CollectionRecoverySpec {
                    collection_id: NamespaceId(2),
                    generation: 1,
                },
            ],
        )
        .unwrap();

        assert_eq!(worker_b_state.record_count(), 6);
        assert_eq!(worker_b_state.namespace_dim(1), Some(384));
        assert_eq!(worker_b_state.namespace_dim(2), Some(768));
        // Every record Worker A wrote is present with the same content —
        // continuity across a total worker replacement.
        for i in 0..6u32 {
            let rec = worker_b_state.get_record(RecordId(i)).unwrap();
            assert_eq!(rec.namespace_id, if i % 2 == 0 { 1 } else { 2 });
        }
    }

    #[test]
    fn recovery_restores_three_mixed_dimension_collections() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();

        let mut state = KernelState::new();
        seed(&mut state, 1, 384, 4);
        seed(&mut state, 2, 768, 4);
        seed(&mut state, 3, 1536, 4);

        for (ns, dim) in [(1u16, 384u32), (2, 768), (3, 1536)] {
            let _ = dim;
            snapshot_collection(
                &provider,
                project_id,
                &state,
                NamespaceId(ns),
                1,
                Lsn(12),
                Metric::SquaredL2,
            )
            .unwrap();
        }

        let restored = recover_project_from_snapshots(
            &provider,
            project_id,
            &[
                CollectionRecoverySpec {
                    collection_id: NamespaceId(1),
                    generation: 1,
                },
                CollectionRecoverySpec {
                    collection_id: NamespaceId(2),
                    generation: 1,
                },
                CollectionRecoverySpec {
                    collection_id: NamespaceId(3),
                    generation: 1,
                },
            ],
        )
        .unwrap();

        assert_eq!(restored.record_count(), 12);
        assert_eq!(restored.namespace_dim(1), Some(384));
        assert_eq!(restored.namespace_dim(2), Some(768));
        assert_eq!(restored.namespace_dim(3), Some(1536));
    }

    #[test]
    fn collection_a_snapshot_cannot_be_restored_as_collection_b() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();

        let mut state = KernelState::new();
        seed(&mut state, 1, 384, 2);
        snapshot_collection(
            &provider,
            project_id,
            &state,
            NamespaceId(1),
            1,
            Lsn(2),
            Metric::SquaredL2,
        )
        .unwrap();

        // There is no snapshot for collection 2 (generation 1) — attempting
        // to "restore B" by asking for a key that was never written for B
        // must fail loudly (NotFound), never silently substitute A's bytes.
        let err = recover_project_from_snapshots(
            &provider,
            project_id,
            &[CollectionRecoverySpec {
                collection_id: NamespaceId(2),
                generation: 1,
            }],
        )
        .err()
        .unwrap();
        assert!(err.to_string().to_lowercase().contains("not found"));
    }

    #[test]
    fn corrupted_snapshot_fails_recovery_safely() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();

        let mut state = KernelState::new();
        seed(&mut state, 1, 384, 2);
        snapshot_collection(
            &provider,
            project_id,
            &state,
            NamespaceId(1),
            1,
            Lsn(2),
            Metric::SquaredL2,
        )
        .unwrap();

        // Corrupt the artifact on disk directly.
        let path = dir
            .path()
            .join("projects")
            .join(project_id.to_string())
            .join("collections")
            .join("1")
            .join("snapshots")
            .join("generation-000001");
        std::fs::write(&path, b"not a valid collection snapshot at all").unwrap();

        let result = recover_project_from_snapshots(
            &provider,
            project_id,
            &[CollectionRecoverySpec {
                collection_id: NamespaceId(1),
                generation: 1,
            }],
        );
        assert!(
            result.is_err(),
            "recovery must fail safely on a corrupted artifact, not load garbage"
        );
    }

    // ── Phase 2.1: manifest lifecycle ───────────────────────────────────────

    #[test]
    fn manifest_written_on_collection_creation() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();

        publish_collection_manifest(
            &provider,
            project_id,
            NamespaceId(1),
            384,
            Metric::SquaredL2,
        )
        .unwrap();

        let discovered = discover_collections(&provider, project_id).unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].collection_id, NamespaceId(1));
        assert_eq!(discovered[0].dimension, 384);
        assert_eq!(discovered[0].snapshot_generation, None, "no snapshot yet");
    }

    #[test]
    fn manifest_updated_after_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();

        publish_collection_manifest(
            &provider,
            project_id,
            NamespaceId(1),
            384,
            Metric::SquaredL2,
        )
        .unwrap();

        let mut state = KernelState::new();
        seed(&mut state, 1, 384, 3);
        snapshot_collection(
            &provider,
            project_id,
            &state,
            NamespaceId(1),
            1,
            Lsn(3),
            Metric::SquaredL2,
        )
        .unwrap();

        let key = StorageKey::CollectionManifest {
            project_id,
            collection_id: NamespaceId(1),
        };
        let manifest = CollectionManifest::decode(&key, &provider.get(&key).unwrap()).unwrap();
        assert_eq!(manifest.snapshot_generation, Some(1));
        assert_eq!(manifest.snapshot_base_lsn, Lsn(3));

        // A second snapshot advances the generation, never regresses it.
        for i in 0..2u32 {
            let id = state.next_record_id();
            state
                .apply_event_ns(
                    &KernelEvent::InsertRecord {
                        id,
                        vector: FxpVector {
                            data: (0..384).map(|_| FxpScalar(0)).collect(),
                        },
                        metadata: None,
                        tag: i as u64,
                    },
                    1,
                )
                .unwrap();
        }
        snapshot_collection(
            &provider,
            project_id,
            &state,
            NamespaceId(1),
            2,
            Lsn(5),
            Metric::SquaredL2,
        )
        .unwrap();
        let manifest = CollectionManifest::decode(&key, &provider.get(&key).unwrap()).unwrap();
        assert_eq!(manifest.snapshot_generation, Some(2));
        assert_eq!(manifest.snapshot_base_lsn, Lsn(5));
    }

    #[test]
    fn manifest_points_only_to_durable_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();
        let mut state = KernelState::new();
        seed(&mut state, 1, 4, 1);

        snapshot_collection(
            &provider,
            project_id,
            &state,
            NamespaceId(1),
            1,
            Lsn(1),
            Metric::SquaredL2,
        )
        .unwrap();

        // The manifest's referenced generation must actually exist as a
        // durable artifact — never a dangling pointer.
        let key = StorageKey::CollectionManifest {
            project_id,
            collection_id: NamespaceId(1),
        };
        let manifest = CollectionManifest::decode(&key, &provider.get(&key).unwrap()).unwrap();
        let snap_key = StorageKey::CollectionSnapshot {
            project_id,
            collection_id: NamespaceId(1),
            generation: manifest.snapshot_generation.unwrap(),
        };
        assert!(provider.exists(&snap_key).unwrap());
    }

    #[test]
    fn discovery_of_zero_collections_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let discovered = discover_collections(&provider, ProjectId::new()).unwrap();
        assert!(discovered.is_empty());
    }

    #[test]
    fn corrupt_manifest_fails_discovery_safely() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();
        publish_collection_manifest(
            &provider,
            project_id,
            NamespaceId(1),
            384,
            Metric::SquaredL2,
        )
        .unwrap();

        let path = dir
            .path()
            .join("projects")
            .join(project_id.to_string())
            .join("collections")
            .join("1")
            .join("manifest")
            .join("collection");
        std::fs::write(&path, b"not json").unwrap();

        assert!(discover_collections(&provider, project_id).is_err());
    }

    // ── Phase 2.1 §12: the core acceptance test ─────────────────────────────

    #[test]
    fn snapshot_plus_wal_tail_recovery_reproduces_original_state() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();
        let wal_path = dir.path().join("events.log");

        let mut writer =
            valori_storage::events::event_log::EventLogWriter::open(&wal_path, Some(4)).unwrap();
        let mut state = KernelState::new();
        state
            .configure_namespace(1, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();

        // LSN 1..10: pre-snapshot data.
        for i in 0..10u32 {
            let id = state.next_record_id();
            let evt = KernelEvent::InsertRecord {
                id,
                vector: FxpVector {
                    data: (0..4).map(|d| FxpScalar((i * 10 + d) as i32)).collect(),
                },
                metadata: None,
                tag: 0,
            };
            state.apply_event_ns(&evt, 1).unwrap();
            writer
                .append(&valori_storage::events::event_log::LogEntry::EventNs {
                    namespace_id: 1,
                    event: evt,
                })
                .unwrap();
        }
        publish_collection_manifest(&provider, project_id, NamespaceId(1), 4, Metric::SquaredL2)
            .unwrap();
        snapshot_collection(
            &provider,
            project_id,
            &state,
            NamespaceId(1),
            1,
            Lsn(10),
            Metric::SquaredL2,
        )
        .unwrap();

        // LSN 11..15: new events AFTER the snapshot (some inserts, one delete).
        for i in 10..14u32 {
            let id = state.next_record_id();
            let evt = KernelEvent::InsertRecord {
                id,
                vector: FxpVector {
                    data: (0..4).map(|d| FxpScalar((i * 10 + d) as i32)).collect(),
                },
                metadata: None,
                tag: 0,
            };
            state.apply_event_ns(&evt, 1).unwrap();
            writer
                .append(&valori_storage::events::event_log::LogEntry::EventNs {
                    namespace_id: 1,
                    event: evt,
                })
                .unwrap();
        }
        let del = KernelEvent::DeleteRecord { id: RecordId(2) };
        state.apply_event_ns(&del, 1).unwrap();
        writer
            .append(&valori_storage::events::event_log::LogEntry::EventNs {
                namespace_id: 1,
                event: del,
            })
            .unwrap();
        drop(writer);

        let original_hash = valori_kernel::snapshot::blake3::hash_state_blake3(&state);

        // "Worker dies", "new Worker" recovers from snapshot + WAL tail only.
        let (recovered, highest_lsn) =
            recover_project_with_wal_tail(&provider, project_id, &wal_path).unwrap();
        let recovered_hash = valori_kernel::snapshot::blake3::hash_state_blake3(&recovered);

        assert_eq!(
            recovered_hash, original_hash,
            "recovered state must byte-match the pre-crash state"
        );
        assert_eq!(highest_lsn, Lsn(15));
        assert_eq!(recovered.record_count(), state.record_count());
    }

    /// §13: the specific test designed to catch incorrect per-Collection
    /// replay logic. Two collections with DIFFERENT snapshot base_lsn,
    /// interleaved WAL events — the naive "replay from the single minimum
    /// LSN with no per-namespace filtering" bug would double-apply events
    /// for the collection with the newer snapshot; the naive "restore A
    /// fully then B fully" bug would violate RecordId ordering.
    #[test]
    fn interleaved_collection_events_with_different_snapshot_lsns() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();
        let wal_path = dir.path().join("events.log");

        let mut writer =
            valori_storage::events::event_log::EventLogWriter::open(&wal_path, Some(4)).unwrap();
        let mut state = KernelState::new();
        state
            .configure_namespace(1, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap(); // A
        state
            .configure_namespace(2, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap(); // B

        let append = |writer: &mut valori_storage::events::event_log::EventLogWriter,
                      state: &mut KernelState,
                      ns: u16,
                      evt: KernelEvent| {
            state.apply_event_ns(&evt, ns).unwrap();
            writer
                .append(&valori_storage::events::event_log::LogEntry::EventNs {
                    namespace_id: ns,
                    event: evt,
                })
                .unwrap();
        };

        // LSN 1: A insert(a0)
        let a0 = state.next_record_id();
        append(
            &mut writer,
            &mut state,
            1,
            KernelEvent::InsertRecord {
                id: a0,
                vector: FxpVector {
                    data: vec![FxpScalar(1); 4],
                },
                metadata: None,
                tag: 0,
            },
        );
        // LSN 2: B insert(b0)
        let b0 = state.next_record_id();
        append(
            &mut writer,
            &mut state,
            2,
            KernelEvent::InsertRecord {
                id: b0,
                vector: FxpVector {
                    data: vec![FxpScalar(2); 4],
                },
                metadata: None,
                tag: 0,
            },
        );
        // LSN 3: A insert(a1)
        let a1 = state.next_record_id();
        append(
            &mut writer,
            &mut state,
            1,
            KernelEvent::InsertRecord {
                id: a1,
                vector: FxpVector {
                    data: vec![FxpScalar(3); 4],
                },
                metadata: None,
                tag: 0,
            },
        );

        // Snapshot A here, at base_lsn = 3 (A has a0, a1 — both live).
        publish_collection_manifest(&provider, project_id, NamespaceId(1), 4, Metric::SquaredL2)
            .unwrap();
        snapshot_collection(
            &provider,
            project_id,
            &state,
            NamespaceId(1),
            1,
            Lsn(3),
            Metric::SquaredL2,
        )
        .unwrap();

        // LSN 4: B insert(b1)
        let b1 = state.next_record_id();
        append(
            &mut writer,
            &mut state,
            2,
            KernelEvent::InsertRecord {
                id: b1,
                vector: FxpVector {
                    data: vec![FxpScalar(4); 4],
                },
                metadata: None,
                tag: 0,
            },
        );

        // Snapshot B here, at base_lsn = 2 — OLDER than A's (B has only b0
        // through LSN 2, even though b1 already exists in the live `state`
        // by the time this snapshot is written — a real snapshot always
        // describes state as of its own declared base_lsn, not "as of when
        // it happened to be written").
        let mut b_only_through_lsn2 = KernelState::new();
        b_only_through_lsn2
            .configure_namespace(2, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();
        // `b0`'s real id is 1 (a0 was allocated first in the full
        // interleaved sequence) — this standalone mini-state needs a
        // placeholder in DEFAULT_NS to advance its own next-id counter to
        // match, purely so the explicit-id insert below validates. The
        // placeholder is namespace 0, invisible to `extract_from_kernel_state`
        // (which only reads namespace 2), so it has no effect on the
        // extracted snapshot content.
        b_only_through_lsn2
            .apply_event_ns(
                &KernelEvent::InsertRecord {
                    id: RecordId(0),
                    vector: FxpVector {
                        data: vec![FxpScalar(0); 4],
                    },
                    metadata: None,
                    tag: 0,
                },
                0,
            )
            .unwrap();
        b_only_through_lsn2
            .apply_event_ns(
                &KernelEvent::InsertRecord {
                    id: b0,
                    vector: FxpVector {
                        data: vec![FxpScalar(2); 4],
                    },
                    metadata: None,
                    tag: 0,
                },
                2,
            )
            .unwrap();
        publish_collection_manifest(&provider, project_id, NamespaceId(2), 4, Metric::SquaredL2)
            .unwrap();
        snapshot_collection(
            &provider,
            project_id,
            &b_only_through_lsn2,
            NamespaceId(2),
            1,
            Lsn(2),
            Metric::SquaredL2,
        )
        .unwrap();

        // LSN 5: A insert(a2)
        let a2 = state.next_record_id();
        append(
            &mut writer,
            &mut state,
            1,
            KernelEvent::InsertRecord {
                id: a2,
                vector: FxpVector {
                    data: vec![FxpScalar(5); 4],
                },
                metadata: None,
                tag: 0,
            },
        );
        drop(writer);

        let original_hash = valori_kernel::snapshot::blake3::hash_state_blake3(&state);

        let (recovered, _) =
            recover_project_with_wal_tail(&provider, project_id, &wal_path).unwrap();
        let recovered_hash = valori_kernel::snapshot::blake3::hash_state_blake3(&recovered);

        assert_eq!(
            recovered_hash, original_hash,
            "A's snapshot (base_lsn 3) must not cause B's LSN-4 event to be \
             skipped, and B's snapshot (base_lsn 2) must not cause A's \
             LSN-3/5 events to be re-applied or dropped"
        );
        assert_eq!(
            recovered.get_record(a0).map(|r| r.vector.data[0].0),
            Some(1)
        );
        assert_eq!(
            recovered.get_record(a1).map(|r| r.vector.data[0].0),
            Some(3)
        );
        assert_eq!(
            recovered.get_record(b0).map(|r| r.vector.data[0].0),
            Some(2)
        );
        assert_eq!(
            recovered.get_record(b1).map(|r| r.vector.data[0].0),
            Some(4)
        );
        assert_eq!(
            recovered.get_record(a2).map(|r| r.vector.data[0].0),
            Some(5)
        );
    }

    #[test]
    fn mixed_dimensions_survive_live_recovery_path() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();
        let wal_path = dir.path().join("events.log"); // never written to — snapshot-only recovery

        let mut state = KernelState::new();
        seed(&mut state, 1, 384, 2);
        seed(&mut state, 2, 768, 2);
        seed(&mut state, 3, 1536, 2);

        for (ns, dim) in [(1u16, 384u32), (2, 768), (3, 1536)] {
            publish_collection_manifest(
                &provider,
                project_id,
                NamespaceId(ns),
                dim,
                Metric::SquaredL2,
            )
            .unwrap();
            snapshot_collection(
                &provider,
                project_id,
                &state,
                NamespaceId(ns),
                1,
                Lsn(6),
                Metric::SquaredL2,
            )
            .unwrap();
        }

        let (recovered, _) =
            recover_project_with_wal_tail(&provider, project_id, &wal_path).unwrap();
        assert_eq!(recovered.namespace_dim(1), Some(384));
        assert_eq!(recovered.namespace_dim(2), Some(768));
        assert_eq!(recovered.namespace_dim(3), Some(1536));
        assert_eq!(recovered.record_count(), 6);
    }

    #[test]
    fn snapshot_plus_wal_tail_recovery_preserves_graph_state() {
        use valori_core::{EdgeKind, NodeKind};
        use valori_kernel::types::id::{EdgeId, NodeId};

        let dir = tempfile::tempdir().unwrap();
        let provider = LocalStorageProvider::open(dir.path()).unwrap();
        let project_id = ProjectId::new();
        let wal_path = dir.path().join("events.log");

        let mut writer =
            valori_storage::events::event_log::EventLogWriter::open(&wal_path, Some(4)).unwrap();
        let mut state = KernelState::new();
        state
            .configure_namespace(1, 4, valori_kernel::index::Metric::SquaredL2, 0)
            .unwrap();

        // 1. Create a node and a record pre-snapshot
        let r0 = state.next_record_id();
        let ev_r0 = KernelEvent::InsertRecord {
            id: r0,
            vector: FxpVector {
                data: vec![FxpScalar(10); 4],
            },
            metadata: None,
            tag: 0,
        };
        state.apply_event_ns(&ev_r0, 1).unwrap();
        writer
            .append(&valori_storage::events::event_log::LogEntry::EventNs {
                namespace_id: 1,
                event: ev_r0,
            })
            .unwrap();

        let n0 = state.next_node_id();
        let ev_n0 = KernelEvent::CreateNode {
            id: n0,
            kind: NodeKind::Document,
            record: Some(r0),
        };
        state.apply_event_ns(&ev_n0, 1).unwrap();
        writer
            .append(&valori_storage::events::event_log::LogEntry::EventNs {
                namespace_id: 1,
                event: ev_n0,
            })
            .unwrap();

        let n1 = state.next_node_id();
        let ev_n1 = KernelEvent::CreateNode {
            id: n1,
            kind: NodeKind::Concept,
            record: None,
        };
        state.apply_event_ns(&ev_n1, 1).unwrap();
        writer
            .append(&valori_storage::events::event_log::LogEntry::EventNs {
                namespace_id: 1,
                event: ev_n1,
            })
            .unwrap();

        // 2. Snapshot collection at base_lsn = 3
        publish_collection_manifest(&provider, project_id, NamespaceId(1), 4, Metric::SquaredL2)
            .unwrap();
        snapshot_collection(
            &provider,
            project_id,
            &state,
            NamespaceId(1),
            1,
            Lsn(3),
            Metric::SquaredL2,
        )
        .unwrap();

        // 3. Post-snapshot events in WAL: Create edge, create another node
        let e0 = state.next_edge_id();
        let ev_e0 = KernelEvent::CreateEdge {
            id: e0,
            from: n0,
            to: n1,
            kind: EdgeKind::Mentions,
        };
        state.apply_event_ns(&ev_e0, 1).unwrap();
        writer
            .append(&valori_storage::events::event_log::LogEntry::EventNs {
                namespace_id: 1,
                event: ev_e0,
            })
            .unwrap();

        let n2 = state.next_node_id();
        let ev_n2 = KernelEvent::CreateNode {
            id: n2,
            kind: NodeKind::Tool,
            record: None,
        };
        state.apply_event_ns(&ev_n2, 1).unwrap();
        writer
            .append(&valori_storage::events::event_log::LogEntry::EventNs {
                namespace_id: 1,
                event: ev_n2,
            })
            .unwrap();
        drop(writer);

        let original_hash = valori_kernel::snapshot::blake3::hash_state_blake3(&state);

        // 4. Recover from StorageProvider + WAL tail
        let (recovered, highest_lsn) =
            recover_project_with_wal_tail(&provider, project_id, &wal_path).unwrap();
        let recovered_hash = valori_kernel::snapshot::blake3::hash_state_blake3(&recovered);

        assert_eq!(recovered_hash, original_hash);
        assert_eq!(highest_lsn, Lsn(5));
        assert_eq!(recovered.node_count(), 3);
        assert_eq!(recovered.edge_count(), 1);

        let node0 = recovered.get_node(n0).unwrap();
        assert_eq!(node0.kind, NodeKind::Document);
        assert_eq!(node0.record, Some(r0));
        assert_eq!(node0.namespace_id, 1);

        let edge0 = recovered.get_edge(e0).unwrap();
        assert_eq!(edge0.from, n0);
        assert_eq!(edge0.to, n1);
        assert_eq!(edge0.kind, EdgeKind::Mentions);
    }
}
