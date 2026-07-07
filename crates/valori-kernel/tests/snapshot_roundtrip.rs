// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Snapshot encode → decode round-trips must reproduce the exact state
//! (verified by hash), and corrupted snapshots must be rejected.

use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::{encode_state, encode_capacity_hint};
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

fn encode(state: &KernelState) -> Vec<u8> {
    let mut buf = Vec::with_capacity(encode_capacity_hint(state));
    encode_state(state, &mut buf).expect("encode");
    buf
}

#[test]
fn roundtrip_preserves_state_hash() {
    let state = populated_state();
    let hash_before = hash_state_blake3(&state);

    let buf = encode(&state);
    let restored = decode_state(&buf).expect("decode");
    assert_eq!(hash_state_blake3(&restored), hash_before);
    assert_eq!(restored.record_count(), state.record_count());
    assert_eq!(restored.node_count(), state.node_count());
    assert_eq!(restored.edge_count(), state.edge_count());
}

#[test]
fn restored_state_continues_sequencing() {
    let state = populated_state();
    let buf = encode(&state);

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
    let mut buf = encode(&state);
    buf[0] ^= 0xFF;
    assert!(decode_state(&buf).is_err());
}

#[test]
fn truncated_snapshot_is_rejected() {
    let state = populated_state();
    let buf = encode(&state);
    assert!(decode_state(&buf[..buf.len() / 2]).is_err());
}

#[test]
fn empty_state_roundtrips() {
    let state = KernelState::new();
    let buf = encode(&state);
    let restored = decode_state(&buf).unwrap();
    assert_eq!(hash_state_blake3(&restored), hash_state_blake3(&state));
}

#[test]
fn v7_meta_roundtrips() {
    // SetMeta-committed key/value pairs must survive a snapshot round-trip —
    // this is what makes standalone document metadata durable across
    // snapshot-based recovery, not just full event-log replay.
    let mut state = populated_state();
    state
        .apply_event(&KernelEvent::SetMeta {
            key: "document:0".into(),
            value: r#"{"filename":"Composer2.pdf","collection":"firstone--c1"}"#.into(),
        })
        .unwrap();
    state
        .apply_event(&KernelEvent::SetMeta {
            key: "record:1".into(),
            value: r#"{"text":"chunk body"}"#.into(),
        })
        .unwrap();

    let buf = encode(&state);
    let restored = decode_state(&buf).unwrap();

    assert_eq!(restored.meta.len(), 2);
    assert_eq!(
        restored.meta.get("document:0").map(String::as_str),
        Some(r#"{"filename":"Composer2.pdf","collection":"firstone--c1"}"#)
    );
    assert_eq!(
        restored.meta.get("record:1").map(String::as_str),
        Some(r#"{"text":"chunk body"}"#)
    );
}
