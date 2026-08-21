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
use valori_kernel::snapshot::encode::{encode_capacity_hint, encode_state};
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
        vector: FxpVector {
            data: vec![
                FxpScalar(1024),
                FxpScalar(-512),
                FxpScalar(256),
                FxpScalar(0),
            ],
        },
        metadata: Some(b"single-record-metadata".to_vec()),
        tag: 42,
    })
    .unwrap();
    s.apply_event(&KernelEvent::CreateNode {
        id: NodeId(0),
        kind: NodeKind::Document,
        record: Some(RecordId(0)),
    })
    .unwrap();
    s
}

fn state_multi() -> KernelState {
    let mut s = KernelState::new();

    // 16 records across two namespaces (0 and 1)
    for i in 0u32..8 {
        let data = (0..4)
            .map(|d| FxpScalar((i * 1000 + d * 7) as i32))
            .collect();
        s.apply_event_ns(
            &KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector { data },
                metadata: if i % 3 == 0 {
                    Some(format!("{{\"idx\":{i}}}").into_bytes())
                } else {
                    None
                },
                tag: i as u64 % 5,
            },
            0,
        )
        .unwrap();
    }
    for i in 8u32..16 {
        let data = (0..4)
            .map(|d| FxpScalar((i * 500 + d * 13) as i32 - 1000))
            .collect();
        s.apply_event_ns(
            &KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector { data },
                metadata: None,
                tag: (i % 3) as u64,
            },
            1,
        )
        .unwrap();
    }

    // Graph
    for i in 0u32..4 {
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(i),
            kind: NodeKind::Concept,
            record: Some(RecordId(i)),
        })
        .unwrap();
    }
    for i in 0u32..3 {
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(i),
            kind: EdgeKind::Relation,
            from: NodeId(i),
            to: NodeId(i + 1),
        })
        .unwrap();
    }

    // Meta (V7)
    s.apply_event(&KernelEvent::SetMeta {
        key: "corpus:version".into(),
        value: "fixture-v1".into(),
    })
    .unwrap();
    s.apply_event(&KernelEvent::SetMeta {
        key: "corpus:dim".into(),
        value: "4".into(),
    })
    .unwrap();

    s
}

/// V8 fixture: same as `state_multi`, plus two explicitly-configured
/// collections (namespaces 2 and 3) — proves `namespace_configs` round-trips
/// through encode/decode and that a V7 decoder path (no such section) still
/// produces an equivalent state for namespaces that were never configured.
fn state_multi_collections() -> KernelState {
    use valori_kernel::index::Metric;

    let mut s = state_multi();
    s.configure_namespace(2, 384, Metric::SquaredL2, 0 /* BruteForce */)
        .unwrap();
    s.configure_namespace(3, 1536, Metric::SquaredL2, 2 /* Hnsw */)
        .unwrap();
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
        // G0.2: bumped for STATE_HASH_DOMAIN_VERSION 2 -> 3; see
        // format.rs::empty_state_hash_is_pinned's doc comment.
        "feb47a4c03ee329d108f168945e204413ec8068f44d85503e4ec5bab6412d9a2",
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
    let bytes =
        std::fs::read(fixture_path("snapshot_v7_multi.bin")).expect("committed fixture must exist");
    let mut restored = decode_state(&bytes).expect("fixture must decode forever");

    restored
        .apply_event(&KernelEvent::InsertRecord {
            id: RecordId(16),
            vector: FxpVector {
                data: vec![FxpScalar(100); 4],
            },
            metadata: None,
            tag: 0,
        })
        .unwrap();

    let mut from_scratch = state_multi();
    from_scratch
        .apply_event(&KernelEvent::InsertRecord {
            id: RecordId(16),
            vector: FxpVector {
                data: vec![FxpScalar(100); 4],
            },
            metadata: None,
            tag: 0,
        })
        .unwrap();

    assert_eq!(
        hash_state_blake3(&restored),
        hash_state_blake3(&from_scratch),
        "restored snapshot must continue producing the same state as replay-from-scratch"
    );
}

/// V8: a snapshot with explicit per-collection config round-trips both the
/// vector data AND the namespace_configs map.
#[test]
fn snapshot_v8_multi_collections_decodes_forever() {
    let bytes = std::fs::read(fixture_path("snapshot_v8_multi_collections.bin"))
        .expect("committed snapshot_v8_multi_collections.bin must exist");
    let expected = std::fs::read_to_string(fixture_path("snapshot_v8_multi_collections.hash"))
        .expect("snapshot_v8_multi_collections.hash must exist");
    let state = decode_state(&bytes).expect("fixture must decode forever");
    assert_eq!(
        hex(&hash_state_blake3(&state)),
        expected.trim(),
        "v8 multi-collection snapshot hash changed — snapshot format or hash domain broke compatibility"
    );
    assert_eq!(state.record_count(), 16);
    assert_eq!(state.namespace_dim(2), Some(384));
    assert_eq!(state.namespace_dim(3), Some(1536));
    // Namespace 0 was never explicitly configured — falls back to the legacy
    // process-wide dim, exactly like a pre-existing collection would.
    assert_eq!(state.namespace_dim(0), state.dim);
    assert!(!state.has_namespace_config(0));
    assert!(state.has_namespace_config(2));
}

/// A V8 snapshot with zero explicit collections decodes to a state
/// byte-behaviorally identical to a V7 snapshot of the same data — proves
/// the "old projects behave exactly as before" migration requirement holds
/// without any separate migration code.
#[test]
fn snapshot_v8_with_no_explicit_collections_matches_v7_behavior() {
    let v7_bytes = std::fs::read(fixture_path("snapshot_v7_multi.bin")).unwrap();
    let v7_state = decode_state(&v7_bytes).unwrap();

    let v8_state = state_multi(); // no configure_namespace calls
    let v8_bytes = encode(&v8_state);
    let v8_restored = decode_state(&v8_bytes).unwrap();

    assert_eq!(
        hash_state_blake3(&v7_state),
        hash_state_blake3(&v8_restored)
    );
    assert!(v8_restored.namespace_configs.is_empty());
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

    write_fixture("snapshot_v7_empty.bin", &state_empty());
    write_fixture("snapshot_v7_single.bin", &state_single());
    write_fixture("snapshot_v7_multi.bin", &state_multi());
    write_fixture(
        "snapshot_v8_multi_collections.bin",
        &state_multi_collections(),
    );
}
