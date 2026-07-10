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

// ── Decoder hardening tests ───────────────────────────────────────────────────
// Each test crafts a minimally-valid snapshot then mutates one field to an
// illegal value and verifies that decode_state returns Err.

fn valid_one_record_snapshot() -> Vec<u8> {
    let mut state = KernelState::new();
    state
        .apply_event(&KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector { data: vec![FxpScalar(1), FxpScalar(2)] },
            metadata: None,
            tag: 0,
        })
        .unwrap();
    encode(&state)
}

// Byte offsets in a current (V7) snapshot:
//   0..4   MAGIC
//   4..8   schema_ver
//   8..16  version_val
//  16..20  cap_records  (total_slots mirror in header)
//  20..24  dim
//  24..28  cap_nodes
//  28..32  cap_edges
//  32      format_id    (V5+)
//  33..37  total_slots  (records section header)
//  37      is_present   (first record slot flag)
const OFF_DIM:         usize = 20;
const OFF_TOTAL_SLOTS: usize = 33;
const OFF_IS_PRESENT:  usize = 37;

#[test]
fn invalid_is_present_flag_is_rejected() {
    let mut buf = valid_one_record_snapshot();
    buf[OFF_IS_PRESENT] = 2; // must be 0 or 1
    assert!(decode_state(&buf).is_err(), "is_present=2 must be rejected");
}

#[test]
fn oversized_dim_is_rejected() {
    let state = KernelState::new();
    let mut buf = encode(&state);
    let bad_dim: u32 = 65537; // MAX_DIM + 1
    buf[OFF_DIM..OFF_DIM + 4].copy_from_slice(&bad_dim.to_le_bytes());
    assert!(decode_state(&buf).is_err(), "dim > MAX_DIM must be rejected");
}

#[test]
fn oversized_total_slots_is_rejected() {
    let state = KernelState::new();
    let mut buf = encode(&state);
    let bad_slots: u32 = 10_000_001; // > MAX_RECORDS (10_000_000)
    buf[OFF_TOTAL_SLOTS..OFF_TOTAL_SLOTS + 4].copy_from_slice(&bad_slots.to_le_bytes());
    assert!(decode_state(&buf).is_err(), "total_slots > MAX_RECORDS must be rejected");
}

#[test]
fn record_id_mismatch_is_rejected() {
    let mut buf = valid_one_record_snapshot();
    // id_val for slot 0 is the u32 immediately after is_present.
    let id_offset = OFF_IS_PRESENT + 1;
    let wrong_id: u32 = 99; // slot 0 must have id 0
    buf[id_offset..id_offset + 4].copy_from_slice(&wrong_id.to_le_bytes());
    assert!(decode_state(&buf).is_err(), "record id != slot index must be rejected");
}

#[test]
fn unsupported_schema_version_is_rejected() {
    let state = KernelState::new();
    let mut buf = encode(&state);
    // schema_ver is at offset 4 (after MAGIC).
    let bad_ver: u32 = 99;
    buf[4..8].copy_from_slice(&bad_ver.to_le_bytes());
    assert!(decode_state(&buf).is_err(), "schema_ver 99 must be rejected");
}
