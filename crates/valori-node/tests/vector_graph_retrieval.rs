// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! G1.3 — vector → graph retrieval semantics.
//!
//! The vector→graph bridge is: namespace-scoped vector KNN → `RecordId[]` →
//! `resolve_seed_nodes` (canonical `record` back-reference, first-in-pool-
//! order wins) → seed `NodeId[]` → `expand_subgraph` BFS. This file pins
//! that contract, including the two divergences G1.3 found and fixed:
//! standalone/cluster seed-resolution parity, and staleness across restart.
//!
//! Kernel-level determinism of the pieces (search ordering, BFS ordering,
//! snapshot/replay equivalence) is already proven by G0.1/G1.1; these tests
//! target the composition and the record↔node linkage specifically.

use tempfile::tempdir;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_node::config::{IndexKind, NodeConfig};
use valori_node::engine::{Engine, RecoveryMode};
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

fn durable_cfg(dir: &std::path::Path) -> NodeConfig {
    let mut c = mem_cfg();
    c.event_log_path = Some(dir.join("events.log"));
    c.snapshot_path = Some(dir.join("snapshot.bin"));
    c
}

fn v(seed: f32) -> Vec<f32> {
    (0..DIM).map(|i| seed + i as f32 * 0.01).collect()
}

/// The seed-resolution rule under test, applied to canonical state — the
/// exact function both execution paths now use.
fn resolve(engine: &Engine, record_ids: &[u32]) -> std::collections::HashMap<u32, u32> {
    valori_rag::graph::resolve_seed_nodes(engine.kernel_state(), record_ids)
}

// ── 1/2. vector hit with, and without, a graph node ──────────────────────────

#[test]
fn vector_hit_with_and_without_graph_node() {
    let mut e = Engine::new(&mem_cfg());
    let linked = e.insert_record_from_f32(&v(0.10)).unwrap();
    let unlinked = e.insert_record_from_f32(&v(0.50)).unwrap();
    let node = e
        .create_node_for_record(Some(linked), NodeKind::Chunk as u8, 0)
        .unwrap();

    let map = resolve(&e, &[linked, unlinked]);
    assert_eq!(map.get(&linked).copied(), Some(node));
    assert_eq!(
        map.get(&unlinked).copied(),
        None,
        "a record with no graph node must resolve to no seed — not an error"
    );

    // And the vector search itself still returns BOTH records: missing graph
    // linkage must never suppress a vector result.
    let hits = e.search_l2_ns(&v(0.10), 10, 0).unwrap();
    let ids: Vec<u32> = hits.iter().map(|(id, _)| *id).collect();
    assert!(ids.contains(&linked) && ids.contains(&unlinked));
}

// ── 3. graph node without a vector stays valid ───────────────────────────────

#[test]
fn graph_node_without_record_is_valid_and_never_a_seed() {
    let mut e = Engine::new(&mem_cfg());
    let structural = e
        .create_node_for_record(None, NodeKind::Concept as u8, 0)
        .unwrap();
    assert!(e
        .get_node(valori_kernel::types::id::NodeId(structural))
        .is_some());
    // It exists, but no record can ever resolve to it.
    assert!(resolve(&e, &[0, 1, 2]).is_empty());
}

// ── 4. THE PARITY REGRESSION: multiple nodes per record ──────────────────────

#[test]
fn multiple_nodes_per_record_resolve_deterministically_to_lowest_node_id() {
    // `CreateNode` imposes no uniqueness on `record`, so this is legal
    // canonical state — and it is exactly where the standalone cache
    // (last-write-wins) diverged from the cluster path (first-in-pool-order).
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let first = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    let second = e
        .create_node_for_record(Some(rid), NodeKind::Document as u8, 0)
        .unwrap();
    assert!(second > first, "sanity: node ids ascend");

    let resolved = resolve(&e, &[rid]).get(&rid).copied();
    assert_eq!(
        resolved,
        Some(first),
        "the documented rule is first-in-pool-order (lowest node id) wins"
    );

    // Repeated resolution is stable.
    for _ in 0..3 {
        assert_eq!(resolve(&e, &[rid]).get(&rid).copied(), Some(first));
    }
}

// ── 5. THE STALENESS REGRESSION: delete one of several nodes ─────────────────

#[test]
fn deleting_one_node_leaves_the_surviving_node_resolvable() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let first = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    let second = e
        .create_node_for_record(Some(rid), NodeKind::Document as u8, 0)
        .unwrap();

    e.delete_node(first).unwrap();

    assert_eq!(
        resolve(&e, &[rid]).get(&rid).copied(),
        Some(second),
        "the surviving node still points at the record, so it must still \
         resolve — the pre-G1.3 cache dropped this mapping entirely"
    );
}

// ── 6. deleted node / deleted edge / deleted record ──────────────────────────

#[test]
fn deleting_the_only_node_yields_no_seed_but_keeps_the_vector_hit() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let node = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    e.delete_node(node).unwrap();

    assert!(resolve(&e, &[rid]).is_empty(), "no live node -> no seed");
    let hits = e.search_l2_ns(&v(0.10), 5, 0).unwrap();
    assert!(
        hits.iter().any(|(id, _)| *id == rid),
        "the record is still a valid vector hit with no graph expansion"
    );
}

#[test]
fn deleting_an_edge_shrinks_expansion_but_keeps_the_seed() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let a = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    let b = e
        .create_node_for_record(None, NodeKind::Concept as u8, 0)
        .unwrap();
    let edge = e.create_edge(a, b, EdgeKind::RefersTo as u8).unwrap();

    let (nodes_before, _) = valori_rag::graph::expand_subgraph(e.kernel_state(), &[a], 2);
    assert_eq!(nodes_before.len(), 2, "seed + neighbour");

    e.delete_edge(edge).unwrap();
    let (nodes_after, edges_after) = valori_rag::graph::expand_subgraph(e.kernel_state(), &[a], 2);
    assert_eq!(nodes_after.len(), 1, "only the seed remains reachable");
    assert!(edges_after.is_empty());
    assert_eq!(
        resolve(&e, &[rid]).get(&rid).copied(),
        Some(a),
        "the seed itself is unaffected by edge deletion"
    );
}

#[test]
fn soft_deleted_record_drops_out_of_vector_results() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    e.insert_record_from_f32(&v(0.90)).unwrap();
    e.soft_delete_record(rid).unwrap();
    let hits = e.search_l2_ns(&v(0.10), 5, 0).unwrap();
    assert!(
        !hits.iter().any(|(id, _)| *id == rid),
        "a soft-deleted record must not seed graph expansion because it is \
         no longer a vector hit at all"
    );
}

// ── 7. namespace isolation, including an ID-collision scenario ───────────────

#[test]
fn namespace_isolation_with_colliding_ids() {
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

    // Same vector content in both namespaces, so a namespace-blind search
    // would happily return the wrong one.
    let rec_a = e.insert_record_from_f32_ns(&v(0.10), ns_a).unwrap();
    let rec_b = e.insert_record_from_f32_ns(&v(0.10), ns_b).unwrap();
    let node_a = e
        .create_node_for_record(Some(rec_a), NodeKind::Chunk as u8, ns_a)
        .unwrap();
    let node_b = e
        .create_node_for_record(Some(rec_b), NodeKind::Chunk as u8, ns_b)
        .unwrap();

    // Vector search is namespace-scoped: A's search never sees B's record.
    let hits_a = e.search_l2_ns(&v(0.10), 10, ns_a).unwrap();
    let ids_a: Vec<u32> = hits_a.iter().map(|(id, _)| *id).collect();
    assert!(ids_a.contains(&rec_a));
    assert!(
        !ids_a.contains(&rec_b),
        "namespace A search must not return namespace B's record"
    );

    // Seed resolution therefore only ever sees in-namespace record ids, and
    // the node it resolves is in that same namespace by construction
    // (`CreateNode` requires node.namespace_id == record.namespace_id).
    assert_eq!(resolve(&e, &ids_a).get(&rec_a).copied(), Some(node_a));
    assert_eq!(
        e.get_node(valori_kernel::types::id::NodeId(node_a))
            .unwrap()
            .namespace_id,
        ns_a
    );
    assert_eq!(
        e.get_node(valori_kernel::types::id::NodeId(node_b))
            .unwrap()
            .namespace_id,
        ns_b
    );
    assert_ne!(node_a, node_b, "distinct nodes, distinct namespaces");
}

// ── 8. empty cases ───────────────────────────────────────────────────────────

#[test]
fn empty_vector_result_and_empty_expansion_are_not_errors() {
    let e = Engine::new(&mem_cfg());
    // No records at all -> empty hits -> empty seeds -> empty subgraph.
    assert!(resolve(&e, &[]).is_empty());
    let (nodes, edges) = valori_rag::graph::expand_subgraph(e.kernel_state(), &[], 2);
    assert!(nodes.is_empty() && edges.is_empty());

    // A seed with no edges expands to just itself.
    let mut e2 = Engine::new(&mem_cfg());
    let rid = e2.insert_record_from_f32(&v(0.10)).unwrap();
    let n = e2
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    let (nodes, edges) = valori_rag::graph::expand_subgraph(e2.kernel_state(), &[n], 2);
    assert_eq!(nodes.len(), 1);
    assert!(edges.is_empty());
}

// ── 9. duplicate paths and cycles in the expanded subgraph ───────────────────

#[test]
fn duplicate_paths_and_cycles_dedupe_deterministically() {
    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let a = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    let b = e
        .create_node_for_record(None, NodeKind::Concept as u8, 0)
        .unwrap();
    let c = e
        .create_node_for_record(None, NodeKind::Concept as u8, 0)
        .unwrap();
    // Diamond a->b, a->c, b->d, c->d  plus a cycle d->a.
    let d = e
        .create_node_for_record(None, NodeKind::Concept as u8, 0)
        .unwrap();
    e.create_edge(a, b, EdgeKind::Relation as u8).unwrap();
    e.create_edge(a, c, EdgeKind::Relation as u8).unwrap();
    e.create_edge(b, d, EdgeKind::Relation as u8).unwrap();
    e.create_edge(c, d, EdgeKind::Relation as u8).unwrap();
    e.create_edge(d, a, EdgeKind::Relation as u8).unwrap(); // cycle back to seed

    let (nodes, _edges) = valori_rag::graph::expand_subgraph(e.kernel_state(), &[a], 4);
    assert_eq!(nodes.len(), 4, "d reached by two paths must appear once");

    // Deterministic across repeated runs.
    for _ in 0..3 {
        let (again, _) = valori_rag::graph::expand_subgraph(e.kernel_state(), &[a], 4);
        assert_eq!(nodes, again);
    }
}

// ── 10. multiple hits -> multiple seeds, deterministic ordering ──────────────

#[test]
fn multiple_vector_hits_produce_multiple_seeds_in_deterministic_order() {
    let mut e = Engine::new(&mem_cfg());
    let mut expected_nodes = Vec::new();
    for i in 0..5u32 {
        let rid = e
            .insert_record_from_f32(&v(0.10 + i as f32 * 0.01))
            .unwrap();
        // Every other record gets a node, so seeds are a strict subset.
        if i % 2 == 0 {
            let n = e
                .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
                .unwrap();
            expected_nodes.push((rid, n));
        }
    }

    let run = |eng: &Engine| -> Vec<u32> {
        let hits = eng.search_l2_ns(&v(0.10), 5, 0).unwrap();
        let record_ids: Vec<u32> = hits.iter().map(|(id, _)| *id).collect();
        let map = valori_rag::graph::resolve_seed_nodes(eng.kernel_state(), &record_ids);
        record_ids
            .iter()
            .filter_map(|r| map.get(r).copied())
            .collect()
    };

    let seeds = run(&e);
    assert_eq!(seeds.len(), 3, "only the 3 linked records seed the graph");
    for _ in 0..3 {
        assert_eq!(run(&e), seeds, "seed order must be stable");
    }
}

// ── 11. restart equivalence — the regression that motivated the fix ─────────

#[test]
fn seed_resolution_is_identical_across_a_real_restart() {
    let dir = tempdir().unwrap();
    let cfg = durable_cfg(dir.path());

    let (rid, before) = {
        let mut e = Engine::new(&cfg);
        assert_eq!(e.try_recover(), RecoveryMode::Fresh);
        let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
        let first = e
            .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
            .unwrap();
        e.create_node_for_record(Some(rid), NodeKind::Document as u8, 0)
            .unwrap();
        // Delete the lower-id node: pre-G1.3 this silently dropped the
        // mapping in-process but a restart restored it, so the same query
        // returned different results before vs. after the restart.
        e.delete_node(first).unwrap();

        let before = resolve(&e, &[rid]).get(&rid).copied();
        e.flush_pending_events().unwrap();
        (rid, before)
    };

    let after = {
        let mut e2 = Engine::new(&cfg);
        let mode = e2.try_recover();
        assert!(
            matches!(mode, RecoveryMode::EventLog(n) if n > 0),
            "expected event-log recovery, got {mode:?}"
        );
        resolve(&e2, &[rid]).get(&rid).copied()
    };

    assert_eq!(
        before, after,
        "seed resolution must be identical before and after a restart"
    );
    assert!(before.is_some(), "sanity: the surviving node must resolve");
}

// ── 12. snapshot equivalence ─────────────────────────────────────────────────

#[test]
fn seed_resolution_is_identical_across_snapshot_restore() {
    use valori_kernel::snapshot::decode::decode_state;
    use valori_kernel::snapshot::encode::{encode_capacity_hint, encode_state};

    let mut e = Engine::new(&mem_cfg());
    let rid = e.insert_record_from_f32(&v(0.10)).unwrap();
    let n1 = e
        .create_node_for_record(Some(rid), NodeKind::Chunk as u8, 0)
        .unwrap();
    let n2 = e
        .create_node_for_record(None, NodeKind::Concept as u8, 0)
        .unwrap();
    e.create_edge(n1, n2, EdgeKind::RefersTo as u8).unwrap();

    let before_seed = resolve(&e, &[rid]).get(&rid).copied();
    let (before_nodes, before_edges) =
        valori_rag::graph::expand_subgraph(e.kernel_state(), &[n1], 2);

    let mut buf = Vec::with_capacity(encode_capacity_hint(e.kernel_state()));
    encode_state(e.kernel_state(), &mut buf).unwrap();
    let restored = decode_state(&buf).unwrap();

    let after_seed = valori_rag::graph::resolve_seed_nodes(&restored, &[rid])
        .get(&rid)
        .copied();
    let (after_nodes, after_edges) = valori_rag::graph::expand_subgraph(&restored, &[n1], 2);

    assert_eq!(before_seed, after_seed);
    assert_eq!(before_nodes, after_nodes);
    assert_eq!(before_edges, after_edges);
}
