// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//!
//! Roundtrip tests for the two trailing snapshot sections added to Engine:
//!
//!   CRTS — created_at timestamps (HashMap<u32, u64>). Used by decay re-ranking.
//!   BCRP — BM25 reranker corpus (HashMap<u64, Vec<String>> + total_tokens).
//!
//! Each test follows the same pattern:
//!   1. Build an Engine, insert records (with text for BCRP).
//!   2. Call engine.snapshot() → bytes.
//!   3. Restore into a fresh Engine from those bytes.
//!   4. Assert the specific section survived.

use valori_node::config::NodeConfig;
use valori_node::EngineFromNodeConfig;
use valori_node::engine::Engine;

fn make_cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 64;
    cfg.max_nodes = 64;
    cfg.max_edges = 128;
    cfg
}

// ── CRTS: created_at timestamps ───────────────────────────────────────────────

#[test]
fn crts_timestamps_survive_roundtrip() {
    let mut engine = Engine::new(&make_cfg());

    // Insert two records so they each get a created_at entry.
    let id0 = engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
    let id1 = engine.insert_record_from_f32(&[0.9, 0.8, 0.7, 0.6]).unwrap();

    // Capture the timestamps before snapshot.
    let ts0_before = engine.record_created_at(id0);
    let ts1_before = engine.record_created_at(id1);
    assert!(ts0_before.is_some(), "record 0 must have a created_at entry");
    assert!(ts1_before.is_some(), "record 1 must have a created_at entry");

    // Roundtrip through snapshot bytes.
    let snap = engine.snapshot().expect("snapshot must succeed");
    let mut engine2 = Engine::new(&make_cfg());
    engine2.restore(&snap).expect("restore must succeed");

    // Timestamps must be identical after restore.
    assert_eq!(
        engine2.record_created_at(id0),
        ts0_before,
        "record 0 created_at must survive snapshot roundtrip"
    );
    assert_eq!(
        engine2.record_created_at(id1),
        ts1_before,
        "record 1 created_at must survive snapshot roundtrip"
    );
}

#[test]
fn crts_absent_in_old_snapshot_does_not_panic() {
    // Simulate an old snapshot that has no CRTS section by stripping the
    // trailing bytes after the NSRG tag.
    let mut engine = Engine::new(&make_cfg());
    engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();

    let snap = engine.snapshot().expect("snapshot");

    // Find the NSRG tag and truncate everything after its payload.
    let nsrg_pos = snap
        .windows(4)
        .position(|w| w == b"NSRG")
        .expect("NSRG must be present in snapshot");
    let nsrg_payload_len = u32::from_le_bytes(
        snap[nsrg_pos + 4..nsrg_pos + 8].try_into().unwrap()
    ) as usize;
    let truncated = &snap[..nsrg_pos + 8 + nsrg_payload_len];

    // Restore must succeed and created_at is simply empty (no panic).
    let mut engine2 = Engine::new(&make_cfg());
    engine2.restore(truncated).expect("restore of pre-CRTS snapshot must succeed");
    assert_eq!(
        engine2.record_created_at(0),
        None,
        "no CRTS in snapshot → created_at is empty"
    );
}

// ── BCRP: BM25 reranker corpus ────────────────────────────────────────────────

#[test]
fn bcrp_corpus_survives_roundtrip() {
    let mut engine = Engine::new(&make_cfg());

    // Insert with text so the reranker indexes the tokens.
    let id0 = engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
    engine.reranker_insert(id0, "the quick brown fox");

    let id1 = engine.insert_record_from_f32(&[0.9, 0.8, 0.7, 0.6]).unwrap();
    engine.reranker_insert(id1, "lazy dog over fence");

    // Corpus size before snapshot.
    let corpus_len_before = engine.reranker_corpus_len();
    assert!(corpus_len_before > 0, "reranker corpus must be non-empty before snapshot");

    let snap = engine.snapshot().expect("snapshot");
    let mut engine2 = Engine::new(&make_cfg());
    engine2.restore(&snap).expect("restore");

    // Corpus must have the same number of entries.
    assert_eq!(
        engine2.reranker_corpus_len(),
        corpus_len_before,
        "reranker corpus entry count must survive snapshot roundtrip"
    );

    // A rerank query against a known token should return results in restored engine.
    let query = vec![0.1f32, 0.2, 0.3, 0.4];
    let candidates = vec![(id0, 0.1f32), (id1, 0.9f32)];
    let reranked = engine2.reranker_rerank("quick fox", &query, &candidates);
    assert!(!reranked.is_empty(), "reranker must return results after restore");
    // id0 has "quick" and "fox" — should rank above id1 for this query.
    assert_eq!(reranked[0].0, id0, "record with matching tokens must rank first");
}

#[test]
fn bcrp_absent_in_old_snapshot_does_not_panic() {
    let mut engine = Engine::new(&make_cfg());
    let id0 = engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
    engine.reranker_insert(id0, "hello world");

    let snap = engine.snapshot().expect("snapshot");

    // Strip BCRP by truncating at the BCRP tag.
    let bcrp_pos = snap
        .windows(4)
        .position(|w| w == b"BCRP")
        .expect("BCRP must be present in snapshot");
    let truncated = &snap[..bcrp_pos];

    let mut engine2 = Engine::new(&make_cfg());
    engine2.restore(truncated).expect("restore of pre-BCRP snapshot must succeed");
    assert_eq!(
        engine2.reranker_corpus_len(),
        0,
        "no BCRP in snapshot → corpus is empty"
    );
}

// ── Combined: both sections survive together ──────────────────────────────────

#[test]
fn crts_and_bcrp_both_survive_roundtrip() {
    let mut engine = Engine::new(&make_cfg());

    let id0 = engine.insert_record_from_f32(&[0.1, 0.2, 0.3, 0.4]).unwrap();
    engine.reranker_insert(id0, "neural retrieval augmented generation");
    let id1 = engine.insert_record_from_f32(&[0.5, 0.5, 0.5, 0.5]).unwrap();
    engine.reranker_insert(id1, "vector database deterministic search");

    let ts0 = engine.record_created_at(id0);
    let ts1 = engine.record_created_at(id1);
    let corpus_len = engine.reranker_corpus_len();

    let snap = engine.snapshot().expect("snapshot");
    let mut engine2 = Engine::new(&make_cfg());
    engine2.restore(&snap).expect("restore");

    assert_eq!(engine2.record_created_at(id0), ts0, "CRTS id0");
    assert_eq!(engine2.record_created_at(id1), ts1, "CRTS id1");
    assert_eq!(engine2.reranker_corpus_len(), corpus_len, "BCRP corpus len");
}
