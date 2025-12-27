use crate::types::vector::FxpVector;
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::storage::pool::RecordPool;
use crate::types::id::RecordId;
use crate::error::KernelError;
use std::vec::Vec;

#[test]
fn test_pool_insert_delete() {
    const CAP: usize = 4;
    const D: usize = 2;
    let mut pool = RecordPool::<CAP, D>::new();

    // Insert 0
    let v0 = FxpVector::new_zeros();
    let id0 = pool.insert(v0, None).unwrap();
    assert_eq!(id0, RecordId(0));

    // Insert 1
    let id1 = pool.insert(v0, None).unwrap();
    assert_eq!(id1, RecordId(1));

    // Delete 0
    pool.delete(id0).unwrap();
    assert!(pool.get(id0).is_none());

    // Insert should reuse slot 0 (deterministic first-fit)
    let id2 = pool.insert(v0, None).unwrap();
    assert_eq!(id2, RecordId(0));
}

#[test]
fn test_pool_capacity() {
    const CAP: usize = 2;
    const D: usize = 2;
    let mut pool = RecordPool::<CAP, D>::new();
    let v = FxpVector::new_zeros();

    pool.insert(v, None).unwrap();
    pool.insert(v, None).unwrap();
    
    // Should fail
    let res = pool.insert(v, None);
    match res {
        Err(KernelError::CapacityExceeded) => (),
        _ => panic!("Expected CapacityExceeded"),
    }
}

#[test]
fn test_pool_iter() {
    const CAP: usize = 5;
    const D: usize = 1;
    let mut pool = RecordPool::<CAP, D>::new();
    let v = FxpVector::new_zeros();

    pool.insert(v, None).unwrap(); // 0
    pool.insert(v, None).unwrap(); // 1
    pool.insert(v, None).unwrap(); // 2

    pool.delete(RecordId(1)).unwrap();

    let ids: Vec<u32> = pool.iter().map(|r| r.id.0).collect();
    // Should be [0, 2]
    assert_eq!(ids, vec![0, 2]);
}
