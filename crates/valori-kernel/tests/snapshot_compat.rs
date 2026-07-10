// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Snapshot format compatibility corpus.
//!
//! The `.bin` files under `tests/fixtures/` are COMMITTED BYTES written by
//! a specific encoder version. Every test here must pass forever, unchanged.
//! If a test fails it means the snapshot format, state-hash domain, or the
//! encoder changed in a way that breaks backward compatibility — that is a
//! breaking change and must be treated as such, not silently fixed by
//! regenerating the fixture.
//!
//! `generate_snapshot_fixtures` (ignored) writes the files. Run it once
//! whenever a new schema version is introduced, then commit the new bins.

use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::{encode_state, encode_capacity_hint};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::{EdgeId, NodeId, RecordId};
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn encode(state: &KernelState) -> Vec<u8> {
    let mut buf = Vec::with_capacity(encode_capacity_hint(state));
    encode_state(state, &mut buf).expect("encode_state failed");
    buf
}

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ── Fixture state builders ────────────────────────────────────────────────────

fn state_empty() -> KernelState {
    KernelState::new()
}

fn state_single() -> KernelState {
    let mut s = KernelState::new();
    s.apply_event(&KernelEvent::InsertRecord {
        id: RecordId(0),
        vector: FxpVector { data: vec![FxpScalar(1024), FxpScalar(-512), FxpScalar(256), FxpScalar(0)] },
        metadata: Some(b"single-record-metadata".to_vec()),
        tag: 42,
    }).unwrap();
    s.apply_event(&KernelEvent::CreateNode {
        id: NodeId(0),
        kind: NodeKind::Document,
        record: Some(RecordId(0)),
    }).unwrap();
    s
}

fn state_multi() -> KernelState {
    let mut s = KernelState::new();

    // 16 records across two namespaces (0 and 1)
    for i in 0u32..8 {
        let data = (0..4).map(|d| FxpScalar((i * 1000 + d * 7) as i32)).collect();
        s.apply_event_ns(&KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector { data },
            metadata: if i % 3 == 0 { Some(format!("{{\"idx\":{i}}}").into_bytes()) } else { None },
            tag: i as u64 % 5,
        }, 0).unwrap();
    }
    for i in 8u32..16 {
        let data = (0..4).map(|d| FxpScalar((i * 500 + d * 13) as i32 - 1000)).collect();
        s.apply_event_ns(&KernelEvent::InsertRecord {
            id: RecordId(i),
            vector: FxpVector { data },
            metadata: None,
            tag: (i % 3) as u64,
        }, 1).unwrap();
    }

    // Graph
    for i in 0u32..4 {
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(i),
            kind: NodeKind::Concept,
            record: Some(RecordId(i)),
        }).unwrap();
    }
    for i in 0u32..3 {
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(i),
            kind: EdgeKind::Relation,
            from: NodeId(i),
            to: NodeId(i + 1),
        }).unwrap();
    }

    // Meta (V7)
    s.apply_event(&KernelEvent::SetMeta {
        key: "corpus:version".into(),
        value: "fixture-v1".into(),
    }).unwrap();
    s.apply_event(&KernelEvent::SetMeta {
        key: "corpus:dim".into(),
        value: "4".into(),
    }).unwrap();

    s
}

// ── Forever-decode tests ──────────────────────────────────────────────────────

/// Empty state hash is also pinned in `format.rs::empty_state_hash_is_pinned` —
/// the snapshot fixture test is the binary complement: same state, but now
/// the binary encoding itself is also locked.
#[test]
fn snapshot_v7_empty_decodes_forever() {
    let bytes = std::fs::read(fixture_path("snapshot_v7_empty.bin"))
        .expect("committed snapshot_v7_empty.bin must exist");
    let state = decode_state(&bytes).expect("fixture must decode forever");
    assert_eq!(
        hex(&hash_state_blake3(&state)),
        "4eeaa41d0b2eb66651bdbb252f4b91a7fa191d3f1cee4d311b6056966fba4d4a",
        "empty-state hash changed — snapshot format or hash domain broke compatibility"
    );
    assert_eq!(state.record_count(), 0);
}

#[test]
fn snapshot_v7_single_decodes_forever() {
    let bytes = std::fs::read(fixture_path("snapshot_v7_single.bin"))
        .expect("committed snapshot_v7_single.bin must exist");
    let expected = std::fs::read_to_string(fixture_path("snapshot_v7_single.hash"))
        .expect("snapshot_v7_single.hash must exist");
    let state = decode_state(&bytes).expect("fixture must decode forever");
    assert_eq!(
        hex(&hash_state_blake3(&state)),
        expected.trim(),
        "single-record snapshot hash changed — snapshot format or hash domain broke compatibility"
    );
    assert_eq!(state.record_count(), 1);
    assert_eq!(state.node_count(), 1);
}

#[test]
fn snapshot_v7_multi_decodes_forever() {
    let bytes = std::fs::read(fixture_path("snapshot_v7_multi.bin"))
        .expect("committed snapshot_v7_multi.bin must exist");
    let expected = std::fs::read_to_string(fixture_path("snapshot_v7_multi.hash"))
        .expect("snapshot_v7_multi.hash must exist");
    let state = decode_state(&bytes).expect("fixture must decode forever");
    assert_eq!(
        hex(&hash_state_blake3(&state)),
        expected.trim(),
        "multi-record snapshot hash changed — snapshot format or hash domain broke compatibility"
    );
    assert_eq!(state.record_count(), 16);
    assert_eq!(state.node_count(), 4);
    assert_eq!(state.edge_count(), 3);
}

/// V7 snapshot restores to a state where further events can be applied and
/// produce the same hash as if the full event sequence had been replayed.
#[test]
fn snapshot_v7_multi_can_continue_after_restore() {
    let bytes = std::fs::read(fixture_path("snapshot_v7_multi.bin"))
        .expect("committed fixture must exist");
    let mut restored = decode_state(&bytes).expect("fixture must decode forever");

    restored.apply_event(&KernelEvent::InsertRecord {
        id: RecordId(16),
        vector: FxpVector { data: vec![FxpScalar(100); 4] },
        metadata: None,
        tag: 0,
    }).unwrap();

    let mut from_scratch = state_multi();
    from_scratch.apply_event(&KernelEvent::InsertRecord {
        id: RecordId(16),
        vector: FxpVector { data: vec![FxpScalar(100); 4] },
        metadata: None,
        tag: 0,
    }).unwrap();

    assert_eq!(
        hash_state_blake3(&restored),
        hash_state_blake3(&from_scratch),
        "restored snapshot must continue producing the same state as replay-from-scratch"
    );
}

// ── Fixture generator (run once per schema version bump, then commit) ─────────

/// `cargo test -p valori-kernel --test snapshot_compat generate_snapshot_fixtures -- --ignored --nocapture`
#[test]
#[ignore]
fn generate_snapshot_fixtures() {
    use std::fs;

    let dir = fixture_path("");
    fs::create_dir_all(&dir).unwrap();

    let write_fixture = |name: &str, state: &KernelState| {
        let bytes = encode(state);
        let hash = hex(&hash_state_blake3(state));
        fs::write(dir.join(name), &bytes).unwrap();
        fs::write(dir.join(name.replace(".bin", ".hash")), &hash).unwrap();
        println!("{name}: {} bytes, hash {hash}", bytes.len());
    };

    write_fixture("snapshot_v7_empty.bin",  &state_empty());
    write_fixture("snapshot_v7_single.bin", &state_single());
    write_fixture("snapshot_v7_multi.bin",  &state_multi());
}
