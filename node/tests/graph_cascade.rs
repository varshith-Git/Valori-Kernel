// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Graph cascade-delete and reverse-index tests.
//!
//! These tests verify that:
//!   1. `first_in_edge` / `next_in` back-pointers are populated correctly on
//!      every `create_edge` call.
//!   2. `delete_node` removes ALL incident edges (outgoing + incoming) without
//!      scanning the full edge pool — it walks the two linked lists whose heads
//!      are `first_out_edge` and `first_in_edge`, touching only O(degree) edges.
//!   3. `delete_edge` correctly unlinks from BOTH the outgoing and incoming
//!      lists so no dangling pointers survive.
//!   4. Unrelated edges are never affected by a node deletion.
//!   5. The reverse index survives a snapshot round-trip.

use valori_node::config::{NodeConfig, IndexKind};
use valori_node::engine::Engine;
use valori_kernel::types::id::{NodeId, EdgeId};

// ── helpers ──────────────────────────────────────────────────────────────────

fn bare_cfg() -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.dim = 4;
    cfg.max_records = 64;
    cfg.max_nodes   = 32;
    cfg.max_edges   = 64;
    cfg.index_kind  = IndexKind::BruteForce;
    cfg.event_log_path = None;
    cfg.wal_path       = None;
    cfg.snapshot_path  = None;
    cfg
}

/// Create a bare node (no record) and return its NodeId.
fn add_node(engine: &mut Engine) -> u32 {
    engine.create_node_for_record(None, 0).unwrap()
}

/// Return true when the edge slot is occupied.
fn edge_alive(engine: &Engine, eid: u32) -> bool {
    engine.state.is_edge_active(EdgeId(eid))
}

/// Return the `first_in_edge` head for a node, or None if the node is gone / empty.
fn first_in(engine: &Engine, nid: u32) -> Option<u32> {
    engine.state.get_node(NodeId(nid))?.first_in_edge.map(|e| e.0)
}

/// Return the `first_out_edge` head for a node, or None.
fn first_out(engine: &Engine, nid: u32) -> Option<u32> {
    engine.state.get_node(NodeId(nid))?.first_out_edge.map(|e| e.0)
}

/// Collect all outgoing edge IDs for a node by walking the linked list.
fn outgoing(engine: &Engine, nid: u32) -> Vec<u32> {
    engine.state
        .outgoing_edges(NodeId(nid))
        .map(|it| it.map(|e| e.id.0).collect())
        .unwrap_or_default()
}

// ── 1. Reverse index population ───────────────────────────────────────────────

/// `create_edge(A → B)` must set B's `first_in_edge` to the new edge.
#[test]
fn test_reverse_index_set_on_create_edge() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);
    let b = add_node(&mut e);

    let eid = e.create_edge(a, b, 0).unwrap();

    assert_eq!(
        first_in(&e, b),
        Some(eid),
        "B's first_in_edge must point at the new edge after A→B is created",
    );
    assert!(
        first_in(&e, a).is_none(),
        "A has no incoming edges; first_in_edge must remain None",
    );
}

/// Two edges pointing at the same destination must chain through `next_in`.
#[test]
fn test_reverse_index_chains_multiple_incoming() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);
    let b = add_node(&mut e);
    let c = add_node(&mut e);

    let e1 = e.create_edge(a, c, 0).unwrap(); // A → C  (first)
    let e2 = e.create_edge(b, c, 0).unwrap(); // B → C  (second; prepended → head)

    // Head is e2 (most recent), chain continues to e1.
    assert_eq!(first_in(&e, c), Some(e2), "most-recent incoming edge is the list head");

    // Walk the chain manually via `next_in`.
    let head_edge = e.state.get_edge(EdgeId(e2)).unwrap();
    assert_eq!(
        head_edge.next_in.map(|x| x.0),
        Some(e1),
        "next_in from e2 must reach e1",
    );

    assert_eq!(e.state.get_edge(EdgeId(e1)).unwrap().next_in, None);
}

// ── 2. DeleteNode — outgoing cascade ─────────────────────────────────────────

/// Deleting the *source* of an edge must remove the edge and clear B's incoming pointer.
#[test]
fn test_delete_source_node_removes_edge() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);
    let b = add_node(&mut e);

    let eid = e.create_edge(a, b, 0).unwrap();

    e.delete_node(a).unwrap();

    assert!(!edge_alive(&e, eid), "edge must be removed when its source node is deleted");
    assert!(
        first_in(&e, b).is_none(),
        "B's first_in_edge must be None after the only incoming edge was deleted",
    );
}

// ── 3. DeleteNode — incoming cascade ─────────────────────────────────────────

/// Deleting the *destination* of an edge must remove the edge and clear A's outgoing pointer.
#[test]
fn test_delete_destination_node_removes_edge() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);
    let b = add_node(&mut e);

    let eid = e.create_edge(a, b, 0).unwrap();

    e.delete_node(b).unwrap();

    assert!(!edge_alive(&e, eid), "edge must be removed when its destination node is deleted");
    assert!(
        first_out(&e, a).is_none(),
        "A's first_out_edge must be None after the only outgoing edge was deleted",
    );
}

// ── 4. DeleteNode — middle node ───────────────────────────────────────────────

/// A node in the middle of a chain (A → B → C) connects via both an incoming
/// and an outgoing edge.  Deleting B must remove both edges; A and C are untouched.
#[test]
fn test_delete_middle_node_removes_both_edges() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);
    let b = add_node(&mut e);
    let c = add_node(&mut e);

    let e1 = e.create_edge(a, b, 0).unwrap(); // A → B
    let e2 = e.create_edge(b, c, 0).unwrap(); // B → C

    e.delete_node(b).unwrap();

    assert!(!edge_alive(&e, e1), "A→B edge must be gone");
    assert!(!edge_alive(&e, e2), "B→C edge must be gone");
    assert!(first_out(&e, a).is_none(), "A has no more outgoing edges");
    assert!(first_in(&e, c).is_none(),  "C has no more incoming edges");

    // A and C themselves must still exist.
    assert!(e.state.get_node(NodeId(a)).is_some(), "A must survive");
    assert!(e.state.get_node(NodeId(c)).is_some(), "C must survive");
}

// ── 5. DeleteNode — hub with many incoming edges ──────────────────────────────

/// A hub node with N incoming edges: all N edges must be removed when the hub
/// is deleted, regardless of how many edges exist in total.
/// This is the O(in-degree) property — only the hub's incident edges are touched.
#[test]
fn test_delete_hub_clears_all_incoming_edges() {
    const SPOKES: u32 = 8;

    let mut e = Engine::new(&bare_cfg());
    let hub = add_node(&mut e);

    let mut spoke_ids  = Vec::new();
    let mut edge_ids   = Vec::new();

    for _ in 0..SPOKES {
        let s = add_node(&mut e);
        let eid = e.create_edge(s, hub, 0).unwrap(); // spoke → hub
        spoke_ids.push(s);
        edge_ids.push(eid);
    }

    assert_eq!(e.state.edge_count(), SPOKES as usize);

    e.delete_node(hub).unwrap();

    for &eid in &edge_ids {
        assert!(!edge_alive(&e, eid), "spoke→hub edge {} must be gone", eid);
    }
    // All spoke nodes survive.
    for &s in &spoke_ids {
        assert!(e.state.get_node(NodeId(s)).is_some(), "spoke {} must survive", s);
        assert!(
            first_out(&e, s).is_none(),
            "spoke {} must have no outgoing edges after hub deleted", s,
        );
    }
}

// ── 6. Unrelated edges survive node deletion ─────────────────────────────────

/// Edges between unrelated nodes must be completely unaffected by a node deletion.
#[test]
fn test_unrelated_edges_survive_node_deletion() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);
    let b = add_node(&mut e);
    let c = add_node(&mut e);
    let d = add_node(&mut e);

    let victim_edge   = e.create_edge(a, b, 0).unwrap(); // A → B  (will be deleted)
    let survivor_edge = e.create_edge(c, d, 0).unwrap(); // C → D  (must survive)

    e.delete_node(a).unwrap();

    assert!(!edge_alive(&e, victim_edge),   "A→B must be gone");
    assert!( edge_alive(&e, survivor_edge), "C→D must survive");
    assert_eq!(
        first_in(&e, d),
        Some(survivor_edge),
        "D's incoming pointer must still reference C→D",
    );
    assert_eq!(
        outgoing(&e, c),
        vec![survivor_edge],
        "C's outgoing list must still contain only C→D",
    );
}

// ── 7. DeleteEdge — unlinks from both lists ───────────────────────────────────

/// Deleting a single edge from a node that has multiple edges must leave the
/// remaining edges intact in both the outgoing and incoming lists.
#[test]
fn test_delete_edge_unlinks_from_both_lists() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);
    let b = add_node(&mut e);
    let c = add_node(&mut e);

    let e1 = e.create_edge(a, c, 0).unwrap(); // A → C
    let e2 = e.create_edge(a, b, 0).unwrap(); // A → B  (head of A's outgoing list)

    // Delete only A → B.
    e.delete_edge(e2).unwrap();

    assert!(!edge_alive(&e, e2), "A→B must be gone");
    assert!( edge_alive(&e, e1), "A→C must survive");

    // A's outgoing list must contain only e1 now.
    assert_eq!(outgoing(&e, a), vec![e1], "A's outgoing list must contain only A→C");

    // B's incoming list must be empty; C's must still hold e1.
    assert!(first_in(&e, b).is_none(),  "B has no more incoming edges");
    assert_eq!(first_in(&e, c), Some(e1), "C still has incoming edge from A");
}

/// Deleting an edge from the middle of an incoming list must stitch the
/// predecessor's `next_in` to the successor correctly.
#[test]
fn test_delete_middle_incoming_edge_stitches_list() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);
    let b = add_node(&mut e);
    let c = add_node(&mut e);
    let hub = add_node(&mut e);

    // Build incoming chain on hub: a→hub, b→hub, c→hub
    // After three prepends the order is: e3 → e2 → e1 → None
    let e1 = e.create_edge(a, hub, 0).unwrap();
    let e2 = e.create_edge(b, hub, 0).unwrap();
    let e3 = e.create_edge(c, hub, 0).unwrap();

    // Delete the middle one (e2 = b→hub).
    e.delete_edge(e2).unwrap();

    assert!(!edge_alive(&e, e2), "b→hub must be gone");
    assert!( edge_alive(&e, e1), "a→hub must survive");
    assert!( edge_alive(&e, e3), "c→hub must survive");

    // Walk the surviving incoming chain: should be e3 → e1 → None.
    assert_eq!(first_in(&e, hub), Some(e3), "head must still be e3 (c→hub)");
    let e3_next = e.state.get_edge(EdgeId(e3)).unwrap().next_in.map(|x| x.0);
    assert_eq!(
        e3_next,
        Some(e1),
        "e3.next_in must skip the deleted e2 and point to e1",
    );
    assert_eq!(
        e.state.get_edge(EdgeId(e1)).unwrap().next_in,
        None,
        "e1 is the tail; next_in must be None",
    );
}

// ── 8. Snapshot round-trip preserves reverse index ───────────────────────────

/// `first_in_edge` and `next_in` must survive a snapshot → restore cycle
/// (schema V4 serialization).
#[test]
fn test_snapshot_preserves_reverse_index() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);
    let b = add_node(&mut e);
    let c = add_node(&mut e);

    let e1 = e.create_edge(a, c, 0).unwrap();
    let e2 = e.create_edge(b, c, 0).unwrap(); // head

    // Save + restore.
    let snap = e.snapshot().unwrap();
    let mut e2_restored = Engine::new(&bare_cfg());
    e2_restored.restore(&snap).unwrap();

    // Incoming chain on C must be intact.
    assert_eq!(
        first_in(&e2_restored, c),
        Some(e2),
        "after restore, C's first_in_edge must still point to e2",
    );
    let e2_next = e2_restored.state.get_edge(EdgeId(e2)).unwrap().next_in.map(|x| x.0);
    assert_eq!(
        e2_next,
        Some(e1),
        "after restore, e2.next_in must still point to e1",
    );

    // Cascade delete must still work on the restored engine.
    e2_restored.delete_node(c).unwrap();
    assert!(!edge_alive(&e2_restored, e1), "a→c must be gone after cascade on restored engine");
    assert!(!edge_alive(&e2_restored, e2), "b→c must be gone after cascade on restored engine");
}

// ── 9. Self-loop ──────────────────────────────────────────────────────────────

/// A self-loop (A → A) is a single edge that appears in both the outgoing and
/// incoming list of the same node.  Deleting A must remove it exactly once.
#[test]
fn test_delete_node_with_self_loop() {
    let mut e = Engine::new(&bare_cfg());
    let a = add_node(&mut e);

    let eid = e.create_edge(a, a, 0).unwrap();
    assert!(edge_alive(&e, eid));

    // Must not panic or double-free.
    e.delete_node(a).unwrap();

    assert!(!edge_alive(&e, eid), "self-loop edge must be removed");
}
