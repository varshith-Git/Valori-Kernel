// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The core invariant: identical event sequences produce bit-identical
//! state hashes; any divergence in content or order changes the hash.

use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::id::RecordId;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

const DIM: usize = 8;

fn vec_from_seed(seed: u32) -> FxpVector {
    let data = (0..DIM)
        .map(|i| FxpScalar(((seed.wrapping_mul(2654435761).wrapping_add(i as u32)) % 65536) as i32 - 32768))
        .collect();
    FxpVector { data }
}

fn build_events(n: u32) -> Vec<KernelEvent> {
    (0..n)
        .map(|i| KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: vec_from_seed(i),
            metadata: Some(alloc_meta(i)),
            tag: i as u64 % 7,
        })
        .collect()
}

fn alloc_meta(i: u32) -> Vec<u8> {
    format!("{{\"n\":{i}}}").into_bytes()
}

fn replay(events: &[KernelEvent]) -> [u8; 32] {
    let mut state = KernelState::new();
    for e in events {
        state.apply_event(e).unwrap();
    }
    hash_state_blake3(&state)
}

#[test]
fn same_events_same_hash() {
    let events = build_events(200);
    assert_eq!(replay(&events), replay(&events));
}

#[test]
fn hash_is_stable_across_fresh_states() {
    // Two completely independent state instances, no shared allocations.
    let h1 = replay(&build_events(50));
    let h2 = replay(&build_events(50));
    assert_eq!(h1, h2);
}

#[test]
fn single_scalar_change_changes_hash() {
    let mut a = build_events(50);
    let b = build_events(50);
    if let KernelEvent::InsertRecord { vector, .. } = &mut a[25] {
        vector.data[0] = FxpScalar(vector.data[0].0.wrapping_add(1));
    }
    assert_ne!(replay(&a), replay(&b));
}

#[test]
fn metadata_change_changes_hash() {
    let mut a = build_events(50);
    let b = build_events(50);
    if let KernelEvent::InsertRecord { metadata, .. } = &mut a[10] {
        *metadata = Some(b"tampered".to_vec());
    }
    assert_ne!(replay(&a), replay(&b));
}

#[test]
fn tag_change_changes_hash() {
    let mut a = build_events(50);
    let b = build_events(50);
    if let KernelEvent::InsertRecord { tag, .. } = &mut a[33] {
        *tag = 999;
    }
    assert_ne!(replay(&a), replay(&b));
}

#[test]
fn empty_state_hash_is_constant() {
    let h1 = hash_state_blake3(&KernelState::new());
    let h2 = hash_state_blake3(&KernelState::new());
    assert_eq!(h1, h2);
    assert_ne!(h1, [0u8; 32]);
}
