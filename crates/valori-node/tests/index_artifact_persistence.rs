// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Phase 4.1 — Index artifact persistence tests.
//!
//! Tests the round-trip: build index → write artifact → restart →
//! restore from artifact (or fall back gracefully).
//!
//! All tests drive the `Engine` API directly (no HTTP server) to keep them
//! fast and deterministic.  The pattern:
//!   1. configure engine with LocalStorageProvider
//!   2. create collection, insert records, build index
//!   3. snapshot collection (makes the manifest+artifact durable)
//!   4. drop engine (simulates restart — in-memory state is gone)
//!   5. new engine, same provider → try_recover()
//!   6. assert: search works, and the right recovery path was taken

use std::sync::Arc;
use tempfile::tempdir;
use valori_domain::{IndexKind, Metric, ProjectId};
use valori_engine::index_manager::IndexSpec;
use valori_node::engine::{Engine, RecoveryMode};
use valori_node::EngineFromNodeConfig;
use valori_storage::provider::local::LocalStorageProvider;
use valori_storage::provider::StorageProvider;

const DIM: usize = 16;
const N: usize = 120; // big enough for IVF (≥ 16 centroids)

// ── helpers ──────────────────────────────────────────────────────────────────

fn records(n: usize) -> Vec<(u32, Vec<f32>)> {
    (0..n)
        .map(|i| {
            let mut v = vec![0.0f32; DIM];
            v[0] = i as f32;
            (i as u32, v)
        })
        .collect()
}

fn make_engine(provider: Arc<dyn StorageProvider>, project_id: ProjectId) -> Engine {
    let mut cfg = valori_node::config::NodeConfig::default();
    cfg.max_records = N * 2;
    cfg.max_nodes = N;
    cfg.max_edges = N;
    let mut e = Engine::new(&cfg);
    e.configure_storage_provider(provider, project_id, None);
    e
}

/// Insert N records with the given step vector into `ns_id`.
fn fill_ns(engine: &mut Engine, ns_id: u16, n: usize) {
    for i in 0..n {
        let mut v = vec![0.0f32; DIM];
        v[0] = i as f32;
        engine
            .insert_record_from_f32_ns(&v, ns_id)
            .expect("insert failed");
    }
}

/// Build an HNSW index directly and install it via finish_index_build.
fn build_hnsw(engine: &mut Engine, ns_id: u16) -> u32 {
    use valori_index::{HnswConfig, HnswIndex, VectorIndex};
    let spec = IndexSpec {
        index_type: "hnsw".into(),
        parameters: serde_json::json!({}),
    };
    let gen = engine
        .start_index_build(ns_id, spec)
        .expect("start_index_build failed");
    let records = engine.snapshot_records_for_ns(ns_id);
    let mut idx: Box<dyn VectorIndex + Send + Sync> =
        Box::new(HnswIndex::new_with_config(HnswConfig::default()));
    idx.build(&records);
    engine
        .finish_index_build(ns_id, gen, idx)
        .expect("finish_index_build failed");
    gen
}

/// Build an IVF index directly and install it via finish_index_build.
fn build_ivf(engine: &mut Engine, ns_id: u16) -> u32 {
    use valori_index::{IvfConfig, IvfIndex, VectorIndex};
    let spec = IndexSpec {
        index_type: "ivf".into(),
        parameters: serde_json::json!({}),
    };
    let gen = engine
        .start_index_build(ns_id, spec)
        .expect("start_index_build failed");
    let records = engine.snapshot_records_for_ns(ns_id);
    let n_list = std::cmp::max(16, (records.len() as f32).sqrt() as usize);
    let mut idx: Box<dyn VectorIndex + Send + Sync> = Box::new(IvfIndex::new(
        IvfConfig {
            n_list,
            n_probe: 4,
            auto_scale: true,
        },
        DIM,
    ));
    idx.build(&records);
    engine
        .finish_index_build(ns_id, gen, idx)
        .expect("finish_index_build failed");
    gen
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Build HNSW, snapshot, restart: artifact is restored and searches work.
#[test]
fn artifact_hnsw_roundtrip() {
    let dir = tempdir().unwrap();
    let provider: Arc<dyn StorageProvider> =
        Arc::new(LocalStorageProvider::open(dir.path()).unwrap());
    let project_id = ProjectId::new();

    let ns_id;
    {
        let mut engine = make_engine(provider.clone(), project_id);
        ns_id = engine
            .create_collection_with_config("col", DIM as u32, Metric::SquaredL2, IndexKind::Hnsw)
            .unwrap();
        fill_ns(&mut engine, ns_id, N);
        build_hnsw(&mut engine, ns_id);

        // Snapshot makes the record data + manifest durable.
        engine
            .snapshot_collection_to_storage(valori_kernel::types::id::NamespaceId(ns_id), 1)
            .unwrap();
        // engine dropped here — in-memory state is gone
    }

    // ── Restart ──────────────────────────────────────────────────────────────
    let mut e2 = make_engine(provider, project_id);
    let mode = e2.try_recover();
    assert!(
        matches!(mode, RecoveryMode::StorageProvider(_)),
        "expected StorageProvider recovery, got {mode:?}"
    );

    // Collection is present and searchable.
    let mut q = vec![0.0f32; DIM];
    q[0] = 50.0; // nearest to record 50
    let hits = e2.search_l2_ns(&q, 1, ns_id).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].0, 50,
        "expected record 50 to be nearest (HNSW restore)"
    );

    // The index_state should reflect an active generation.
    let state = e2.index_state(ns_id);
    assert!(
        state.active_generation.is_some(),
        "index_state should have an active generation after restart"
    );
}

/// Build IVF, snapshot, restart: artifact is restored and searches work.
#[test]
fn artifact_ivf_roundtrip() {
    let dir = tempdir().unwrap();
    let provider: Arc<dyn StorageProvider> =
        Arc::new(LocalStorageProvider::open(dir.path()).unwrap());
    let project_id = ProjectId::new();

    let ns_id;
    {
        let mut engine = make_engine(provider.clone(), project_id);
        ns_id = engine
            .create_collection_with_config("col", DIM as u32, Metric::SquaredL2, IndexKind::Ivf)
            .unwrap();
        fill_ns(&mut engine, ns_id, N);
        build_ivf(&mut engine, ns_id);

        engine
            .snapshot_collection_to_storage(valori_kernel::types::id::NamespaceId(ns_id), 1)
            .unwrap();
    }

    let mut e2 = make_engine(provider, project_id);
    let mode = e2.try_recover();
    assert!(matches!(mode, RecoveryMode::StorageProvider(_)));

    let mut q = vec![0.0f32; DIM];
    q[0] = 60.0;
    let hits = e2.search_l2_ns(&q, 1, ns_id).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].0, 60,
        "expected record 60 to be nearest (IVF restore)"
    );

    let state = e2.index_state(ns_id);
    assert!(state.active_generation.is_some());
}

/// Artifact file deleted before restart → brute-force fallback, searches still work.
#[test]
fn artifact_missing_falls_back_to_rebuild() {
    let dir = tempdir().unwrap();
    let provider: Arc<dyn StorageProvider> =
        Arc::new(LocalStorageProvider::open(dir.path()).unwrap());
    let project_id = ProjectId::new();

    let ns_id;
    {
        let mut engine = make_engine(provider.clone(), project_id);
        ns_id = engine
            .create_collection_with_config("col", DIM as u32, Metric::SquaredL2, IndexKind::Hnsw)
            .unwrap();
        fill_ns(&mut engine, ns_id, N);
        build_hnsw(&mut engine, ns_id);
        engine
            .snapshot_collection_to_storage(valori_kernel::types::id::NamespaceId(ns_id), 1)
            .unwrap();
    }

    // Manually delete all files under indexes/ to simulate a corrupt/missing artifact.
    let indexes_dir = dir
        .path()
        .join("projects")
        .join(project_id.to_string())
        .join("collections")
        .join(ns_id.to_string())
        .join("indexes");
    if indexes_dir.exists() {
        std::fs::remove_dir_all(&indexes_dir).unwrap();
    }

    // Restart: should fall back to rebuild from records, not crash.
    let mut e2 = make_engine(provider, project_id);
    let mode = e2.try_recover();
    assert!(matches!(mode, RecoveryMode::StorageProvider(_)));

    // Search still works via the rebuilt index.
    let mut q = vec![0.0f32; DIM];
    q[0] = 30.0;
    let hits = e2.search_l2_ns(&q, 1, ns_id).unwrap();
    assert!(
        !hits.is_empty(),
        "search must work even after artifact fallback"
    );
}

/// Stale artifact (inserts after build): must rebuild, not use stale artifact.
#[test]
fn stale_artifact_triggers_rebuild() {
    let dir = tempdir().unwrap();
    let provider: Arc<dyn StorageProvider> =
        Arc::new(LocalStorageProvider::open(dir.path()).unwrap());
    let project_id = ProjectId::new();

    let ns_id;
    let post_build_record_id;
    {
        let mut engine = make_engine(provider.clone(), project_id);
        ns_id = engine
            .create_collection_with_config("col", DIM as u32, Metric::SquaredL2, IndexKind::Hnsw)
            .unwrap();
        fill_ns(&mut engine, ns_id, N);
        build_hnsw(&mut engine, ns_id);

        // Insert a record AFTER the build — makes base_lsn < current_lsn.
        let mut v = vec![0.0f32; DIM];
        v[0] = 999.0;
        post_build_record_id = engine.insert_record_from_f32_ns(&v, ns_id).unwrap();

        // Snapshot includes the new record; artifact is from before.
        engine
            .snapshot_collection_to_storage(valori_kernel::types::id::NamespaceId(ns_id), 1)
            .unwrap();
    }

    // Restart: artifact is stale → rebuild from records → new record visible.
    let mut e2 = make_engine(provider, project_id);
    let mode = e2.try_recover();
    assert!(matches!(mode, RecoveryMode::StorageProvider(_)));

    // The post-build record must be searchable.
    let mut q = vec![0.0f32; DIM];
    q[0] = 999.0;
    let hits = e2.search_l2_ns(&q, 1, ns_id).unwrap();
    assert!(!hits.is_empty(), "post-build record must be findable");
    assert_eq!(
        hits[0].0, post_build_record_id,
        "stale-artifact fallback must include post-build records"
    );
}

/// Build HNSW with explicit m/ef parameters; verify search still works.
/// (We can't directly inspect HnswConfig from outside, so we verify build+search succeeds.)
#[test]
fn hnsw_explicit_parameters_accepted() {
    use valori_index::{HnswConfig, HnswIndex, VectorIndex};

    let dir = tempdir().unwrap();
    let provider: Arc<dyn StorageProvider> =
        Arc::new(LocalStorageProvider::open(dir.path()).unwrap());
    let project_id = ProjectId::new();

    let mut engine = make_engine(provider, project_id);
    let ns_id = engine
        .create_collection_with_config("col", DIM as u32, Metric::SquaredL2, IndexKind::Hnsw)
        .unwrap();
    fill_ns(&mut engine, ns_id, N);

    // Build with explicit parameters (m=8, ef_construction=40, ef_search=20).
    let spec = IndexSpec {
        index_type: "hnsw".into(),
        parameters: serde_json::json!({"m": 8, "ef_construction": 40, "ef_search": 20}),
    };
    let gen = engine.start_index_build(ns_id, spec).unwrap();
    let records = engine.snapshot_records_for_ns(ns_id);
    let mut cfg = HnswConfig::default();
    cfg.m = 8;
    cfg.m_max0 = 16;
    cfg.ef_construction = 40;
    cfg.ef_search = 20;
    let mut idx: Box<dyn VectorIndex + Send + Sync> = Box::new(HnswIndex::new_with_config(cfg));
    idx.build(&records);
    engine.finish_index_build(ns_id, gen, idx).unwrap();

    let mut q = vec![0.0f32; DIM];
    q[0] = 50.0;
    let hits = engine.search_l2_ns(&q, 1, ns_id).unwrap();
    assert_eq!(
        hits[0].0, 50,
        "HNSW with explicit params must find correct nearest"
    );
}

/// Build IVF with explicit n_list/n_probe; verify search still works.
#[test]
fn ivf_explicit_parameters_accepted() {
    use valori_index::{IvfConfig, IvfIndex, VectorIndex};

    let dir = tempdir().unwrap();
    let provider: Arc<dyn StorageProvider> =
        Arc::new(LocalStorageProvider::open(dir.path()).unwrap());
    let project_id = ProjectId::new();

    let mut engine = make_engine(provider, project_id);
    let ns_id = engine
        .create_collection_with_config("col", DIM as u32, Metric::SquaredL2, IndexKind::Ivf)
        .unwrap();
    fill_ns(&mut engine, ns_id, N);

    // Explicit n_list=20, n_probe=5 (auto_scale=false).
    let spec = IndexSpec {
        index_type: "ivf".into(),
        parameters: serde_json::json!({"n_list": 20, "n_probe": 5}),
    };
    let gen = engine.start_index_build(ns_id, spec).unwrap();
    let records = engine.snapshot_records_for_ns(ns_id);
    let mut idx: Box<dyn VectorIndex + Send + Sync> = Box::new(IvfIndex::new(
        IvfConfig {
            n_list: 20,
            n_probe: 5,
            auto_scale: false,
        },
        DIM,
    ));
    idx.build(&records);
    engine.finish_index_build(ns_id, gen, idx).unwrap();

    let mut q = vec![0.0f32; DIM];
    q[0] = 70.0;
    let hits = engine.search_l2_ns(&q, 1, ns_id).unwrap();
    assert_eq!(
        hits[0].0, 70,
        "IVF with explicit params must find correct nearest"
    );
}

/// Drop active index clears the manifest's index fields.
#[test]
fn drop_index_clears_manifest() {
    use valori_storage::provider::StorageKey;

    let dir = tempdir().unwrap();
    let provider: Arc<dyn StorageProvider> =
        Arc::new(LocalStorageProvider::open(dir.path()).unwrap());
    let project_id = ProjectId::new();

    let ns_id;
    {
        let mut engine = make_engine(provider.clone(), project_id);
        ns_id = engine
            .create_collection_with_config("col", DIM as u32, Metric::SquaredL2, IndexKind::Hnsw)
            .unwrap();
        fill_ns(&mut engine, ns_id, N);
        build_hnsw(&mut engine, ns_id);

        // Snapshot so the manifest + artifact exist.
        engine
            .snapshot_collection_to_storage(valori_kernel::types::id::NamespaceId(ns_id), 1)
            .unwrap();

        // Manifest should now have active_index_generation set.
        let manifest_key = StorageKey::CollectionManifest {
            project_id,
            collection_id: valori_kernel::types::id::NamespaceId(ns_id),
        };
        let bytes = provider.get(&manifest_key).unwrap();
        let manifest =
            valori_storage::collection_manifest::CollectionManifest::decode(&manifest_key, &bytes)
                .unwrap();
        assert!(
            manifest.active_index_generation.is_some(),
            "manifest must have active_index_generation after build"
        );

        // Drop the index.
        engine.drop_collection_index(ns_id);

        // Manifest should now have active_index_generation cleared.
        let bytes2 = provider.get(&manifest_key).unwrap();
        let manifest2 =
            valori_storage::collection_manifest::CollectionManifest::decode(&manifest_key, &bytes2)
                .unwrap();
        assert!(
            manifest2.active_index_generation.is_none(),
            "manifest must clear active_index_generation after drop"
        );
    }
}
