// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G0.1 — Graph Determinism & State Integrity.
//!
//! Executable proofs for the invariants established by
//! `docs/reviews/graph-g0-architecture-audit.md` and
//! `docs/reviews/graph-g0.1-determinism-state-integrity.md`. Each test here
//! maps to a specific G0.1 phase; see the doc comment on each test for the
//! exact claim it proves and the phase it satisfies.
//!
//! These tests compare CANONICAL FIELDS directly (node id/kind/record/
//! namespace_id/adjacency, edge id/kind/from/to/adjacency) rather than only
//! aggregate counts or hashes — per G0.1 Phase 3/4's explicit requirement
//! that count/hash equality alone is insufficient.

use valori_kernel::event::KernelEvent;
use valori_kernel::snapshot::blake3::hash_state_blake3;
use valori_kernel::snapshot::decode::decode_state;
use valori_kernel::snapshot::encode::{encode_capacity_hint, encode_state};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::{EdgeId, NodeId, RecordId};
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::vector::FxpVector;

const DIM: usize = 4;

fn vec_from_seed(seed: u32) -> FxpVector {
    FxpVector {
        data: (0..DIM)
            .map(|d| FxpScalar(((seed.wrapping_mul(2654435761) >> (d * 3)) % 20000) as i32 - 10000))
            .collect(),
    }
}

/// A graph-and-namespace-inclusive event sequence exercising every real
/// graph `KernelEvent` variant found in the codebase (G0.1 Phase 3
/// instruction: "do not invent event types; use actual graph events"):
/// node creation, multiple edges, a self-loop, node deletion (with cascade),
/// edge deletion, and namespace placement via `apply_event_ns`. Returns
/// `(namespace_id, event)` pairs, matching the production replay wire shape
/// (`LogEntry::EventNs`).
fn graph_scenario_events() -> Vec<(u16, KernelEvent)> {
    vec![
        // Namespace 0: records + a small graph (A -> B, B -> C, A -> A self-loop).
        (
            0,
            KernelEvent::InsertRecord {
                id: RecordId(0),
                vector: vec_from_seed(1),
                metadata: Some(b"doc-a".to_vec()),
                tag: 7,
            },
        ),
        (
            0,
            KernelEvent::InsertRecord {
                id: RecordId(1),
                vector: vec_from_seed(2),
                metadata: None,
                tag: 0,
            },
        ),
        (
            0,
            KernelEvent::CreateNode {
                id: NodeId(0),
                kind: NodeKind::Document,
                record: Some(RecordId(0)),
            },
        ),
        (
            0,
            KernelEvent::CreateNode {
                id: NodeId(1),
                kind: NodeKind::Chunk,
                record: Some(RecordId(1)),
            },
        ),
        (
            0,
            KernelEvent::CreateNode {
                id: NodeId(2),
                kind: NodeKind::Concept,
                record: None,
            },
        ),
        (
            0,
            KernelEvent::CreateEdge {
                id: EdgeId(0),
                from: NodeId(0),
                to: NodeId(1),
                kind: EdgeKind::ParentOf,
            },
        ),
        (
            0,
            KernelEvent::CreateEdge {
                id: EdgeId(1),
                from: NodeId(1),
                to: NodeId(2),
                kind: EdgeKind::RefersTo,
            },
        ),
        (
            0,
            KernelEvent::CreateEdge {
                id: EdgeId(2), // self-loop
                from: NodeId(0),
                to: NodeId(0),
                kind: EdgeKind::Relation,
            },
        ),
        // A duplicate edge: same (from, to, kind) as edge 0. Distinct id.
        (
            0,
            KernelEvent::CreateEdge {
                id: EdgeId(3),
                from: NodeId(0),
                to: NodeId(1),
                kind: EdgeKind::ParentOf,
            },
        ),
        // Namespace 1: an unrelated node, structurally identical to node 0
        // except for namespace — exercises namespace placement.
        (
            1,
            KernelEvent::CreateNode {
                id: NodeId(3),
                kind: NodeKind::Document,
                record: None,
            },
        ),
        // Delete edge 1 (B -> C) explicitly.
        (0, KernelEvent::DeleteEdge { id: EdgeId(1) }),
        // Delete node 2 (C) — nothing incident remains, but exercises
        // DeleteNode on a leaf.
        (0, KernelEvent::DeleteNode { id: NodeId(2) }),
    ]
}

fn apply_all(events: &[(u16, KernelEvent)]) -> KernelState {
    let mut state = KernelState::new();
    for (ns, evt) in events {
        state
            .apply_event_ns(evt, *ns)
            .unwrap_or_else(|e| panic!("event application failed: {evt:?} ({e:?})"));
    }
    state
}

/// Extracts every canonical, publicly-readable graph field for one node, in
/// a form comparable with `assert_eq!`. Mirrors exactly what G0.1 Phase 3
/// asks to be compared: id, kind, record linkage, namespace, and (via a
/// separate adjacency walk) both adjacency directions.
fn node_fingerprint(state: &KernelState, id: NodeId) -> Option<(u32, u8, Option<u32>, u16)> {
    state
        .get_node(id)
        .map(|n| (n.id.0, n.kind as u8, n.record.map(|r| r.0), n.namespace_id))
}

fn edge_fingerprint(state: &KernelState, id: EdgeId) -> Option<(u32, u8, u32, u32)> {
    state
        .get_edge(id)
        .map(|e| (e.id.0, e.kind as u8, e.from.0, e.to.0))
}

fn outgoing_ids(state: &KernelState, id: NodeId) -> Vec<u32> {
    state
        .outgoing_edges(id)
        .map(|it| it.map(|e| e.id.0).collect())
        .unwrap_or_default()
}

fn incoming_ids(state: &KernelState, id: NodeId) -> Vec<u32> {
    state
        .incoming_edges(id)
        .map(|it| it.map(|e| e.id.0).collect())
        .unwrap_or_default()
}

/// Compares two `KernelState`s field-by-field over every canonical graph
/// property enumerated in G0.1 Phase 3: node ids, kind, record linkage,
/// namespace, edge ids, source, destination, kind, and BOTH adjacency
/// directions (outgoing + incoming) for every live node. Also compares
/// record ids/namespace to catch a namespace-only divergence that graph
/// fields alone might miss.
fn assert_graph_states_equivalent(a: &KernelState, b: &KernelState, label: &str) {
    assert_eq!(
        a.node_count(),
        b.node_count(),
        "{label}: node_count differs"
    );
    assert_eq!(
        a.edge_count(),
        b.edge_count(),
        "{label}: edge_count differs"
    );
    assert_eq!(
        a.record_count(),
        b.record_count(),
        "{label}: record_count differs"
    );

    let max_node = a.next_node_id().0.max(b.next_node_id().0);
    for i in 0..max_node {
        let id = NodeId(i);
        assert_eq!(
            node_fingerprint(a, id),
            node_fingerprint(b, id),
            "{label}: node {i} fingerprint (id,kind,record,namespace) differs"
        );
        assert_eq!(
            outgoing_ids(a, id),
            outgoing_ids(b, id),
            "{label}: node {i} outgoing adjacency differs"
        );
        assert_eq!(
            incoming_ids(a, id),
            incoming_ids(b, id),
            "{label}: node {i} incoming adjacency differs"
        );
    }

    let max_edge = a.next_edge_id().0.max(b.next_edge_id().0);
    for i in 0..max_edge {
        let id = EdgeId(i);
        assert_eq!(
            edge_fingerprint(a, id),
            edge_fingerprint(b, id),
            "{label}: edge {i} fingerprint (id,kind,from,to) differs"
        );
    }

    for ns in [0u16, 1u16] {
        let a_ns: Vec<u32> = a.iter_records_in_ns(ns).map(|r| r.id.0).collect();
        let b_ns: Vec<u32> = b.iter_records_in_ns(ns).map(|r| r.id.0).collect();
        assert_eq!(
            a_ns, b_ns,
            "{label}: namespace {ns} record membership differs"
        );
    }
}

// ── Phase 3: graph replay equivalence ─────────────────────────────────────
//
// S0 = fresh state; S1 = apply_all(events) once; S2 = apply_all(events)
// again into a second fresh state ("replay" — at the kernel layer, replay
// IS re-invoking apply_event_ns in event order; this is exactly what the
// real production path (`valori_storage::events::event_replay::replay_events`)
// does). Field-by-field, not just count/hash, per Phase 3's explicit
// requirement.

#[test]
fn graph_replay_produces_field_identical_state() {
    let events = graph_scenario_events();

    let s1 = apply_all(&events);
    let s2 = apply_all(&events); // independent "replay" of the same event sequence

    assert_graph_states_equivalent(&s1, &s2, "replay vs original apply");
    assert_eq!(
        hash_state_blake3(&s1),
        hash_state_blake3(&s2),
        "replay vs original apply: state hash differs"
    );
}

#[test]
fn graph_replay_is_stable_across_three_independent_applications() {
    // Strengthens the pairwise check above to three independent builds, to
    // rule out a two-run coincidence.
    let events = graph_scenario_events();
    let s1 = apply_all(&events);
    let s2 = apply_all(&events);
    let s3 = apply_all(&events);
    assert_graph_states_equivalent(&s1, &s2, "run 1 vs run 2");
    assert_graph_states_equivalent(&s2, &s3, "run 2 vs run 3");
}

// ── Phase 4: graph snapshot equivalence ───────────────────────────────────
//
// S1 -> snapshot -> restore -> S3. Field-by-field, including adjacency in
// both directions and namespace membership, not just hash/counts (the
// existing `roundtrip_preserves_state_hash` test in snapshot_roundtrip.rs
// already covers hash+count equality; this test is additive, proving the
// stronger field-level claim Phase 4 asks for).

#[test]
fn graph_snapshot_restore_produces_field_identical_state() {
    let events = graph_scenario_events();
    let s1 = apply_all(&events);

    let mut buf = Vec::with_capacity(encode_capacity_hint(&s1));
    encode_state(&s1, &mut buf).expect("encode");
    let s3 = decode_state(&buf).expect("decode");

    assert_graph_states_equivalent(&s1, &s3, "snapshot restore vs original");
    assert_eq!(
        hash_state_blake3(&s1),
        hash_state_blake3(&s3),
        "snapshot restore: state hash differs"
    );
}

// ── Phase 5: namespace invariants ─────────────────────────────────────────
//
// Verified at the KERNEL level (not HTTP/API) per Phase 5's explicit
// instruction. Cross-namespace edge attempts during LIVE apply.
// (Replay-cannot-bypass is proven separately, end-to-end through the real
// disk-backed replay path, by
// `valori_storage::events::event_replay::tests::
// graph_events_recover_into_their_own_namespace_and_reject_cross_ns_edges_on_replay`.)

#[test]
fn cross_namespace_edge_is_rejected_at_the_kernel_apply_layer() {
    let mut state = KernelState::new();
    state
        .apply_event_ns(
            &KernelEvent::CreateNode {
                id: NodeId(0),
                kind: NodeKind::Concept,
                record: None,
            },
            0,
        )
        .unwrap();
    state
        .apply_event_ns(
            &KernelEvent::CreateNode {
                id: NodeId(1),
                kind: NodeKind::Concept,
                record: None,
            },
            1,
        )
        .unwrap();

    let cross_ns_edge = KernelEvent::CreateEdge {
        id: EdgeId(0),
        from: NodeId(0), // namespace 0
        to: NodeId(1),   // namespace 1
        kind: EdgeKind::Relation,
    };
    let result = state.apply_event_ns(&cross_ns_edge, 0);
    assert!(
        result.is_err(),
        "an edge whose endpoints live in different namespaces must be rejected"
    );
    assert_eq!(
        state.edge_count(),
        0,
        "the rejected edge must not be created"
    );
}

#[test]
fn same_namespace_edge_is_accepted() {
    // Negative-space control for the test above: same setup, but both nodes
    // in namespace 0 — must succeed, proving the rejection above is really
    // about the namespace mismatch and not some other validation failure.
    let mut state = KernelState::new();
    state
        .apply_event_ns(
            &KernelEvent::CreateNode {
                id: NodeId(0),
                kind: NodeKind::Concept,
                record: None,
            },
            0,
        )
        .unwrap();
    state
        .apply_event_ns(
            &KernelEvent::CreateNode {
                id: NodeId(1),
                kind: NodeKind::Concept,
                record: None,
            },
            0,
        )
        .unwrap();
    let same_ns_edge = KernelEvent::CreateEdge {
        id: EdgeId(0),
        from: NodeId(0),
        to: NodeId(1),
        kind: EdgeKind::Relation,
    };
    assert!(state.apply_event_ns(&same_ns_edge, 0).is_ok());
    assert_eq!(state.edge_count(), 1);
}

#[test]
fn node_record_linkage_must_match_namespace() {
    // The other half of the G0 namespace invariant: a node cannot claim a
    // record from a different namespace either (kernel.rs's CreateNode arm).
    let mut state = KernelState::new();
    state
        .apply_event_ns(
            &KernelEvent::InsertRecord {
                id: RecordId(0),
                vector: FxpVector::new_zeros(DIM),
                metadata: None,
                tag: 0,
            },
            1, // record lives in namespace 1
        )
        .unwrap();
    let bad_node = KernelEvent::CreateNode {
        id: NodeId(0),
        kind: NodeKind::Document,
        record: Some(RecordId(0)),
    };
    // Node created in namespace 0, but its record is in namespace 1.
    assert!(
        state.apply_event_ns(&bad_node, 0).is_err(),
        "a node must not reference a record from a different namespace"
    );
    assert_eq!(state.node_count(), 0);
}

// ── Phase 6: duplicate edge semantics ─────────────────────────────────────
//
// Documents the actual (not assumed) contract: `add_edge` performs no
// dedup check against (from, to, kind). Creating the same tuple twice
// produces two distinct, independently-tracked EdgeIds, both retained in
// both adjacency lists. Duplicate insertion is NOT rejected and is NOT
// idempotent (it always creates a new edge, never returns an existing one).

#[test]
fn duplicate_edges_are_allowed_and_independently_tracked() {
    let mut state = KernelState::new();
    for i in 0..2 {
        state
            .apply_event(&KernelEvent::CreateNode {
                id: NodeId(i),
                kind: NodeKind::Concept,
                record: None,
            })
            .unwrap();
    }

    // Create the SAME (from, to, kind) tuple three times.
    for i in 0..3u32 {
        let evt = KernelEvent::CreateEdge {
            id: EdgeId(i),
            from: NodeId(0),
            to: NodeId(1),
            kind: EdgeKind::Relation,
        };
        assert!(
            state.apply_event(&evt).is_ok(),
            "duplicate edge creation #{i} must be accepted, not rejected"
        );
    }

    assert_eq!(
        state.edge_count(),
        3,
        "duplicate edge semantics: insertion is NOT idempotent — 3 calls \
         create 3 distinct edges, not 1"
    );
    let out = outgoing_ids(&state, NodeId(0));
    let inc = incoming_ids(&state, NodeId(1));
    assert_eq!(
        out.len(),
        3,
        "all 3 duplicate edges must appear in outgoing adjacency"
    );
    assert_eq!(
        inc.len(),
        3,
        "all 3 duplicate edges must appear in incoming adjacency"
    );
    // Distinct ids — duplicates are NOT deduplicated to a shared EdgeId.
    let mut ids = out.clone();
    ids.sort_unstable();
    assert_eq!(ids, vec![0, 1, 2]);
}

// ── Phase 7: self-loop semantics ──────────────────────────────────────────
//
// `graph_cascade.rs::test_delete_node_with_self_loop` (valori-node) already
// covers self-loop DELETION. That test does not exist at the kernel-crate
// level and does not cover CREATION + both-direction adjacency + snapshot +
// replay in one place, which is what Phase 7 asks for — added here rather
// than duplicated there.

#[test]
fn self_loop_creation_appears_in_both_adjacency_directions() {
    let mut state = KernelState::new();
    state
        .apply_event(&KernelEvent::CreateNode {
            id: NodeId(0),
            kind: NodeKind::Concept,
            record: None,
        })
        .unwrap();
    state
        .apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(0),
            from: NodeId(0),
            to: NodeId(0),
            kind: EdgeKind::Relation,
        })
        .unwrap();

    assert_eq!(outgoing_ids(&state, NodeId(0)), vec![0]);
    assert_eq!(incoming_ids(&state, NodeId(0)), vec![0]);

    // Survives snapshot round-trip.
    let mut buf = Vec::with_capacity(encode_capacity_hint(&state));
    encode_state(&state, &mut buf).unwrap();
    let restored = decode_state(&buf).unwrap();
    assert_eq!(outgoing_ids(&restored, NodeId(0)), vec![0]);
    assert_eq!(incoming_ids(&restored, NodeId(0)), vec![0]);

    // Survives replay (independent re-apply of the same events).
    let events = vec![
        (
            0u16,
            KernelEvent::CreateNode {
                id: NodeId(0),
                kind: NodeKind::Concept,
                record: None,
            },
        ),
        (
            0u16,
            KernelEvent::CreateEdge {
                id: EdgeId(0),
                from: NodeId(0),
                to: NodeId(0),
                kind: EdgeKind::Relation,
            },
        ),
    ];
    let replayed = apply_all(&events);
    assert_eq!(outgoing_ids(&replayed, NodeId(0)), vec![0]);
    assert_eq!(incoming_ids(&replayed, NodeId(0)), vec![0]);
}

// ── Phase 10: CRITICAL HASH PROPERTY — fields the current contract commits ──
//
// The hash contract (see G0.1 doc §4) commits, per edge: id, kind, from,
// to. Direction and target are therefore load-bearing: swapping them MUST
// change the hash. This is the only class of "hash divergence" property
// testable through the public API alone (see G0.1 doc for why the
// namespace_id / first_in_edge / next_in gaps require a different proof
// technique).

#[test]
fn hash_contract_direction_is_committed() {
    // A -> B
    let mut a = KernelState::new();
    a.apply_event(&KernelEvent::CreateNode {
        id: NodeId(0),
        kind: NodeKind::Concept,
        record: None,
    })
    .unwrap();
    a.apply_event(&KernelEvent::CreateNode {
        id: NodeId(1),
        kind: NodeKind::Concept,
        record: None,
    })
    .unwrap();
    a.apply_event(&KernelEvent::CreateEdge {
        id: EdgeId(0),
        from: NodeId(0),
        to: NodeId(1),
        kind: EdgeKind::Relation,
    })
    .unwrap();

    // B -> A (reversed direction, otherwise identical construction).
    let mut b = KernelState::new();
    b.apply_event(&KernelEvent::CreateNode {
        id: NodeId(0),
        kind: NodeKind::Concept,
        record: None,
    })
    .unwrap();
    b.apply_event(&KernelEvent::CreateNode {
        id: NodeId(1),
        kind: NodeKind::Concept,
        record: None,
    })
    .unwrap();
    b.apply_event(&KernelEvent::CreateEdge {
        id: EdgeId(0),
        from: NodeId(1),
        to: NodeId(0),
        kind: EdgeKind::Relation,
    })
    .unwrap();

    assert_ne!(
        hash_state_blake3(&a),
        hash_state_blake3(&b),
        "edge direction is a committed hash field — A->B must not hash the same as B->A"
    );
}

#[test]
fn hash_contract_target_is_committed() {
    // A -> B
    let mut a = KernelState::new();
    for i in 0..3 {
        a.apply_event(&KernelEvent::CreateNode {
            id: NodeId(i),
            kind: NodeKind::Concept,
            record: None,
        })
        .unwrap();
    }
    a.apply_event(&KernelEvent::CreateEdge {
        id: EdgeId(0),
        from: NodeId(0),
        to: NodeId(1),
        kind: EdgeKind::Relation,
    })
    .unwrap();

    // A -> C (different target, otherwise identical construction).
    let mut b = KernelState::new();
    for i in 0..3 {
        b.apply_event(&KernelEvent::CreateNode {
            id: NodeId(i),
            kind: NodeKind::Concept,
            record: None,
        })
        .unwrap();
    }
    b.apply_event(&KernelEvent::CreateEdge {
        id: EdgeId(0),
        from: NodeId(0),
        to: NodeId(2),
        kind: EdgeKind::Relation,
    })
    .unwrap();

    assert_ne!(
        hash_state_blake3(&a),
        hash_state_blake3(&b),
        "edge target is a committed hash field — A->B must not hash the same as A->C"
    );
}

/// G0.2 SUPERSEDES this test's original G0.1 form. Originally this locked
/// down the *gap* (namespace_id excluded from the hash). G0.2 closed that
/// gap (`STATE_HASH_DOMAIN_VERSION` 2 -> 3) after auditing the consensus-wide
/// blast radius and finding it was small and mechanically fixable — see
/// `docs/reviews/graph-g0.2-canonical-state-hash-commitment.md`. This test
/// now proves the corrected contract: two states whose ONLY difference is
/// which namespace a structurally-identical node lives in now hash
/// DIFFERENTLY.
#[test]
fn hash_contract_now_covers_node_namespace_id() {
    let mut a = KernelState::new();
    a.apply_event_ns(
        &KernelEvent::CreateNode {
            id: NodeId(0),
            kind: NodeKind::Concept,
            record: None,
        },
        0,
    )
    .unwrap();

    let mut b = KernelState::new();
    b.apply_event_ns(
        &KernelEvent::CreateNode {
            id: NodeId(0),
            kind: NodeKind::Concept,
            record: None,
        },
        5,
    )
    .unwrap();

    assert_eq!(
        a.get_node(NodeId(0)).unwrap().namespace_id,
        0,
        "sanity: state a's node really is in namespace 0"
    );
    assert_eq!(
        b.get_node(NodeId(0)).unwrap().namespace_id,
        5,
        "sanity: state b's node really is in namespace 5"
    );
    assert_ne!(
        hash_state_blake3(&a),
        hash_state_blake3(&b),
        "G0.2: node namespace_id is now a committed hash field — a \
         namespace-misrouting bug is no longer invisible to hash comparison"
    );
}

/// G0.2 companion to the record-level namespace case above.
#[test]
fn hash_contract_now_covers_record_namespace_id() {
    let mut a = KernelState::new();
    a.apply_event_ns(
        &KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::new_zeros(DIM),
            metadata: None,
            tag: 0,
        },
        0,
    )
    .unwrap();

    let mut b = KernelState::new();
    b.apply_event_ns(
        &KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: FxpVector::new_zeros(DIM),
            metadata: None,
            tag: 0,
        },
        5,
    )
    .unwrap();

    assert_ne!(
        hash_state_blake3(&a),
        hash_state_blake3(&b),
        "G0.2: record namespace_id is now a committed hash field"
    );
}

/// G0.2: the `SetMeta` sidecar is now committed too — two states differing
/// only in a meta key/value pair must hash differently.
#[test]
fn hash_contract_now_covers_meta_sidecar() {
    let mut a = KernelState::new();
    a.apply_event(&KernelEvent::SetMeta {
        key: "corpus:version".into(),
        value: "1.0.0".into(),
    })
    .unwrap();

    let mut b = KernelState::new();
    b.apply_event(&KernelEvent::SetMeta {
        key: "corpus:version".into(),
        value: "2.0.0".into(),
    })
    .unwrap();

    assert_ne!(
        hash_state_blake3(&a),
        hash_state_blake3(&b),
        "G0.2: KernelState.meta is now a committed hash field"
    );
}

/// G0.2's deliberate, narrower exclusion (documented in
/// `hash_state_blake3`'s own doc comment and in the G0.2 report): unlike
/// `namespace_id`, the namespace intrusive-list pointers (`next_in_ns`/
/// `prev_in_ns`) are NOT hashed. `KernelState::rebuild_namespace_lists`
/// (`pub fn`, used to migrate pre-V6 snapshots) rebuilds these pointers in
/// the OPPOSITE order from live `apply_event_ns` construction — both are
/// valid linked lists over the same namespace membership, so hashing the
/// pointers would make hash equality depend on which of two correct
/// algorithms built the state, not on real content divergence (this is
/// exactly what broke `snapshot_version_migration.rs::
/// cross_version_decode_reencode_chain_is_hash_stable` during G0.2's
/// implementation — see the G0.2 report §6).
///
/// This test reproduces the divergence directly: build a state, confirm its
/// namespace-list pointers differ before and after calling
/// `rebuild_namespace_lists()` (proving the two algorithms really do
/// disagree on pointer values for identical content), then confirm the
/// hash is unaffected by the rebuild.
#[test]
fn hash_contract_still_excludes_namespace_list_pointers() {
    let mut state = KernelState::new();
    for i in 0u32..3 {
        state
            .apply_event(&KernelEvent::InsertRecord {
                id: RecordId(i),
                vector: FxpVector::new_zeros(DIM),
                metadata: None,
                tag: 0,
            })
            .unwrap();
    }

    let hash_before = hash_state_blake3(&state);
    let pointers_before: Vec<(u32, u32)> = (0..3)
        .map(|i| {
            let r = state.get_record(RecordId(i)).unwrap();
            (r.next_in_ns, r.prev_in_ns)
        })
        .collect();

    state.rebuild_namespace_lists();

    let pointers_after: Vec<(u32, u32)> = (0..3)
        .map(|i| {
            let r = state.get_record(RecordId(i)).unwrap();
            (r.next_in_ns, r.prev_in_ns)
        })
        .collect();
    let hash_after = hash_state_blake3(&state);

    assert_ne!(
        pointers_before, pointers_after,
        "setup check: rebuild_namespace_lists must actually produce a \
         different pointer ordering than live construction, or this test \
         proves nothing"
    );
    assert_eq!(
        hash_before, hash_after,
        "G0.2: next_in_ns/prev_in_ns are deliberately excluded from the \
         hash — rebuilding the namespace list pointers must not change the \
         state hash, even though the pointer VALUES themselves changed"
    );
}
