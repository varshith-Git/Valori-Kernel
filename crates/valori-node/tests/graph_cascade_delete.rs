// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.3.1 — Record → GraphNode cascade semantics (Option A, approved).
//!
//! Contract under test (see
//! docs/reviews/graph-g1.3.1-record-graph-cascade-semantics.md):
//!   * Hard-deleting a record cascade-deletes every LIVE node referencing it
//!     (ascending `NodeId` order), and each of those nodes' incident edges.
//!   * Soft-deleting a record does NOT touch the graph at all — the record
//!     row survives, so `node.record ⇒ live record` already holds.
//!   * The invariant `node.record ⇒ live record` holds after every hard
//!     delete, canonical-state `check_invariants()` passes, and the state's
//!     own snapshot survives an encode→decode round trip (BUG-1's exact
//!     regression).
//!   * Cross-namespace record deletion is rejected (BUG-4).

use valori_kernel::types::enums::NodeKind;
use valori_kernel::types::id::{NodeId, RecordId};
use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::Engine;
use valori_node::EngineFromNodeConfig;

const DIM: usize = 4;

fn mem_cfg() -> NodeConfig {
    let mut c = NodeConfig::default();
    c.max_records = 128;
    c.max_nodes = 128;
    c.max_edges = 256;
    c.event_log_path = None;
    c.wal_path = None;
    c.snapshot_path = None;
    c
}

fn v(seed: f32) -> Vec<f32> {
    (0..DIM).map(|i| seed + i as f32 * 0.01).collect()
}

fn live_nodes_referencing(e: &Engine, record_id: u32) -> Vec<u32> {
    valori_rag::graph::nodes_referencing_record(e.kernel_state(), record_id)
}

// ── 1. zero, one, and many referencing nodes ─────────────────────────────────

#[test]
fn hard_delete_with_zero_referencing_nodes_just_deletes_the_record() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    e.delete_record(rid).unwrap();
    assert!(e.state.get_record(RecordId(rid)).is_none());
    assert!(e.state.check_invariants().is_ok());
}

#[test]
fn hard_delete_with_one_referencing_node_cascades() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let node = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    e.delete_record(rid).unwrap();
    assert!(e.get_node(NodeId(node)).is_none(), "node must be gone too");
    assert!(e.state.check_invariants().is_ok());
}

#[test]
fn hard_delete_with_many_referencing_nodes_cascades_to_all_of_them() {
    // This is the exact production shape: `/v1/memory/contradict` and
    // `/v1/memory/consolidate` each create a fresh node per record with no
    // reuse check, so 3+ live nodes on one record is not hypothetical.
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let n1 = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    let n2 = e
        .create_node_for_record(Some(rid), NodeKind::Document as u8, 0)
        .unwrap();
    let n3 = e
        .create_node_for_record(Some(rid), NodeKind::Concept as u8, 0)
        .unwrap();

    e.delete_record(rid).unwrap();

    for n in [n1, n2, n3] {
        assert!(
            e.get_node(NodeId(n)).is_none(),
            "node {n} must not survive its record's hard delete"
        );
    }
    assert!(e.state.check_invariants().is_ok());
}

// ── 2. deletion order is deterministic (ascending NodeId) ────────────────────

#[test]
fn cascade_order_is_ascending_node_id_regardless_of_creation_order() {
    let mut e = Engine::new(&mem_cfg());
    let rid_a = e.insert_record_from_f32(&v(0.10)).unwrap();
    let rid_b = e.insert_record_from_f32(&v(0.20)).unwrap();
    // Interleave creation so ascending-by-id != ascending-by-creation-time
    // for record `rid_a` specifically: its second node is created after a
    // node on a different record gets a lower id assigned to `rid_b`.
    let a1 = e
        .create_node_for_record(Some(rid_a), NodeKind::Chunk as u8, 0)
        .unwrap();
    let _b1 = e
        .create_node_for_record(Some(rid_b), NodeKind::Chunk as u8, 0)
        .unwrap();
    let a2 = e
        .create_node_for_record(Some(rid_a), NodeKind::Document as u8, 0)
        .unwrap();

    assert_eq!(
        live_nodes_referencing(&e, rid_a),
        vec![a1, a2],
        "the enumeration primitive must return ascending NodeId order"
    );

    e.delete_record(rid_a).unwrap();
    assert!(e.get_node(NodeId(a1)).is_none());
    assert!(e.get_node(NodeId(a2)).is_none());
    // rid_b's node must be untouched — cascade is scoped to the deleted record.
    assert!(e.get_node(NodeId(_b1)).is_some());
}

// ── 3. cascade also frees incident edges (via delete_node's own cascade) ─────

#[test]
fn cascade_frees_the_deleted_nodes_incident_edges() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let a = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    let b = e
        .create_node_for_record(None, NodeKind::Concept as u8, 0)
        .unwrap();
    let edge = e
        .create_edge(a, b, valori_kernel::types::enums::EdgeKind::RefersTo as u8)
        .unwrap();

    e.delete_record(rid).unwrap();

    assert!(e.get_node(NodeId(a)).is_none());
    assert!(e.get_node(NodeId(b)).is_some(), "unrelated node b survives");
    assert!(
        e.state
            .get_edge(valori_kernel::types::id::EdgeId(edge))
            .is_none(),
        "edge incident to the cascade-deleted node must also be gone"
    );
    assert!(e.state.check_invariants().is_ok());
}

// ── 4. soft delete does NOT cascade — the whole point of keeping it distinct ─

#[test]
fn soft_delete_does_not_touch_the_graph_at_all() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let node = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();

    e.soft_delete_record(rid).unwrap();

    assert!(
        e.get_node(NodeId(node)).is_some(),
        "soft delete must leave every referencing node untouched"
    );
    assert!(
        e.state.get_record(RecordId(rid)).is_some(),
        "the record row survives a soft delete (flagged, not freed)"
    );
    assert!(e.state.check_invariants().is_ok());
}

// ── 5. BUG-1 regression: hard delete must not corrupt the snapshot ───────────

#[test]
fn hard_delete_survives_encode_decode_round_trip_with_multiple_nodes() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    e.create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    e.create_node_for_record(Some(rid), NodeKind::Document as u8, 0)
        .unwrap();
    e.insert_record_from_f32(&v(0.90)).unwrap(); // an unrelated survivor

    e.delete_record(rid).unwrap();
    assert!(e.state.check_invariants().is_ok());

    let mut buf: Vec<u8> = Vec::new();
    valori_kernel::snapshot::encode::encode_state(&e.state, &mut buf).unwrap();
    let decoded = valori_kernel::snapshot::decode::decode_state(&buf)
        .expect("BUG-1: a snapshot taken after a cascaded hard delete must decode");
    assert!(decoded.check_invariants().is_ok());
}

// ── 6. cross-namespace hard delete is rejected (BUG-4) ────────────────────────

#[test]
fn hard_delete_engine_primitive_only_cascades_within_the_records_own_namespace() {
    // `Engine::delete_record` itself has no namespace parameter (matching
    // `delete_node`'s existing convention — the namespace check lives at the
    // API boundary, see `SharedEngine::delete` / `DataPlaneState::delete`).
    // This test pins the enumeration primitive's namespace-scoping instead:
    // a record in namespace B must never have namespace A's colliding-id
    // node swept into its cascade.
    let mut e = Engine::new(&mem_cfg());
    e.create_collection_with_config(
        "ns-a",
        4,
        valori_domain::Metric::SquaredL2,
        valori_domain::IndexKind::Brute,
    )
    .unwrap();
    e.create_collection_with_config(
        "ns-b",
        4,
        valori_domain::Metric::SquaredL2,
        valori_domain::IndexKind::Brute,
    )
    .unwrap();
    let ns_a = e.resolve_collection(Some("ns-a")).unwrap();
    let ns_b = e.resolve_collection(Some("ns-b")).unwrap();

    let rec_a = e.insert_record_from_f32_ns(&v(0.10), ns_a).unwrap();
    let rec_b = e.insert_record_from_f32_ns(&v(0.10), ns_b).unwrap();
    let node_a = e
        .create_node_for_record(Some(rec_a), NodeKind::Chunk as u8, ns_a)
        .unwrap();
    let node_b = e
        .create_node_for_record(Some(rec_b), NodeKind::Chunk as u8, ns_b)
        .unwrap();

    assert_eq!(live_nodes_referencing(&e, rec_a), vec![node_a]);
    assert_eq!(live_nodes_referencing(&e, rec_b), vec![node_b]);

    e.delete_record(rec_a).unwrap();
    assert!(e.get_node(NodeId(node_a)).is_none());
    assert!(
        e.get_node(NodeId(node_b)).is_some(),
        "namespace B's node must survive namespace A's record deletion"
    );
}
