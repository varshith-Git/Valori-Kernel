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

// ── Phase 2: Snapshot bit-for-bit stability + replay round-trip ──────────────

use valori_kernel::snapshot::encode::encode_state;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::types::enums::{NodeKind, EdgeKind};
use valori_kernel::types::id::{NodeId, EdgeId};

fn encode(state: &KernelState) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_state(state, &mut buf).expect("encode");
    buf
}

/// A more complex state: records + graph nodes + edges + meta.
fn complex_state() -> KernelState {
    let mut s = KernelState::new();
    for i in 0u32..40 {
        s.apply_event(&KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: vec_from_seed(i),
            metadata: Some(format!("{{\"i\":{i}}}").into_bytes()),
            tag: i as u64 % 5,
        })
        .unwrap();
    }
    for i in 0u32..8 {
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(i),
            kind: NodeKind::Document,
            record: Some(RecordId(i)),
        })
        .unwrap();
    }
    for i in 0u32..4 {
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(i),
            kind: EdgeKind::Supersedes,
            from: NodeId(i * 2),
            to: NodeId(i * 2 + 1),
        })
        .unwrap();
    }
    s.apply_event(&KernelEvent::SetMeta {
        key: "corpus:version".into(),
        value: "1.0.0".into(),
    })
    .unwrap();
    s
}

// ── 2.2: Snapshot byte-for-bit stability ─────────────────────────────────────

#[test]
fn snapshot_is_bit_identical_across_three_encodes() {
    let state = complex_state();
    let b1 = encode(&state);
    let b2 = encode(&state);
    let b3 = encode(&state);
    assert_eq!(b1, b2, "encode 1 vs 2 differ");
    assert_eq!(b2, b3, "encode 2 vs 3 differ");
}

#[test]
fn snapshot_hash_matches_state_hash_after_restore() {
    let state = complex_state();
    let hash_before = hash_state_blake3(&state);
    let buf = encode(&state);
    let restored = decode_state(&buf).expect("decode");
    assert_eq!(hash_state_blake3(&restored), hash_before);
}

#[test]
fn two_identical_builds_produce_identical_snapshot_bytes() {
    // Build the same logical state via two independent KernelState instances.
    let b1 = encode(&complex_state());
    let b2 = encode(&complex_state());
    assert_eq!(b1, b2, "identically-built states must produce identical snapshot bytes");
}

// ── 2.3: Replay round-trip determinism ───────────────────────────────────────

#[test]
fn replay_produces_same_hash_and_record_count() {
    let events: Vec<KernelEvent> = (0u32..80).map(|i| KernelEvent::InsertRecord {
        id: RecordId(i),
        vector: vec_from_seed(i.wrapping_mul(31337)),
        metadata: if i % 4 == 0 { Some(vec![i as u8]) } else { None },
        tag: i as u64 % 3,
    }).collect();

    // Original state
    let mut origin = KernelState::new();
    for e in &events { origin.apply_event(e).unwrap(); }

    // Snapshot → restore
    let snap = encode(&origin);
    let restored = decode_state(&snap).unwrap();

    // Replay from scratch (re-apply all events to a fresh state)
    let mut replayed = KernelState::new();
    for e in &events { replayed.apply_event(e).unwrap(); }

    assert_eq!(hash_state_blake3(&origin), hash_state_blake3(&restored));
    assert_eq!(hash_state_blake3(&origin), hash_state_blake3(&replayed));
    assert_eq!(origin.record_count(), restored.record_count());
    assert_eq!(origin.record_count(), replayed.record_count());
}

#[test]
fn interleaved_insert_delete_replay_matches() {
    // Interleaved insert + soft-delete: a harder replay target.
    let mut events: Vec<KernelEvent> = Vec::new();
    for i in 0u32..50 {
        events.push(KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: vec_from_seed(i),
            metadata: None,
            tag: 0,
        });
    }
    // Soft-delete every third record.
    for i in (0u32..50).step_by(3) {
        events.push(KernelEvent::SoftDeleteRecord { id: RecordId(i) });
    }

    let mut origin = KernelState::new();
    for e in &events { origin.apply_event(e).unwrap(); }

    let snap = encode(&origin);
    let restored = decode_state(&snap).unwrap();

    let mut replayed = KernelState::new();
    for e in &events { replayed.apply_event(e).unwrap(); }

    assert_eq!(hash_state_blake3(&origin), hash_state_blake3(&restored));
    assert_eq!(hash_state_blake3(&origin), hash_state_blake3(&replayed));
}

#[test]
fn snapshot_of_restored_state_is_identical_to_original_snapshot() {
    // Encode → decode → re-encode must reproduce the exact same bytes.
    // This is a stronger claim than hash equality: the binary format is stable.
    let state  = complex_state();
    let snap1  = encode(&state);
    let restored = decode_state(&snap1).unwrap();
    let snap2  = encode(&restored);
    assert_eq!(snap1, snap2, "encode(decode(encode(state))) must equal encode(state)");
}
