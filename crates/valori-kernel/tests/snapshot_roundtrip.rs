// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Snapshot encode → decode round-trips must reproduce the exact state
//! (verified by hash), and corrupted snapshots must be rejected.

use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::encode_state;
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::{EdgeId, NodeId, RecordId};
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

const DIM: usize = 4;

fn populated_state() -> KernelState {
    let mut state = KernelState::new();
    for i in 0u32..20 {
        let data = (0..DIM).map(|d| FxpScalar((i * 100 + d as u32) as i32)).collect();
        state
            .apply_event(&KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector { data },
                metadata: if i % 3 == 0 { Some(vec![i as u8; 8]) } else { None },
                tag: i as u64,
            })
            .unwrap();
    }
    for i in 0u32..4 {
        state
            .apply_event(&KernelEvent::CreateNode {
                id: NodeId(i),
                kind: NodeKind::Concept,
                record: Some(RecordId(i)),
            })
            .unwrap();
    }
    for (e, (f, t)) in [(0u32, (0u32, 1u32)), (1, (1, 2)), (2, (2, 3))] {
        state
            .apply_event(&KernelEvent::CreateEdge {
                id: EdgeId(e),
                kind: EdgeKind::Relation,
                from: NodeId(f),
                to: NodeId(t),
            })
            .unwrap();
    }
    state
}

#[test]
fn roundtrip_preserves_state_hash() {
    let state = populated_state();
    let hash_before = hash_state_blake3(&state);

    let mut buf = vec![0u8; 1 << 20];
    let len = encode_state(&state, &mut buf).expect("encode");
    buf.truncate(len);

    let restored = decode_state(&buf).expect("decode");
    assert_eq!(hash_state_blake3(&restored), hash_before);
    assert_eq!(restored.record_count(), state.record_count());
    assert_eq!(restored.node_count(), state.node_count());
    assert_eq!(restored.edge_count(), state.edge_count());
}

#[test]
fn restored_state_continues_sequencing() {
    // A restored snapshot must accept the NEXT sequential id — this is what
    // crash recovery and follower bootstrap rely on.
    let state = populated_state();
    let mut buf = vec![0u8; 1 << 20];
    let len = encode_state(&state, &mut buf).unwrap();
    buf.truncate(len);

    let mut restored = decode_state(&buf).unwrap();
    restored
        .apply_event(&KernelEvent::InsertRecord {
            id: RecordId(20),
            vector: FxpVector::new_zeros(DIM),
            metadata: None,
            tag: 0,
        })
        .expect("restored state must continue the id sequence");
}

#[test]
fn corrupt_magic_is_rejected() {
    let state = populated_state();
    let mut buf = vec![0u8; 1 << 20];
    let len = encode_state(&state, &mut buf).unwrap();
    buf.truncate(len);

    buf[0] ^= 0xFF;
    assert!(decode_state(&buf).is_err());
}

#[test]
fn truncated_snapshot_is_rejected() {
    let state = populated_state();
    let mut buf = vec![0u8; 1 << 20];
    let len = encode_state(&state, &mut buf).unwrap();
    buf.truncate(len / 2);
    assert!(decode_state(&buf).is_err());
}

#[test]
fn empty_state_roundtrips() {
    let state = KernelState::new();
    let mut buf = vec![0u8; 4096];
    let len = encode_state(&state, &mut buf).unwrap();
    buf.truncate(len);
    let restored = decode_state(&buf).unwrap();
    assert_eq!(hash_state_blake3(&restored), hash_state_blake3(&state));
}
