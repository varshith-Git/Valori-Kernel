// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! IVF (Inverted File) index integration and recall tests.
//!
//! Verifies that:
//!   1. Vectors inserted with an IVF-configured engine can be retrieved.
//!   2. Recall@K is above a threshold once the index is populated.
//!   3. Soft-deleted vectors are excluded from IVF query results.
//!   4. `rebuild_index()` reproduces the same search results as direct insert.

use valori_node::config::{NodeConfig, IndexKind, QuantizationKind};
use valori_node::engine::Engine;

const DIM: usize = 8;
const N_VECTORS: usize = 200;

fn make_ivf_cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = DIM;
    cfg.max_records = 512;
    cfg.max_nodes = 512;
    cfg.max_edges = 1024;
    cfg.index_kind = IndexKind::Ivf;
    cfg.quantization_kind = QuantizationKind::None;
    cfg.event_log_path = None;
    cfg.wal_path = None;
    cfg.snapshot_path = None;
    cfg
}

/// Generate a deterministic unit-ish vector for slot `i`.
fn make_vec(i: usize) -> Vec<f32> {
    let angle = i as f32 * std::f32::consts::TAU / N_VECTORS as f32;
    // Spread across all DIM dimensions to avoid degenerate clusters.
    (0..DIM)
        .map(|d| (angle + d as f32 * 0.4).sin() * 0.5 + 0.5)
        .collect()
}

// ── Test 1: IVF returns results after insert + build ─────────────────────────
//
// IVF requires an explicit `build_index()` call so that k-means can compute
// cluster centroids from the full data distribution.  Searches before
// `build_index()` will find 0 results because no centroids exist yet.

#[test]
fn test_ivf_returns_results_after_insert() {
    let mut engine = Engine::new(&make_ivf_cfg());

    for i in 0..N_VECTORS {
        engine.insert_record_from_f32(&make_vec(i)).expect("insert");
    }
    // Build centroids now that the full dataset is loaded.
    engine.build_index();

    // Query the exact vector we inserted as record 42.
    let query = make_vec(42);
    let results = engine.search_l2(&query, 5).expect("search");

    assert!(
        !results.is_empty(),
        "IVF search must return at least one result after {} inserts", N_VECTORS
    );
}

// ── Test 2: recall@1 — the nearest neighbour is the query vector itself ───────

#[test]
fn test_ivf_recall_at_1() {
    let mut engine = Engine::new(&make_ivf_cfg());

    let mut inserted_ids = Vec::new();
    for i in 0..N_VECTORS {
        let id = engine.insert_record_from_f32(&make_vec(i)).expect("insert");
        inserted_ids.push(id);
    }
    engine.build_index(); // compute centroids from full distribution

    // For each of several sample queries, the top-1 result should be the
    // record we inserted with that exact vector.
    let sample_indices = [0, 10, 42, 99, 150, N_VECTORS - 1];
    let mut hits = 0usize;

    for &idx in &sample_indices {
        let query = make_vec(idx);
        let results = engine.search_l2(&query, 1).expect("search");
        if !results.is_empty() && results[0].0 == inserted_ids[idx] {
            hits += 1;
        }
    }

    // IVF recall@1 must be at least 50 % on exact-match queries.
    // (Brute-force would be 100 %; IVF may miss due to coarse quantisation
    // with small cluster counts, but should still do well.)
    let recall = hits as f32 / sample_indices.len() as f32;
    assert!(
        recall >= 0.5,
        "IVF recall@1 on exact-match queries should be ≥ 50 %, got {:.0} % ({}/{} hits)",
        recall * 100.0, hits, sample_indices.len()
    );
}

// ── Test 3: soft-deleted vectors are excluded from results ─────────────────────

#[test]
fn test_ivf_excludes_soft_deleted_records() {
    let mut engine = Engine::new(&make_ivf_cfg());

    // Insert a cluster of very similar vectors, all near [0, 0, ..., 0].
    // Record 0 will be soft-deleted; the others remain.
    let near_origin: Vec<f32> = (0..DIM).map(|_| 0.001).collect();
    let r0 = engine.insert_record_from_f32(&near_origin).expect("insert r0");

    for _ in 1..10 {
        engine.insert_record_from_f32(&near_origin).expect("insert near-origin");
    }
    // Pad with distant vectors so the index has enough variety for IVF to form clusters.
    for i in 10..N_VECTORS {
        engine.insert_record_from_f32(&make_vec(i)).expect("insert");
    }
    engine.build_index(); // compute centroids from full distribution

    engine.soft_delete_record(r0).expect("soft delete");

    // Query near origin — r0 must NOT appear.
    let query: Vec<f32> = vec![0.0; DIM];
    let results = engine.search_l2(&query, 10).expect("search");

    assert!(
        results.iter().all(|(id, _)| *id != r0),
        "soft-deleted record {} must not appear in IVF results; got {:?}",
        r0, results
    );
}

// ── Test 4: rebuild_index reproduces the same nearest neighbour ───────────────

#[test]
fn test_ivf_rebuild_index_consistency() {
    let mut engine = Engine::new(&make_ivf_cfg());

    for i in 0..N_VECTORS {
        engine.insert_record_from_f32(&make_vec(i)).expect("insert");
    }
    engine.build_index(); // compute centroids from full distribution

    let query = make_vec(77);
    let before = engine.search_l2(&query, 5).expect("search before rebuild");

    // Force a full index rebuild (same path taken after event-log recovery).
    engine.rebuild_index();

    let after = engine.search_l2(&query, 5).expect("search after rebuild");

    assert_eq!(
        before.len(), after.len(),
        "result count must not change after rebuild"
    );
    // Top-1 must be the same record.
    if !before.is_empty() && !after.is_empty() {
        assert_eq!(
            before[0].0, after[0].0,
            "top-1 result must be the same before and after rebuild"
        );
    }
}
