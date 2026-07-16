// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Tests for ActiveIndex transitions: switching kinds preserves data and
//! search correctness. Each test exercises the contract stated in
//! `KernelState::set_index_kind`.

use valori_kernel::event::KernelEvent;
use valori_kernel::index::IndexVariant;
use valori_kernel::index::SearchResult;
use valori_kernel::snapshot::{decode::decode_state, encode::encode_state};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

fn fxp_vec(vals: &[i32]) -> FxpVector {
    FxpVector {
        data: vals.iter().copied().map(FxpScalar).collect(),
    }
}

fn make_state_with_records(n: usize, dim: usize) -> KernelState {
    let mut state = KernelState::new();
    for i in 0..n {
        // Distinct vectors: dimension d gets value (i * dim + d) scaled to Q16.16.
        let data: Vec<FxpScalar> = (0..dim)
            .map(|d| FxpScalar(((i * dim + d) as i32) * 256))
            .collect();
        state
            .apply_event(&KernelEvent::InsertRecord {
                id: RecordId(i as u32),
                vector: FxpVector { data },
                metadata: None,
                tag: 0,
            })
            .unwrap();
    }
    state
}

#[test]
fn bf_to_bq_preserves_record_count() {
    let mut state = make_state_with_records(50, 8);
    assert_eq!(state.index_variant(), IndexVariant::BruteForce);
    assert_eq!(state.record_count(), 50);

    state.set_index_kind(IndexVariant::BinaryQuantization);

    assert_eq!(state.index_variant(), IndexVariant::BinaryQuantization);
    // Record pool must be untouched.
    assert_eq!(state.record_count(), 50);
}

#[test]
fn bq_to_bf_preserves_top1_result() {
    let mut state = make_state_with_records(100, 8);
    state.set_index_kind(IndexVariant::BinaryQuantization);

    let query = fxp_vec(&[1024; 8]);

    let mut bq_results = vec![SearchResult::default(); 5];
    let bq_count = state.search_l2(&query, &mut bq_results, None);

    // Switch back to BruteForce.
    state.set_index_kind(IndexVariant::BruteForce);
    assert_eq!(state.index_variant(), IndexVariant::BruteForce);

    let mut bf_results = vec![SearchResult::default(); 5];
    let bf_count = state.search_l2(&query, &mut bf_results, None);

    // BF is exact; BQ is approximate but on 100 records with 8 dims top-1 must agree.
    assert_eq!(bf_count, bq_count);
    assert_eq!(
        bf_results[0].id, bq_results[0].id,
        "top-1 must agree after BQ→BF round-trip"
    );
}

#[test]
fn multiple_switches_do_not_lose_data() {
    let mut state = make_state_with_records(30, 4);

    for _ in 0..5 {
        state.set_index_kind(IndexVariant::BinaryQuantization);
        assert_eq!(state.record_count(), 30);
        state.set_index_kind(IndexVariant::BruteForce);
        assert_eq!(state.record_count(), 30);
    }
}

#[test]
fn noop_switch_is_safe() {
    let mut state = make_state_with_records(10, 4);
    // Switching to the same variant must not corrupt anything.
    state.set_index_kind(IndexVariant::BruteForce);
    state.set_index_kind(IndexVariant::BruteForce);
    assert_eq!(state.record_count(), 10);

    state.set_index_kind(IndexVariant::BinaryQuantization);
    state.set_index_kind(IndexVariant::BinaryQuantization);
    assert_eq!(state.record_count(), 10);
}

#[test]
fn snapshot_restore_after_switch_gives_same_results() {
    let mut state = make_state_with_records(50, 8);
    state.set_index_kind(IndexVariant::BinaryQuantization);

    // Encode → decode.
    let mut buf = Vec::new();
    encode_state(&state, &mut buf).expect("encode");
    let mut restored = decode_state(&buf).expect("decode");
    // After decode the restored state starts with BruteForce (snapshots store
    // vectors, not the index selection). Rebuild as BQ to compare apples-to-apples.
    restored.set_index_kind(IndexVariant::BinaryQuantization);

    let query = fxp_vec(&[2048; 8]);
    let k = 5;
    let mut orig_res = vec![SearchResult::default(); k];
    let mut rest_res = vec![SearchResult::default(); k];
    let c1 = state.search_l2(&query, &mut orig_res, None);
    let c2 = restored.search_l2(&query, &mut rest_res, None);

    assert_eq!(c1, c2);
    for i in 0..c1 {
        assert_eq!(
            orig_res[i].id, rest_res[i].id,
            "result[{i}] id mismatch after snapshot restore"
        );
    }
}
