use crate::types::vector::FxpVector;
use crate::storage::pool::RecordPool;
use crate::index::{SearchResult, VectorIndex};
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::index::brute_force::BruteForceIndex;
use crate::types::id::RecordId;
use crate::types::scalar::FxpScalar;
use crate::fxp::ops::from_f32;

#[test]
fn test_brute_force_search() {
    const CAP: usize = 10;
    const D: usize = 2;
    let mut pool = RecordPool::<CAP, D>::new();
    let index = BruteForceIndex::default();

    let v0 = FxpVector::new_zeros();
    pool.insert(v0).unwrap();

    let v1 = FxpVector { data: [from_f32(10.0), FxpScalar::ZERO] };
    pool.insert(v1).unwrap();

    let v2 = FxpVector { data: [from_f32(2.0), FxpScalar::ZERO] };
    pool.insert(v2).unwrap();

    let query = FxpVector { data: [from_f32(1.0), FxpScalar::ZERO] };
    
    let mut results = [SearchResult::default(); 2];
    let count = index.search(&pool, &query, &mut results);
    
    assert_eq!(count, 2);
    
    // Check results (sorted by score then ID)
    // Both have score 1.0. ID 0 and ID 2.
    // Order: ID 0, then ID 2.
    assert_eq!(results[0].id, RecordId(0));
    assert_eq!(results[0].score, from_f32(1.0));
    
    assert_eq!(results[1].id, RecordId(2));
    assert_eq!(results[1].score, from_f32(1.0));
}

#[test]
fn test_tie_breaking_order() {
    const CAP: usize = 5;
    const D: usize = 1;
    let mut pool = RecordPool::<CAP, D>::new();
    let index = BruteForceIndex::default();

    pool.insert(FxpVector::new_zeros()).unwrap();
    pool.insert(FxpVector::new_zeros()).unwrap();

    let query = FxpVector::new_zeros();
    let mut results = [SearchResult::default(); 1];
    
    index.search(&pool, &query, &mut results);

    assert_eq!(results[0].id, RecordId(0));
}
