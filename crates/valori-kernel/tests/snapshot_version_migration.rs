// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! K4 — cross-version snapshot migration.
//!
//! `decode_state` accepts `schema_ver` 1..=7 and has real, distinct
//! conditional branches per version (tag @V3, metadata @V2, incoming-edge
//! back-pointers @V4 — reconstructed for older files, arithmetic-format
//! byte @V5, namespace fields @V6, meta sidecar @V7). Every snapshot test
//! that existed before this file only ever exercised the CURRENT encoder,
//! which always writes V7 — `tests/snapshot_compat.rs`'s "forever" fixtures
//! are V7-only, `tests/snapshot_roundtrip.rs` round-trips only the current
//! format, and `tests/format.rs` has one V5 case. None of them constructs
//! genuine V1..V6 bytes, so every backward-compat branch in `decode.rs` for
//! schema_ver < 7 was live code with zero test coverage.
//!
//! (The original ask described this gap as "V1→V2→V3" — reading
//! `decode.rs` shows the real range is V1..V7 with five distinct feature
//! cutovers, not three, so coverage here spans the full range instead of
//! stopping at V3.)
//!
//! This file hand-encodes each historical wire format — mirroring
//! `encode::encode_state`'s V7 layout, trimmed to exactly what
//! `decode_state` reads at each `schema_ver` — and verifies two things:
//!   1. every field lands at its correct historical value or default,
//!   2. decoding an old snapshot and re-encoding it (always via the
//!      current encoder) is a lossless, hash-stable fixed point — i.e.
//!      migrating an old snapshot forward is safe to do repeatedly.

use valori_kernel::event::KernelEvent;
use valori_kernel::fxp::format::ACTIVE_FORMAT_ID;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::{encode_capacity_hint, encode_state, MAGIC};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::{EdgeId, NodeId, RecordId, MAX_NAMESPACES, NS_LIST_NIL};
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

const DIM: u32 = 4;

// ── Hand-rolled legacy encoder ──────────────────────────────────────────────
// Mirrors `snapshot::encode::encode_state`'s field order and byte widths
// exactly (verified line-for-line against encode.rs and decode.rs), but only
// emits the fields decode.rs actually reads for a given `schema_ver`. This
// is what a genuine VN encoder would have produced.

struct LegacyRecord {
    id: u32,
    flags: u8,
    tag: u64,
    vector: Vec<i32>,
    metadata: Option<Vec<u8>>,
}

struct LegacyNode {
    id: u32,
    kind: u8,
    record: Option<u32>,
    first_out: Option<u32>,
    first_in: Option<u32>,
}

struct LegacyEdge {
    id: u32,
    kind: u8,
    from: u32,
    to: u32,
    next_out: Option<u32>,
    next_in: Option<u32>,
}

fn encode_legacy(
    schema_ver: u32,
    version_val: u64,
    dim: u32,
    records: &[Option<LegacyRecord>], // one entry per slot; None = hole (is_present = 0)
    nodes: &[LegacyNode],
    edges: &[LegacyEdge],
) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();

    // ── Header ───────────────────────────────────────────────────────────
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&schema_ver.to_le_bytes());
    out.extend_from_slice(&version_val.to_le_bytes());
    out.extend_from_slice(&(records.len() as u32).to_le_bytes()); // legacy cap, decoder discards
    out.extend_from_slice(&dim.to_le_bytes());
    out.extend_from_slice(&(nodes.len() as u32).to_le_bytes()); // legacy cap, discarded
    out.extend_from_slice(&(edges.len() as u32).to_le_bytes()); // legacy cap, discarded

    if schema_ver >= 5 {
        out.push(ACTIVE_FORMAT_ID);
    }

    // ── Records ──────────────────────────────────────────────────────────
    out.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for slot in records {
        match slot {
            None => out.push(0),
            Some(r) => {
                out.push(1);
                out.extend_from_slice(&r.id.to_le_bytes());
                out.push(r.flags);
                if schema_ver >= 3 {
                    out.extend_from_slice(&r.tag.to_le_bytes());
                }
                assert_eq!(r.vector.len(), dim as usize, "test bug: vector/dim mismatch");
                for v in &r.vector {
                    out.extend_from_slice(&v.to_le_bytes());
                }
                if schema_ver >= 2 {
                    match &r.metadata {
                        Some(m) => {
                            out.extend_from_slice(&(m.len() as u32).to_le_bytes());
                            out.extend_from_slice(m);
                        }
                        None => out.extend_from_slice(&0u32.to_le_bytes()),
                    }
                }
                if schema_ver >= 6 {
                    out.extend_from_slice(&0u16.to_le_bytes()); // namespace_id
                    out.extend_from_slice(&NS_LIST_NIL.to_le_bytes()); // next_in_ns
                    out.extend_from_slice(&NS_LIST_NIL.to_le_bytes()); // prev_in_ns
                }
            }
        }
    }

    // ── Nodes ────────────────────────────────────────────────────────────
    out.extend_from_slice(&(nodes.len() as u32).to_le_bytes());
    for n in nodes {
        out.extend_from_slice(&n.id.to_le_bytes());
        out.push(n.kind);
        match n.record {
            Some(r) => { out.push(1); out.extend_from_slice(&r.to_le_bytes()); }
            None => out.push(0),
        }
        match n.first_out {
            Some(e) => { out.push(1); out.extend_from_slice(&e.to_le_bytes()); }
            None => out.push(0),
        }
        if schema_ver >= 4 {
            match n.first_in {
                Some(e) => { out.push(1); out.extend_from_slice(&e.to_le_bytes()); }
                None => out.push(0),
            }
        }
        if schema_ver >= 6 {
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&NS_LIST_NIL.to_le_bytes());
            out.extend_from_slice(&NS_LIST_NIL.to_le_bytes());
        }
    }

    // ── Edges ────────────────────────────────────────────────────────────
    out.extend_from_slice(&(edges.len() as u32).to_le_bytes());
    for e in edges {
        out.extend_from_slice(&e.id.to_le_bytes());
        out.push(e.kind);
        out.extend_from_slice(&e.from.to_le_bytes());
        out.extend_from_slice(&e.to.to_le_bytes());
        match e.next_out {
            Some(x) => { out.push(1); out.extend_from_slice(&x.to_le_bytes()); }
            None => out.push(0),
        }
        if schema_ver >= 4 {
            match e.next_in {
                Some(x) => { out.push(1); out.extend_from_slice(&x.to_le_bytes()); }
                None => out.push(0),
            }
        }
    }

    // ── V6+: namespace head arrays ──────────────────────────────────────
    if schema_ver >= 6 {
        for _ in 0..MAX_NAMESPACES { out.extend_from_slice(&NS_LIST_NIL.to_le_bytes()); }
        for _ in 0..MAX_NAMESPACES { out.extend_from_slice(&NS_LIST_NIL.to_le_bytes()); }
    }

    // ── V7+: meta sidecar (empty — not under test here) ─────────────────
    if schema_ver >= 7 {
        out.extend_from_slice(&0u32.to_le_bytes());
    }

    out
}

// ── Scenario A: 2 records, 2 nodes, 1 edge ──────────────────────────────────
// Reused across every version. `node1`'s incoming-edge back-pointer is the
// interesting bit: for schema_ver < 4 it must be *reconstructed* by
// decode.rs's V1-V3 back-compat block; for schema_ver >= 4 it is read
// directly off the wire. Both paths must land on the same value.

fn scenario_a_records() -> Vec<Option<LegacyRecord>> {
    vec![
        Some(LegacyRecord { id: 0, flags: 0, tag: 7, vector: vec![10, -20, 30, -40], metadata: Some(b"meta-a".to_vec()) }),
        Some(LegacyRecord { id: 1, flags: 0, tag: 99, vector: vec![1, 2, 3, 4], metadata: None }),
    ]
}

fn scenario_a_nodes() -> Vec<LegacyNode> {
    vec![
        LegacyNode { id: 0, kind: NodeKind::Document as u8, record: Some(0), first_out: Some(0), first_in: None },
        LegacyNode { id: 1, kind: NodeKind::Concept as u8, record: Some(1), first_out: None, first_in: Some(0) },
    ]
}

fn scenario_a_edges() -> Vec<LegacyEdge> {
    vec![LegacyEdge { id: 0, kind: EdgeKind::Relation as u8, from: 0, to: 1, next_out: None, next_in: None }]
}

/// The state a real VN encoder would have captured, built purely through
/// the public event API (never touches decode/encode internals) — this is
/// the independent "known good" reference every legacy buffer is checked
/// against. `tag_supported`/`metadata_supported` model what that historical
/// version actually had room to store; older versions genuinely lose that
/// data, so the reference must reflect the same loss, not paper over it.
fn scenario_a_reference(tag_supported: bool, metadata_supported: bool) -> KernelState {
    let mut s = KernelState::new();
    s.apply_event(&KernelEvent::InsertRecord {
        id: RecordId(0),
        vector: FxpVector { data: vec![FxpScalar(10), FxpScalar(-20), FxpScalar(30), FxpScalar(-40)] },
        metadata: if metadata_supported { Some(b"meta-a".to_vec()) } else { None },
        tag: if tag_supported { 7 } else { 0 },
    }).unwrap();
    s.apply_event(&KernelEvent::InsertRecord {
        id: RecordId(1),
        vector: FxpVector { data: vec![FxpScalar(1), FxpScalar(2), FxpScalar(3), FxpScalar(4)] },
        metadata: None,
        tag: if tag_supported { 99 } else { 0 },
    }).unwrap();
    s.apply_event(&KernelEvent::CreateNode { id: NodeId(0), kind: NodeKind::Document, record: Some(RecordId(0)) }).unwrap();
    s.apply_event(&KernelEvent::CreateNode { id: NodeId(1), kind: NodeKind::Concept, record: Some(RecordId(1)) }).unwrap();
    s.apply_event(&KernelEvent::CreateEdge { id: EdgeId(0), kind: EdgeKind::Relation, from: NodeId(0), to: NodeId(1) }).unwrap();
    s
}

fn encode_legacy_scenario_a(schema_ver: u32) -> Vec<u8> {
    let version_val = scenario_a_reference(schema_ver >= 3, schema_ver >= 2).version();
    encode_legacy(schema_ver, version_val, DIM, &scenario_a_records(), &scenario_a_nodes(), &scenario_a_edges())
}

/// Field-level assertions any decoded Scenario A state must satisfy, with
/// the version-gated fields (tag, metadata) checked against what that
/// version could actually store.
fn assert_scenario_a(state: &KernelState, schema_ver: u32) {
    assert_eq!(state.record_count(), 2, "schema_ver {schema_ver}");
    assert_eq!(state.node_count(), 2, "schema_ver {schema_ver}");
    assert_eq!(state.edge_count(), 1, "schema_ver {schema_ver}");

    let r0 = state.get_record(RecordId(0)).expect("record 0 must exist");
    let r1 = state.get_record(RecordId(1)).expect("record 1 must exist");
    assert_eq!(r0.vector.data, vec![FxpScalar(10), FxpScalar(-20), FxpScalar(30), FxpScalar(-40)]);
    assert_eq!(r1.vector.data, vec![FxpScalar(1), FxpScalar(2), FxpScalar(3), FxpScalar(4)]);

    if schema_ver >= 3 {
        assert_eq!(r0.tag, 7, "schema_ver {schema_ver} has tag support — must round-trip exactly");
        assert_eq!(r1.tag, 99, "schema_ver {schema_ver} has tag support — must round-trip exactly");
    } else {
        assert_eq!(r0.tag, 0, "schema_ver {schema_ver} predates tag — must default to 0, never leak garbage");
        assert_eq!(r1.tag, 0, "schema_ver {schema_ver} predates tag — must default to 0, never leak garbage");
    }

    if schema_ver >= 2 {
        assert_eq!(r0.metadata.as_deref(), Some(&b"meta-a"[..]), "schema_ver {schema_ver}");
        assert_eq!(r1.metadata, None, "schema_ver {schema_ver}");
    } else {
        assert_eq!(r0.metadata, None, "schema_ver {schema_ver} predates metadata — must default to None");
    }

    assert_eq!(r0.namespace_id, 0, "schema_ver {schema_ver}");
    assert_eq!(r1.namespace_id, 0, "schema_ver {schema_ver}");

    let node0 = state.get_node(NodeId(0)).expect("node 0 must exist");
    let node1 = state.get_node(NodeId(1)).expect("node 1 must exist");
    assert_eq!(node0.first_out_edge, Some(EdgeId(0)), "schema_ver {schema_ver}");
    assert_eq!(
        node1.first_in_edge,
        Some(EdgeId(0)),
        "schema_ver {schema_ver}: incoming-edge back-pointer must be present — explicit on the \
         wire for V4+, reconstructed by the V1-V3 back-compat block otherwise"
    );

    let edge0 = state.get_edge(EdgeId(0)).expect("edge 0 must exist");
    assert_eq!(edge0.from, NodeId(0), "schema_ver {schema_ver}");
    assert_eq!(edge0.to, NodeId(1), "schema_ver {schema_ver}");
}

// ── Per-version decode correctness ──────────────────────────────────────────
// V7 is already covered by tests/snapshot_compat.rs's forever-fixtures; V1-V6
// had zero coverage before this file.

#[test]
fn v1_decodes_correctly() {
    let state = decode_state(&encode_legacy_scenario_a(1)).expect("V1 buffer must decode");
    assert_scenario_a(&state, 1);
}

#[test]
fn v2_decodes_correctly() {
    let state = decode_state(&encode_legacy_scenario_a(2)).expect("V2 buffer must decode");
    assert_scenario_a(&state, 2);
}

#[test]
fn v3_decodes_correctly() {
    let state = decode_state(&encode_legacy_scenario_a(3)).expect("V3 buffer must decode");
    assert_scenario_a(&state, 3);
}

#[test]
fn v4_decodes_correctly() {
    let state = decode_state(&encode_legacy_scenario_a(4)).expect("V4 buffer must decode");
    assert_scenario_a(&state, 4);
}

#[test]
fn v5_decodes_correctly() {
    let state = decode_state(&encode_legacy_scenario_a(5)).expect("V5 buffer must decode");
    assert_scenario_a(&state, 5);
}

#[test]
fn v6_decodes_correctly() {
    let state = decode_state(&encode_legacy_scenario_a(6)).expect("V6 buffer must decode");
    assert_scenario_a(&state, 6);
}

// ── Hole (absent slot) handling — version-independent code path ────────────
// `is_present` is read unconditionally regardless of schema_ver, so one
// version is enough to cover the branch; V1 exercises the oldest path.

#[test]
fn v1_hole_slot_decodes_as_absent_without_shifting_ids() {
    let records = vec![
        Some(LegacyRecord { id: 0, flags: 0, tag: 0, vector: vec![5, 5, 5, 5], metadata: None }),
        None, // hole: is_present = 0
        Some(LegacyRecord { id: 2, flags: 0, tag: 0, vector: vec![9, 9, 9, 9], metadata: None }),
    ];
    let buf = encode_legacy(1, 0, DIM, &records, &[], &[]);
    let state = decode_state(&buf).expect("V1 buffer with a hole must decode");

    assert_eq!(state.total_record_slots(), 3);
    assert_eq!(state.record_count(), 2, "the hole slot must not count as a live record");
    assert!(state.get_record(RecordId(1)).is_none(), "hole slot must decode to None");
    assert_eq!(state.get_record(RecordId(0)).unwrap().vector.data, vec![FxpScalar(5); 4]);
    assert_eq!(state.get_record(RecordId(2)).unwrap().vector.data, vec![FxpScalar(9); 4]);
}

// ── Cross-version migration: decode → re-encode(current) → decode chain ────
// This is the actual "migrate an old snapshot forward" behavior: every
// version from 1 through 7 must decode to a state whose hash matches an
// independently-built reference, and re-encoding that state with the
// CURRENT encoder (always V7) must be a lossless, then idempotent, step.

#[test]
fn cross_version_decode_reencode_chain_is_hash_stable() {
    for schema_ver in 1u32..=7 {
        let tag_supported = schema_ver >= 3;
        let metadata_supported = schema_ver >= 2;

        let buf_old = encode_legacy_scenario_a(schema_ver);
        let decoded_old = decode_state(&buf_old)
            .unwrap_or_else(|_| panic!("schema_ver {schema_ver} must decode"));
        assert_scenario_a(&decoded_old, schema_ver);

        let reference = scenario_a_reference(tag_supported, metadata_supported);
        assert_eq!(
            hash_state_blake3(&decoded_old),
            hash_state_blake3(&reference),
            "schema_ver {schema_ver}: decoded hash must match an equivalent state built via the public event API"
        );

        // Migration step: re-encode with the CURRENT encoder (always V7), decode again.
        let mut buf_migrated = Vec::with_capacity(encode_capacity_hint(&decoded_old));
        encode_state(&decoded_old, &mut buf_migrated).expect("re-encode must succeed");
        let redecoded = decode_state(&buf_migrated)
            .unwrap_or_else(|_| panic!("schema_ver {schema_ver}: migrated buffer must decode"));
        assert_eq!(
            hash_state_blake3(&redecoded),
            hash_state_blake3(&decoded_old),
            "schema_ver {schema_ver}: migrating to the current format must be lossless"
        );

        // Re-encoding a second time must be a no-op fixed point — the hash
        // must never drift on repeated encode/decode once migrated.
        let mut buf_migrated_2 = Vec::with_capacity(encode_capacity_hint(&redecoded));
        encode_state(&redecoded, &mut buf_migrated_2).expect("second re-encode must succeed");
        let redecoded_2 = decode_state(&buf_migrated_2)
            .unwrap_or_else(|_| panic!("schema_ver {schema_ver}: twice-migrated buffer must decode"));
        assert_eq!(
            hash_state_blake3(&redecoded_2),
            hash_state_blake3(&decoded_old),
            "schema_ver {schema_ver}: repeated re-encoding must be a stable fixed point"
        );
    }
}

// ── Decoder hardening: V6 namespace head arrays are untrusted input ────────

#[test]
fn v6_out_of_range_namespace_head_is_rejected() {
    let mut buf = encode_legacy_scenario_a(6);
    // schema_ver 6 has no meta section (V7-only), so the namespace head
    // arrays are exactly the trailing 2 * MAX_NAMESPACES * 4 bytes.
    let heads_start = buf.len() - 2 * MAX_NAMESPACES * 4;
    let bad_head: u32 = 999_999; // far past total_slots (2)
    buf[heads_start..heads_start + 4].copy_from_slice(&bad_head.to_le_bytes());
    assert!(
        decode_state(&buf).is_err(),
        "an out-of-range namespace record head must be rejected, not trusted"
    );
}

#[test]
fn schema_version_zero_is_rejected() {
    let buf = encode_legacy(0, 0, DIM, &scenario_a_records(), &[], &[]);
    assert!(decode_state(&buf).is_err(), "schema_ver 0 is out of the valid 1..=7 range");
}
