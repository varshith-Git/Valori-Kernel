// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! L2 search: exact-match retrieval, deterministic ordering, tag filtering.

use valori_kernel::event::KernelEvent;
use valori_kernel::index::SearchResult;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

const DIM: usize = 4;

fn fxp(v: &[i32]) -> FxpVector {
    FxpVector {
        data: v.iter().map(|&x| FxpScalar(x << 16)).collect(),
    }
}

fn populated() -> KernelState {
    let mut state = KernelState::new();
    let points: [&[i32]; 4] = [&[0, 0, 0, 0], &[1, 0, 0, 0], &[0, 5, 0, 0], &[9, 9, 9, 9]];
    for (i, p) in points.iter().enumerate() {
        state
            .apply_event(&KernelEvent::InsertRecord {
                id: RecordId(i as u32),
                vector: fxp(p),
                metadata: None,
                tag: (i % 2) as u64,
            })
            .unwrap();
    }
    state
}

fn search(state: &KernelState, query: &FxpVector, k: usize, filter: Option<u64>) -> Vec<u32> {
    let mut buf = vec![
        SearchResult {
            id: RecordId(0),
            score: i64::MAX
        };
        k
    ];
    let found = state.search_l2(query, &mut buf, filter);
    buf.truncate(found);
    buf.iter().map(|r| r.id.0).collect()
}

#[test]
fn exact_match_ranks_first() {
    let state = populated();
    let hits = search(&state, &fxp(&[1, 0, 0, 0]), 4, None);
    assert_eq!(hits[0], 1, "exact match must be the top result");
}

#[test]
fn search_is_deterministic() {
    let state = populated();
    let q = fxp(&[1, 1, 0, 0]);
    let a = search(&state, &q, 4, None);
    let b = search(&state, &q, 4, None);
    assert_eq!(a, b);

    // And across an independently rebuilt state:
    let c = search(&populated(), &q, 4, None);
    assert_eq!(a, c);
}

#[test]
fn tag_filter_excludes_other_tags() {
    let state = populated();
    // tag 1 → records 1 and 3 only
    let hits = search(&state, &fxp(&[0, 0, 0, 0]), 4, Some(1));
    assert!(!hits.is_empty());
    for id in &hits {
        assert!(*id == 1 || *id == 3, "tag filter leaked record {id}");
    }
}

#[test]
fn k_larger_than_corpus_returns_all() {
    let state = populated();
    let hits = search(&state, &fxp(&[0, 0, 0, 0]), 16, None);
    assert_eq!(hits.len(), 4);
}
