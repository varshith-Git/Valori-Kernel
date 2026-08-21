// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Native GraphRAG: retrieve the subgraph around the K nearest vectors in one
//! pass over a single consistent kernel snapshot.
//!
//! Vectors and the knowledge graph live in the same `KernelState`, so a vector
//! KNN, the record→node resolution, and the subgraph BFS all run against one
//! snapshot with no second system and no cross-store drift. Both the standalone
//! (`server.rs`) and cluster (`cluster_server.rs`) data planes call into here so
//! the traversal stays identical by construction, not by copy-paste.

use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::{json, Value};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::NodeId;

/// Hard cap on traversal depth — mirrors the existing `/graph/subgraph` limit so
/// a hostile `depth` can't fan out the whole graph.
pub const MAX_DEPTH: u32 = 4;

// ── G1.1 — deterministic graph query primitives ─────────────────────────────
//
// A single, filterable, depth-and-result-bounded traversal primitive that
// future phases (hybrid retrieval, GraphRAG improvements) build on top of,
// rather than each reimplementing BFS. Reuses `KernelState::outgoing_edges`/
// `incoming_edges`/`get_node` — no new canonical state, no new index.
//
// Deliberately narrower than `expand_subgraph`: `query_graph` always
// validates the start node's namespace explicitly (see the doc comment on
// `query_graph` for why), excludes the start node from results (§7 of the
// G1.0 contract's worked example), and returns results in a declared,
// sorted order rather than raw BFS-visitation order.

/// Default traversal depth when a caller does not specify one. An
/// engineering default for this phase, not a product/billing limit — see
/// `docs/reviews/graph-g1.1-query-primitives.md`.
pub const DEFAULT_QUERY_DEPTH: u32 = 2;

/// Default result-count cap when a caller does not specify one.
pub const DEFAULT_QUERY_LIMIT: usize = 100;

/// Hard cap on the result-count limit a caller may request, regardless of
/// what they ask for — a safety bound, not a pricing tier.
pub const MAX_QUERY_LIMIT: usize = 1000;

/// `serde(default = "...")` requires a function, not a const path — these
/// just forward to the single source of truth above.
pub fn default_query_depth() -> u32 {
    DEFAULT_QUERY_DEPTH
}
pub fn default_query_limit() -> usize {
    DEFAULT_QUERY_LIMIT
}

/// Which edge direction(s) a query traverses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Follow `from -> to` edges (this node is the source).
    Outgoing,
    /// Follow `to -> from` edges (this node is the destination) — i.e. walk
    /// `first_in_edge`/`next_in`.
    Incoming,
    /// Follow both directions from every frontier node.
    Both,
}

/// A deterministic, bounded graph traversal query.
///
/// Only `start` is required. `direction`, `max_depth`, and `limit` are
/// always meaningful defaults (never "unbounded") — see the field docs for
/// exact bounds. `edge_kind`/`node_kind` are the only genuinely optional
/// fields: `None` means "no filter."
#[derive(Clone, Debug)]
pub struct GraphQuery {
    /// The node to traverse from. Required — there is no meaningful
    /// "start anywhere" query.
    pub start: NodeId,
    /// Which edges to follow. Defaulted to `Outgoing` by callers that don't
    /// care (matches `expand_subgraph`'s existing outgoing-only behavior).
    pub direction: Direction,
    /// Restricts which EDGES are followed during traversal (not just which
    /// are reported) — an edge whose kind doesn't match is never walked, so
    /// a node reachable only through a non-matching edge kind is not
    /// visited. `None` = no restriction.
    pub edge_kind: Option<EdgeKind>,
    /// Restricts which NODES are entered during traversal (not just which
    /// are reported) — symmetric with `edge_kind`: a non-matching node is a
    /// dead end, not expanded through. `None` = no restriction. The start
    /// node itself is never filtered by this (it is never returned or
    /// re-entered regardless of its own kind — see `query_graph`'s doc).
    pub node_kind: Option<NodeKind>,
    /// Maximum hop count from `start`. Clamped to `[0, MAX_DEPTH]` — never
    /// rejected, silently bounded, matching `expand_subgraph`'s existing
    /// convention. `0` is a valid, meaningful input: "traverse nothing,
    /// return an empty result" (distinct from a missing/invalid start node,
    /// which is `None`, not an empty `Vec`).
    pub max_depth: u32,
    /// Maximum number of results returned. Clamped to `[1, MAX_QUERY_LIMIT]`
    /// (unlike `max_depth`, `0` is floored to `1` — an empty *query* is
    /// meaningful via `max_depth: 0`; an empty *limit* is not a separate
    /// concept worth supporting and would just be a confusing way to spell
    /// the same thing).
    pub limit: usize,
}

/// One traversal result. All hits share the query's namespace by
/// construction — edges cannot cross namespaces (enforced at
/// `apply_event_ns`), so this is not re-validated per hit here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphQueryHit {
    pub node_id: u32,
    pub kind: NodeKind,
    pub record_id: Option<u32>,
    /// Hop count from the start node (1 = direct neighbor). If a node is
    /// reachable via multiple paths, this is its SHORTEST such distance —
    /// the standard first-visit-wins BFS property.
    pub depth: u32,
}

/// Runs a deterministic, bounded graph query.
///
/// Returns `None` if `start` does not exist, or exists but is not in
/// `namespace_id` — both cases collapse to the same `None` so a caller
/// cannot distinguish "wrong namespace" from "does not exist" (the standard
/// tenant-isolation-safe behavior; see the G1.1 review doc for why this
/// explicit check exists here even though edges themselves cannot cross
/// namespaces — it guards the *start* parameter, which is caller-supplied
/// and not otherwise namespace-checked before traversal begins).
///
/// Returns `Some(hits)` otherwise — `hits` may be empty (e.g. `max_depth: 0`,
/// or an isolated node with no matching neighbors).
///
/// # Ordering contract
/// Results are ordered by ascending `depth`, then ascending `node_id`
/// within the same depth — a declared, sorted order, not raw BFS-visitation
/// order (which is deterministic but implementation-defined — see G1.0
/// §9). If `limit` would truncate the result set, the traversal completes
/// in full first (bounded by `max_depth`, itself capped at
/// [`MAX_DEPTH`]), is sorted by `(depth, node_id)`, and *then* truncated —
/// so `limit` always keeps the `limit` closest results, never an
/// arbitrary BFS-order-dependent subset.
///
/// # Determinism
/// Given the same canonical graph and the same query, this returns the
/// same result in the same order — every source of variation (visited-set
/// membership tests, adjacency iteration) affects only which nodes are
/// found, never their reported order, which is always the explicit sort
/// above.
pub fn query_graph(
    state: &KernelState,
    namespace_id: u16,
    query: &GraphQuery,
) -> Option<Vec<GraphQueryHit>> {
    let start_node = state.get_node(query.start)?;
    if start_node.namespace_id != namespace_id {
        return None;
    }

    let max_depth = query.max_depth.min(MAX_DEPTH);
    let limit = query.limit.clamp(1, MAX_QUERY_LIMIT);

    let mut visited: HashSet<u32> = HashSet::new();
    visited.insert(query.start.0); // the start node is never re-entered or reported
    let mut queue: VecDeque<(NodeId, u32)> = VecDeque::new();
    queue.push_back((query.start, 0));

    let mut hits: Vec<GraphQueryHit> = Vec::new();

    while let Some((nid, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue; // do not expand past the requested bound
        }
        let mut visit = |neighbor_kind: EdgeKind, neighbor: NodeId| {
            if let Some(ek) = query.edge_kind {
                if neighbor_kind != ek {
                    return;
                }
            }
            if !visited.insert(neighbor.0) {
                return; // already visited (cycle, self-loop, or a shorter path already found it)
            }
            let next_depth = depth + 1;
            if let Some(node) = state.get_node(neighbor) {
                // `node_kind` restricts traversal, not just output: a node
                // that fails the filter is a dead end — recorded nowhere,
                // and never enqueued for further expansion.
                if query.node_kind.is_none_or(|k| node.kind == k) {
                    hits.push(GraphQueryHit {
                        node_id: neighbor.0,
                        kind: node.kind,
                        record_id: node.record.map(|r| r.0),
                        depth: next_depth,
                    });
                    queue.push_back((neighbor, next_depth));
                }
            }
        };

        match query.direction {
            Direction::Outgoing => {
                if let Some(iter) = state.outgoing_edges(nid) {
                    for edge in iter {
                        visit(edge.kind, edge.to);
                    }
                }
            }
            Direction::Incoming => {
                if let Some(iter) = state.incoming_edges(nid) {
                    for edge in iter {
                        visit(edge.kind, edge.from);
                    }
                }
            }
            Direction::Both => {
                if let Some(iter) = state.outgoing_edges(nid) {
                    for edge in iter {
                        visit(edge.kind, edge.to);
                    }
                }
                if let Some(iter) = state.incoming_edges(nid) {
                    for edge in iter {
                        visit(edge.kind, edge.from);
                    }
                }
            }
        }
    }

    hits.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.node_id.cmp(&b.node_id)));
    hits.truncate(limit);
    Some(hits)
}

/// All live node ids whose `record` back-reference is `record_id`, in
/// ascending `NodeId` order.
///
/// `CreateNode` imposes no uniqueness on `record` (G1.3.1's audit confirmed
/// this is exercised in production — `/v1/memory/contradict` and
/// `/v1/memory/consolidate` both create a fresh node per record with no
/// reuse check), so a record can have zero, one, or many referencing nodes.
/// This is the enumeration primitive record deletion needs to cascade
/// correctly to *every* referencing node, not just one cached mapping. See
/// docs/reviews/graph-g1.3.1-record-graph-cascade-semantics.md.
pub fn nodes_referencing_record(state: &KernelState, record_id: u32) -> Vec<u32> {
    let mut ids: Vec<u32> = state
        .iter_nodes()
        .filter(|n| n.record.map(|r| r.0) == Some(record_id))
        .map(|n| n.id.0)
        .collect();
    ids.sort_unstable();
    ids
}

/// Resolve `record_id → node_id` for a specific set of records by scanning the
/// node pool once.
///
/// Both execution paths resolve seeds straight from canonical `KernelState`
/// (no cached mapping — see G1.3 and G1.3.1, which each found and fixed a
/// divergence caused by the standalone engine's old `record_to_node` cache).
/// First node wins per record (deterministic in iteration order), which for
/// the standard ingest path is the record's `Chunk` node.
pub fn resolve_seed_nodes(state: &KernelState, record_ids: &[u32]) -> HashMap<u32, u32> {
    let want: HashSet<u32> = record_ids.iter().copied().collect();
    let mut map: HashMap<u32, u32> = HashMap::with_capacity(want.len());
    if want.is_empty() {
        return map;
    }
    for node in state.iter_nodes() {
        if let Some(rid) = node.record {
            if want.contains(&rid.0) {
                map.entry(rid.0).or_insert(node.id.0);
            }
        }
    }
    map
}

/// G1.4.1 — multi-source, bounded, direction-scoped BFS from `seeds`,
/// returning each reached node's SHORTEST hop distance from the nearest
/// seed. Seed nodes themselves get distance `0`.
///
/// Pure and deterministic: `HashSet`-guarded first-visit-wins (same
/// property `query_graph`/`expand_subgraph` already rely on), so a node
/// reachable via multiple equal-length paths, from multiple seeds, or via
/// duplicate edges (allowed and never deduplicated per G1.0 §9) always
/// records the same shortest distance regardless of traversal order. Never
/// mutates state; takes no lock beyond the caller's own `&KernelState`
/// borrow. `max_depth` is clamped to [`MAX_DEPTH`], matching every other
/// bounded traversal in this module.
///
/// Used by `valori-search::graph_rerank` (G1.4.1) to compute the graph
/// signal for reranking; not itself namespace-aware — callers must ensure
/// `seeds` are already namespace-scoped (namespace isolation for graph
/// reads is enforced at the `GraphOps`/`RecordOps` API boundary, per
/// G1.1.1's established pattern, not inside pure kernel-traversal
/// primitives like this one).
pub fn graph_distances_from_seeds(
    state: &KernelState,
    seeds: &[u32],
    direction: Direction,
    max_depth: u32,
) -> HashMap<u32, u32> {
    let max_depth = max_depth.min(MAX_DEPTH);
    let mut distances: HashMap<u32, u32> = HashMap::new();
    let mut queue: VecDeque<(NodeId, u32)> = VecDeque::new();

    // Seed in ascending order so visitation order (and therefore which
    // equal-length path "wins" ties in intermediate bookkeeping — though
    // the recorded distance is path-independent, see doc above) is itself
    // deterministic and reproducible, not dependent on caller-supplied
    // slice order.
    let mut sorted_seeds: Vec<u32> = seeds.to_vec();
    sorted_seeds.sort_unstable();
    sorted_seeds.dedup();

    for &seed in &sorted_seeds {
        if distances.insert(seed, 0).is_none() {
            queue.push_back((NodeId(seed), 0));
        }
    }

    while let Some((nid, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        let mut visit = |kind: EdgeKind, neighbor: NodeId| {
            let _ = kind;
            distances.entry(neighbor.0).or_insert_with(|| {
                queue.push_back((neighbor, depth + 1));
                depth + 1
            });
        };
        if matches!(direction, Direction::Outgoing | Direction::Both) {
            if let Some(iter) = state.outgoing_edges(nid) {
                for edge in iter {
                    visit(edge.kind, edge.to);
                }
            }
        }
        if matches!(direction, Direction::Incoming | Direction::Both) {
            if let Some(iter) = state.incoming_edges(nid) {
                for edge in iter {
                    visit(edge.kind, edge.from);
                }
            }
        }
    }

    distances
}

/// Breadth-first expansion from one or more seed nodes, returning the visited
/// nodes and traversed edges as JSON. Nodes and edges are de-duplicated; a node
/// is emitted once even when reached from multiple seeds.
///
/// `depth` is clamped to [`MAX_DEPTH`]. The JSON shapes match the long-standing
/// `/graph/subgraph` response so existing clients keep working.
///
/// This is a convenience wrapper over [`expand_subgraph_budgeted`] with no
/// node/edge budget (unlimited traversal, bounded only by `depth`).
pub fn expand_subgraph(state: &KernelState, seeds: &[u32], depth: u32) -> (Vec<Value>, Vec<Value>) {
    expand_subgraph_budgeted(state, seeds, depth, None, None)
}

/// Bounded breadth-first expansion — identical to [`expand_subgraph`] but with
/// hard stops on the number of nodes and edges visited during traversal.
///
/// BFS halts (and returns what it has so far) as soon as either limit is reached:
/// - `max_nodes` — stop before visiting a new node when the count would exceed this.
/// - `max_edges` — stop emitting edges for a node when the count would exceed this.
///
/// `None` means unlimited for that dimension. When both are `None` this is
/// equivalent to [`expand_subgraph`].
///
/// **Use case**: prevents runaway traversal in dense graphs at high branching
/// factor. The returned nodes/edges are always internally consistent (every edge
/// in `edges_out` has its `from` node in `nodes_out`; destination nodes may be
/// absent when the budget ran out before they were processed).
pub fn expand_subgraph_budgeted(
    state: &KernelState,
    seeds: &[u32],
    depth: u32,
    max_nodes: Option<u32>,
    max_edges: Option<u32>,
) -> (Vec<Value>, Vec<Value>) {
    let depth = depth.min(MAX_DEPTH);
    let max_nodes_usize = max_nodes.map(|n| n as usize).unwrap_or(usize::MAX);
    let max_edges_usize = max_edges.map(|e| e as usize).unwrap_or(usize::MAX);

    let mut visited_nodes: HashSet<u32> = HashSet::new();
    let mut visited_edges: HashSet<u32> = HashSet::new();
    let mut nodes_out: Vec<Value> = Vec::new();
    let mut edges_out: Vec<Value> = Vec::new();
    let mut queue: VecDeque<(u32, u32)> = VecDeque::new();

    for &seed in seeds {
        queue.push_back((seed, depth));
    }

    'bfs: while let Some((nid, rem)) = queue.pop_front() {
        if nodes_out.len() >= max_nodes_usize {
            break 'bfs;
        }
        if !visited_nodes.insert(nid) {
            continue;
        }
        if let Some(node) = state.get_node(NodeId(nid)) {
            nodes_out.push(json!({
                "id": node.id.0,
                "kind": node.kind as u8,
                "record": node.record.map(|r| r.0),
            }));
            if rem > 0 {
                if let Some(iter) = state.outgoing_edges(NodeId(nid)) {
                    for edge in iter {
                        if edges_out.len() >= max_edges_usize {
                            // Edge budget exhausted — stop emitting edges for this
                            // node, but do not break the outer BFS loop so already-
                            // queued nodes can still be visited (within the node budget).
                            break;
                        }
                        if visited_edges.insert(edge.id.0) {
                            edges_out.push(json!({
                                "id": edge.id.0,
                                "from": edge.from.0,
                                "to": edge.to.0,
                                "kind": edge.kind as u8,
                            }));
                        }
                        if !visited_nodes.contains(&edge.to.0) {
                            queue.push_back((edge.to.0, rem - 1));
                        }
                    }
                }
            }
        }
    }

    (nodes_out, edges_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_seeds_returns_empty() {
        use valori_kernel::state::kernel::KernelState;
        let state = KernelState::new();
        let (nodes, edges) = expand_subgraph(&state, &[], 2);
        assert!(nodes.is_empty());
        assert!(edges.is_empty());
    }

    #[test]
    fn resolve_seeds_empty_state() {
        use valori_kernel::state::kernel::KernelState;
        let state = KernelState::new();
        let result = resolve_seed_nodes(&state, &[1, 2, 3]);
        assert!(result.is_empty());
    }

    // ── G1.4.1 — graph_distances_from_seeds ───────────────────────────────────

    #[test]
    fn distances_empty_seeds_returns_empty() {
        let state = nontrivial_graph();
        assert!(graph_distances_from_seeds(&state, &[], Direction::Outgoing, 4).is_empty());
    }

    #[test]
    fn distances_seed_itself_is_zero() {
        let state = nontrivial_graph();
        let d = graph_distances_from_seeds(&state, &[0], Direction::Outgoing, 4);
        assert_eq!(d.get(&0), Some(&0));
    }

    #[test]
    fn distances_outgoing_diamond_shortest_wins() {
        // 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3: node 3 reachable via two
        // length-2 paths — must record 2, not be affected by which path
        // the BFS explores first.
        let state = nontrivial_graph();
        let d = graph_distances_from_seeds(&state, &[0], Direction::Outgoing, 4);
        assert_eq!(d.get(&1), Some(&1));
        assert_eq!(d.get(&2), Some(&1));
        assert_eq!(d.get(&3), Some(&2));
    }

    #[test]
    fn distances_incoming_walks_edges_backwards() {
        let state = nontrivial_graph();
        // From node 3 (the diamond's sink), incoming reaches 1 and 2 at
        // depth 1, and 0 at depth 2 — the mirror image of the outgoing case.
        let d = graph_distances_from_seeds(&state, &[3], Direction::Incoming, 4);
        assert_eq!(d.get(&1), Some(&1));
        assert_eq!(d.get(&2), Some(&1));
        assert_eq!(d.get(&0), Some(&2));
    }

    #[test]
    fn distances_both_merges_directions() {
        let state = nontrivial_graph();
        // From node 1: outgoing reaches 3 (depth 1); incoming reaches 0
        // (depth 1). `Both` must find both.
        let d = graph_distances_from_seeds(&state, &[1], Direction::Both, 4);
        assert_eq!(d.get(&3), Some(&1));
        assert_eq!(d.get(&0), Some(&1));
    }

    #[test]
    fn distances_multiple_seeds_take_the_nearer_one() {
        let state = nontrivial_graph();
        // Seeding from both 1 and 2 (each already depth-1 from 0): node 3
        // is depth-1 from either seed directly, must record 1, not 2.
        let d = graph_distances_from_seeds(&state, &[1, 2], Direction::Outgoing, 4);
        assert_eq!(d.get(&3), Some(&1));
    }

    #[test]
    fn distances_respect_max_depth_bound() {
        let state = nontrivial_graph();
        let d = graph_distances_from_seeds(&state, &[0], Direction::Outgoing, 1);
        assert_eq!(d.get(&1), Some(&1));
        assert_eq!(d.get(&3), None, "depth 2 must not appear when max_depth=1");
    }

    #[test]
    fn distances_unreachable_node_is_absent() {
        let state = nontrivial_graph();
        // Seed from 3 outgoing: only the self-loop is reachable (3 -> 3),
        // node 0/1/2 are not reachable via outgoing edges from 3.
        let d = graph_distances_from_seeds(&state, &[3], Direction::Outgoing, 4);
        assert_eq!(d.get(&0), None);
        assert_eq!(d.get(&1), None);
        assert_eq!(d.get(&2), None);
    }

    #[test]
    fn distances_are_order_independent_over_seed_slice_order() {
        let state = nontrivial_graph();
        let a = graph_distances_from_seeds(&state, &[1, 2], Direction::Outgoing, 4);
        let b = graph_distances_from_seeds(&state, &[2, 1], Direction::Outgoing, 4);
        let mut av: Vec<_> = a.into_iter().collect();
        let mut bv: Vec<_> = b.into_iter().collect();
        av.sort();
        bv.sort();
        assert_eq!(av, bv);
    }

    // ── G0.1 Phase 9 — deterministic traversal ────────────────────────────────
    //
    // A nontrivial graph (fan-out, a shared descendant reached via two paths,
    // and a cycle) — a stronger case than the trivial empty-input tests
    // above. Builds the same `KernelState` twice (independent instances,
    // same event sequence — the same "replay" pattern used for the
    // kernel-level determinism tests) and runs `expand_subgraph` three times
    // to prove T1 == T2 == T3 for both node and edge output, including
    // order (not just membership as a set).
    fn nontrivial_graph() -> valori_kernel::state::kernel::KernelState {
        use valori_kernel::event::KernelEvent;
        use valori_kernel::types::enums::{EdgeKind, NodeKind};
        use valori_kernel::types::id::{EdgeId, NodeId};

        let mut s = valori_kernel::state::kernel::KernelState::new();
        // 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3 (diamond: 3 reachable via two
        // paths), 3 -> 3 (self-loop, forces the visited-set dedup path).
        for i in 0u32..4 {
            s.apply_event(&KernelEvent::CreateNode {
                id: NodeId(i),
                kind: NodeKind::Concept,
                record: None,
            })
            .unwrap();
        }
        let edges = [(0u32, 1u32), (0, 2), (1, 3), (2, 3), (3, 3)];
        for (i, (from, to)) in edges.into_iter().enumerate() {
            s.apply_event(&KernelEvent::CreateEdge {
                id: EdgeId(i as u32),
                from: NodeId(from),
                to: NodeId(to),
                kind: EdgeKind::Relation,
            })
            .unwrap();
        }
        s
    }

    #[test]
    fn traversal_output_is_deterministic_across_repeated_runs() {
        let g1 = nontrivial_graph();
        let g2 = nontrivial_graph(); // independent rebuild, same event sequence

        let run = |state: &valori_kernel::state::kernel::KernelState| {
            let (nodes, edges) = expand_subgraph(state, &[0], 4);
            (
                nodes.iter().map(|v| v["id"].clone()).collect::<Vec<_>>(),
                edges.iter().map(|v| v["id"].clone()).collect::<Vec<_>>(),
            )
        };

        let t1 = run(&g1);
        let t2 = run(&g1);
        let t3 = run(&g1);
        assert_eq!(t1, t2, "T1 vs T2 on the same state must be identical");
        assert_eq!(t2, t3, "T2 vs T3 on the same state must be identical");

        // And across two independently-built (but event-identical) states —
        // proving determinism is a property of the graph, not of one
        // particular in-memory instance.
        let t_g2 = run(&g2);
        assert_eq!(
            t1, t_g2,
            "traversal over an independently-rebuilt identical graph must match"
        );

        // Sanity: the diamond + self-loop actually got visited (not a
        // vacuous pass over an empty result).
        assert_eq!(t1.0.len(), 4, "all 4 nodes must be visited");
        assert_eq!(
            t1.1.len(),
            5,
            "all 5 edges (incl. the self-loop) must be visited"
        );
    }

    // ── G1.1 — deterministic graph query primitives ───────────────────────────
    //
    // Fixture matches the G1.0/G1.1 worked example structurally (Alice ->
    // KNOWS -> Bob, -> WORKS_AT -> Acme, -> CREATED -> Project-X), mapped
    // onto the real, fixed `EdgeKind`/`NodeKind` enums (there is no "KNOWS"
    // variant — Valori's relationship vocabulary is closed, per G1.0 §2).
    // Mapping used throughout: KNOWS -> EdgeKind::Follows, WORKS_AT ->
    // EdgeKind::ByAgent, CREATED -> EdgeKind::ParentOf; Bob -> NodeKind::User
    // ("Person"-like), Acme -> NodeKind::Agent ("Organization"-like),
    // Project-X -> NodeKind::Document ("Project"-like).
    use valori_kernel::event::KernelEvent;
    use valori_kernel::types::id::{EdgeId, RecordId};

    /// Alice(0) --Follows--> Bob(1) [User]
    /// Alice(0) --ByAgent--> Acme(2) [Agent]
    /// Alice(0) --ParentOf--> ProjectX(3) [Document]
    /// All in namespace 0 unless noted.
    fn alice_graph() -> KernelState {
        let mut s = KernelState::new();
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(0),
            kind: NodeKind::Concept, // Alice
            record: None,
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(1),
            kind: NodeKind::User, // Bob
            record: None,
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(2),
            kind: NodeKind::Agent, // Acme
            record: None,
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(3),
            kind: NodeKind::Document, // Project-X
            record: None,
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(0),
            from: NodeId(0),
            to: NodeId(1),
            kind: EdgeKind::Follows, // KNOWS
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(1),
            from: NodeId(0),
            to: NodeId(2),
            kind: EdgeKind::ByAgent, // WORKS_AT
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(2),
            from: NodeId(0),
            to: NodeId(3),
            kind: EdgeKind::ParentOf, // CREATED
        })
        .unwrap();
        s
    }

    fn base_query(start: u32) -> GraphQuery {
        GraphQuery {
            start: NodeId(start),
            direction: Direction::Outgoing,
            edge_kind: None,
            node_kind: None,
            max_depth: DEFAULT_QUERY_DEPTH,
            limit: DEFAULT_QUERY_LIMIT,
        }
    }

    // 1. single-node lookup (depth 0 = "does start exist, no expansion")
    #[test]
    fn depth_zero_returns_empty_for_existing_start() {
        let s = alice_graph();
        let q = GraphQuery {
            max_depth: 0,
            ..base_query(0)
        };
        let hits = query_graph(&s, 0, &q).expect("start exists");
        assert!(hits.is_empty());
    }

    // 2. missing-node lookup
    #[test]
    fn missing_start_node_returns_none() {
        let s = alice_graph();
        let hits = query_graph(&s, 0, &base_query(999));
        assert!(hits.is_none(), "a nonexistent start node must yield None");
    }

    // 3. direct outgoing neighbor + relationship-type filtering (5, and the
    //    worked example from the prompt: edge_type=KNOWS -> Bob).
    #[test]
    fn outgoing_neighbor_filtered_by_edge_kind() {
        let s = alice_graph();
        let q = GraphQuery {
            edge_kind: Some(EdgeKind::Follows), // KNOWS
            max_depth: 1,
            ..base_query(0)
        };
        let hits = query_graph(&s, 0, &q).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].node_id, 1, "must return Bob only");
    }

    #[test]
    fn outgoing_neighbor_filtered_by_different_edge_kind() {
        let s = alice_graph();
        let q = GraphQuery {
            edge_kind: Some(EdgeKind::ByAgent), // WORKS_AT
            max_depth: 1,
            ..base_query(0)
        };
        let hits = query_graph(&s, 0, &q).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].node_id, 2, "must return Acme only");
    }

    // 4. direct incoming neighbor
    #[test]
    fn incoming_neighbor_from_bobs_perspective() {
        let s = alice_graph();
        let q = GraphQuery {
            direction: Direction::Incoming,
            max_depth: 1,
            ..base_query(1) // start at Bob
        };
        let hits = query_graph(&s, 0, &q).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].node_id, 0, "Bob's only incoming neighbor is Alice");
    }

    // 5. both-direction traversal
    #[test]
    fn both_direction_traversal_from_bob_reaches_alice_going_incoming() {
        let s = alice_graph();
        let q = GraphQuery {
            direction: Direction::Both,
            max_depth: 1,
            ..base_query(1)
        };
        let hits = query_graph(&s, 0, &q).unwrap();
        // Bob has no outgoing edges, one incoming (from Alice).
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].node_id, 0);
    }

    // 7. node-kind filtering: "node_kind = Person(User)" -> Bob, per the
    //    worked example (all three of Alice's neighbors are reachable at
    //    depth 1, but only Bob has kind User).
    #[test]
    fn node_kind_filter_returns_only_matching_kind() {
        let s = alice_graph();
        let q = GraphQuery {
            node_kind: Some(NodeKind::User),
            max_depth: 1,
            ..base_query(0)
        };
        let hits = query_graph(&s, 0, &q).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].node_id, 1);
        assert_eq!(hits[0].kind, NodeKind::User);
    }

    // node_kind restricts TRAVERSAL, not just output — a node further out
    // than a non-matching node must still be unreachable through it.
    #[test]
    fn node_kind_filter_blocks_traversal_through_non_matching_nodes() {
        let mut s = alice_graph();
        // Extend Acme (Agent, node 2) with a further hop to a Concept node.
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(4),
            kind: NodeKind::Concept,
            record: None,
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(3),
            from: NodeId(2),
            to: NodeId(4),
            kind: EdgeKind::Relation,
        })
        .unwrap();

        // Filtering to User: Acme (Agent) fails the filter, so node 4
        // (reachable only through Acme) must not appear even at depth 2.
        let q = GraphQuery {
            node_kind: Some(NodeKind::User),
            max_depth: 2,
            ..base_query(0)
        };
        let hits = query_graph(&s, 0, &q).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.node_id).collect::<Vec<_>>(),
            vec![1],
            "only Bob (depth 1, kind User) — node 4 must be unreachable \
             through the filtered-out Acme node"
        );
    }

    // 8. namespace isolation
    #[test]
    fn start_node_in_different_namespace_returns_none() {
        let mut s = KernelState::new();
        s.apply_event_ns(
            &KernelEvent::CreateNode {
                id: NodeId(0),
                kind: NodeKind::Concept,
                record: None,
            },
            7, // node actually lives in namespace 7
        )
        .unwrap();

        // Querying it as if it were in namespace 0 must behave exactly like
        // "does not exist" — never leak cross-tenant existence.
        let result_wrong_ns = query_graph(&s, 0, &base_query(0));
        assert!(result_wrong_ns.is_none());

        // The correct namespace must still work.
        let result_right_ns = query_graph(&s, 7, &base_query(0));
        assert!(result_right_ns.is_some());
    }

    #[test]
    fn traversal_cannot_cross_namespaces_even_when_attempted() {
        // Two nodes in different namespaces; an edge between them is
        // rejected at the canonical layer (G0's namespace invariant), so
        // there is no way to even construct a graph that would let
        // query_graph's traversal leak across namespaces. This test proves
        // the setup is impossible, which is itself the strongest form of
        // this guarantee — reusable for the review doc as citable evidence.
        let mut s = KernelState::new();
        s.apply_event_ns(
            &KernelEvent::CreateNode {
                id: NodeId(0),
                kind: NodeKind::Concept,
                record: None,
            },
            0,
        )
        .unwrap();
        s.apply_event_ns(
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
            from: NodeId(0),
            to: NodeId(1),
            kind: EdgeKind::Relation,
        };
        assert!(
            s.apply_event_ns(&cross_ns_edge, 0).is_err(),
            "cross-namespace edge must be rejected at the canonical layer"
        );
    }

    // 9. bounded depth — Alice -> Bob -> Charlie -> Dave chain.
    fn chain_graph() -> KernelState {
        let mut s = KernelState::new();
        for i in 0u32..4 {
            s.apply_event(&KernelEvent::CreateNode {
                id: NodeId(i),
                kind: NodeKind::Concept,
                record: None,
            })
            .unwrap();
        }
        for i in 0u32..3 {
            s.apply_event(&KernelEvent::CreateEdge {
                id: EdgeId(i),
                from: NodeId(i),
                to: NodeId(i + 1),
                kind: EdgeKind::Follows,
            })
            .unwrap();
        }
        s
    }

    #[test]
    fn bounded_depth_returns_exactly_the_expected_prefix() {
        let s = chain_graph();
        let ids = |depth: u32| -> Vec<u32> {
            let q = GraphQuery {
                max_depth: depth,
                ..base_query(0)
            };
            query_graph(&s, 0, &q)
                .unwrap()
                .into_iter()
                .map(|h| h.node_id)
                .collect()
        };
        assert_eq!(ids(1), vec![1], "depth 1 -> Bob only");
        assert_eq!(ids(2), vec![1, 2], "depth 2 -> Bob, Charlie");
        assert_eq!(ids(3), vec![1, 2, 3], "depth 3 -> Bob, Charlie, Dave");
        assert!(
            !ids(3).contains(&0),
            "the start node (Alice) must never be included in results"
        );
    }

    // 10. cycle handling — Alice -> Bob -> Charlie -> Alice must terminate.
    #[test]
    fn cycle_does_not_cause_infinite_traversal() {
        let mut s = KernelState::new();
        for i in 0u32..3 {
            s.apply_event(&KernelEvent::CreateNode {
                id: NodeId(i),
                kind: NodeKind::Concept,
                record: None,
            })
            .unwrap();
        }
        // 0 -> 1 -> 2 -> 0 (cycle back to start).
        for (i, (from, to)) in [(0u32, 1u32), (1, 2), (2, 0)].into_iter().enumerate() {
            s.apply_event(&KernelEvent::CreateEdge {
                id: EdgeId(i as u32),
                from: NodeId(from),
                to: NodeId(to),
                kind: EdgeKind::Relation,
            })
            .unwrap();
        }

        let q = GraphQuery {
            max_depth: MAX_DEPTH, // the largest allowed depth — must still terminate
            ..base_query(0)
        };
        let hits = query_graph(&s, 0, &q).unwrap();
        let ids: Vec<u32> = hits.iter().map(|h| h.node_id).collect();
        // Node 0 (the start) is excluded even though the cycle revisits it.
        assert_eq!(
            ids,
            vec![1, 2],
            "the cycle must not reappear the start node"
        );
    }

    // 11. duplicate-edge behavior — two parallel A->B edges must still
    //     report B exactly once (G0.1 established duplicates are allowed as
    //     canonical edges, but a query result reports distinct NODES).
    #[test]
    fn duplicate_edges_report_the_target_node_once() {
        let mut s = KernelState::new();
        for i in 0u32..2 {
            s.apply_event(&KernelEvent::CreateNode {
                id: NodeId(i),
                kind: NodeKind::Concept,
                record: None,
            })
            .unwrap();
        }
        for i in 0u32..3 {
            s.apply_event(&KernelEvent::CreateEdge {
                id: EdgeId(i),
                from: NodeId(0),
                to: NodeId(1),
                kind: EdgeKind::Relation,
            })
            .unwrap();
        }
        let hits = query_graph(&s, 0, &base_query(0)).unwrap();
        assert_eq!(hits.len(), 1, "3 parallel edges must still yield 1 hit");
        assert_eq!(hits[0].node_id, 1);
    }

    // 12. self-loop behavior
    #[test]
    fn self_loop_on_start_node_produces_no_hit() {
        let mut s = KernelState::new();
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(0),
            kind: NodeKind::Concept,
            record: None,
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(0),
            from: NodeId(0),
            to: NodeId(0),
            kind: EdgeKind::Relation,
        })
        .unwrap();
        let hits = query_graph(&s, 0, &base_query(0)).unwrap();
        assert!(
            hits.is_empty(),
            "a self-loop on the start node must not cause the start node \
             to appear in its own results"
        );
    }

    #[test]
    fn self_loop_on_a_reached_node_does_not_hang_or_duplicate() {
        let mut s = KernelState::new();
        for i in 0u32..2 {
            s.apply_event(&KernelEvent::CreateNode {
                id: NodeId(i),
                kind: NodeKind::Concept,
                record: None,
            })
            .unwrap();
        }
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(0),
            from: NodeId(0),
            to: NodeId(1),
            kind: EdgeKind::Relation,
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(1),
            from: NodeId(1),
            to: NodeId(1), // self-loop on the reached node
            kind: EdgeKind::Relation,
        })
        .unwrap();

        let q = GraphQuery {
            max_depth: MAX_DEPTH,
            ..base_query(0)
        };
        let hits = query_graph(&s, 0, &q).unwrap();
        assert_eq!(hits.len(), 1, "node 1 must be reported exactly once");
        assert_eq!(hits[0].node_id, 1);
    }

    // 13/14. deterministic ordering + repeated-query determinism
    #[test]
    fn ordering_is_depth_then_node_id_ascending() {
        // A fan-out where insertion order does NOT match id order, to prove
        // the sort is real and not an accident of BFS visitation order.
        let mut s = KernelState::new();
        for i in 0u32..4 {
            s.apply_event(&KernelEvent::CreateNode {
                id: NodeId(i),
                kind: NodeKind::Concept,
                record: None,
            })
            .unwrap();
        }
        // Create edges to node 3 first, then 1, then 2 — reverse-ish order —
        // so raw BFS/adjacency order would NOT already be ascending.
        for (i, to) in [3u32, 1, 2].into_iter().enumerate() {
            s.apply_event(&KernelEvent::CreateEdge {
                id: EdgeId(i as u32),
                from: NodeId(0),
                to: NodeId(to),
                kind: EdgeKind::Relation,
            })
            .unwrap();
        }
        let hits = query_graph(&s, 0, &base_query(0)).unwrap();
        let ids: Vec<u32> = hits.iter().map(|h| h.node_id).collect();
        assert_eq!(ids, vec![1, 2, 3], "must be sorted ascending by node_id");
    }

    #[test]
    fn repeated_identical_query_is_deterministic() {
        let s = alice_graph();
        let q = base_query(0);
        let r1 = query_graph(&s, 0, &q).unwrap();
        let r2 = query_graph(&s, 0, &q).unwrap();
        let r3 = query_graph(&s, 0, &q).unwrap();
        assert_eq!(r1, r2);
        assert_eq!(r2, r3);
    }

    // 15. same canonical graph built through replay returns identical result
    #[test]
    fn replayed_graph_returns_identical_query_result() {
        let s1 = alice_graph();
        let s2 = alice_graph(); // independent rebuild, identical event sequence
        let q = base_query(0);
        assert_eq!(
            query_graph(&s1, 0, &q).unwrap(),
            query_graph(&s2, 0, &q).unwrap(),
            "replaying the same events must produce the same query result"
        );
    }

    // 16. snapshot -> restore -> query produces identical result. This is
    // the "critical test" from G1.1's brief: S -> query -> R1; snapshot(S)
    // -> restore -> query -> R2; assert R1 == R2.
    #[test]
    fn snapshot_restore_produces_identical_query_result() {
        use valori_kernel::snapshot::decode::decode_state;
        use valori_kernel::snapshot::encode::{encode_capacity_hint, encode_state};

        let s = alice_graph();
        let q = base_query(0);
        let r1 = query_graph(&s, 0, &q).unwrap();

        let mut buf = Vec::with_capacity(encode_capacity_hint(&s));
        encode_state(&s, &mut buf).unwrap();
        let restored = decode_state(&buf).unwrap();
        let r2 = query_graph(&restored, 0, &q).unwrap();

        assert_eq!(r1, r2, "snapshot round-trip must not change query results");
    }

    // Extra: an unrelated node in a different namespace must not perturb
    // results — directly testing the "namespace-blind index" risk class
    // (property-testing item from Part 12 of the brief, expressed as a
    // concrete example since no property-testing framework is in use here
    // — see the G1.1 review doc, Part 12).
    #[test]
    fn unrelated_node_in_another_namespace_does_not_change_results() {
        let s1 = alice_graph();
        let q = base_query(0);
        let r1 = query_graph(&s1, 0, &q).unwrap();

        let mut s2 = alice_graph();
        s2.apply_event_ns(
            &KernelEvent::CreateNode {
                id: NodeId(4),
                kind: NodeKind::Concept,
                record: None,
            },
            9,
        )
        .unwrap();
        let r2 = query_graph(&s2, 0, &q).unwrap();

        assert_eq!(
            r1, r2,
            "an unrelated node in a different namespace must not affect the result"
        );
    }

    // Result-limit truncation: closest-by-(depth,id) survive, not an
    // arbitrary BFS-order-dependent subset.
    #[test]
    fn limit_keeps_the_closest_results_by_depth_then_id() {
        let s = chain_graph(); // 0 -> 1 -> 2 -> 3
        let q = GraphQuery {
            max_depth: 3,
            limit: 2,
            ..base_query(0)
        };
        let hits = query_graph(&s, 0, &q).unwrap();
        let ids: Vec<u32> = hits.iter().map(|h| h.node_id).collect();
        assert_eq!(
            ids,
            vec![1, 2],
            "limit=2 must keep the two closest (lowest depth, then id) results"
        );
    }

    #[test]
    fn depth_and_limit_are_clamped_not_rejected() {
        let s = chain_graph();
        let q = GraphQuery {
            max_depth: MAX_DEPTH + 100, // wildly over — must clamp, not error
            limit: MAX_QUERY_LIMIT + 100,
            ..base_query(0)
        };
        // Must not panic and must behave as if clamped to MAX_DEPTH.
        let hits = query_graph(&s, 0, &q).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.node_id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    // Record linkage is preserved in results, per Part 3's requirement that
    // lookup expose the linked record id using the existing data model.
    #[test]
    fn hits_report_linked_record_id_when_present() {
        let mut s = KernelState::new();
        s.apply_event(&KernelEvent::InsertRecord {
            id: RecordId(0),
            vector: valori_kernel::types::vector::FxpVector::new_zeros(4),
            metadata: None,
            tag: 0,
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(0),
            kind: NodeKind::Concept,
            record: None,
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateNode {
            id: NodeId(1),
            kind: NodeKind::Chunk,
            record: Some(RecordId(0)),
        })
        .unwrap();
        s.apply_event(&KernelEvent::CreateEdge {
            id: EdgeId(0),
            from: NodeId(0),
            to: NodeId(1),
            kind: EdgeKind::Relation,
        })
        .unwrap();
        let hits = query_graph(&s, 0, &base_query(0)).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record_id, Some(0));
    }

    // ── G1.1 Part 15 — performance baseline ────────────────────────────────────
    //
    // No criterion/bench harness exists in this workspace yet (the existing
    // `crates/valori-cli/src/bin/bench_*.rs` binaries and the fixture
    // generators elsewhere in this codebase are both plain
    // print-and-inspect, not a criterion setup) — reusing that convention
    // rather than introducing a new dependency for one small baseline.
    // `cargo test -p valori-rag --lib graph::tests::query_graph_baseline -- \
    //   --ignored --nocapture`
    #[test]
    #[ignore]
    fn query_graph_baseline() {
        use std::time::Instant;

        fn time_it<F: FnMut()>(label: &str, iters: u32, mut f: F) {
            let start = Instant::now();
            for _ in 0..iters {
                f();
            }
            let elapsed = start.elapsed();
            println!(
                "{label}: {iters} iters, {:?} total, {:?}/iter",
                elapsed,
                elapsed / iters
            );
        }

        // A small chain for direct-neighbor / depth-2 / depth-3.
        let chain = chain_graph();
        time_it("direct neighbor lookup (depth=1)", 10_000, || {
            let q = GraphQuery {
                max_depth: 1,
                ..base_query(0)
            };
            std::hint::black_box(query_graph(&chain, 0, &q));
        });
        time_it("depth-2 traversal", 10_000, || {
            let q = GraphQuery {
                max_depth: 2,
                ..base_query(0)
            };
            std::hint::black_box(query_graph(&chain, 0, &q));
        });
        time_it("depth-3 traversal", 10_000, || {
            let q = GraphQuery {
                max_depth: 3,
                ..base_query(0)
            };
            std::hint::black_box(query_graph(&chain, 0, &q));
        });

        // Filtered traversal (Alice/Bob/Acme fixture, single edge kind).
        let alice = alice_graph();
        time_it("filtered traversal (edge_kind)", 10_000, || {
            let q = GraphQuery {
                edge_kind: Some(EdgeKind::Follows),
                max_depth: 1,
                ..base_query(0)
            };
            std::hint::black_box(query_graph(&alice, 0, &q));
        });

        // A graph with cycles (fan + back-edges), still small.
        let mut cyclic = KernelState::new();
        for i in 0u32..50 {
            cyclic
                .apply_event(&KernelEvent::CreateNode {
                    id: NodeId(i),
                    kind: NodeKind::Concept,
                    record: None,
                })
                .unwrap();
        }
        let mut eid = 0u32;
        for i in 0u32..49 {
            cyclic
                .apply_event(&KernelEvent::CreateEdge {
                    id: EdgeId(eid),
                    from: NodeId(i),
                    to: NodeId(i + 1),
                    kind: EdgeKind::Relation,
                })
                .unwrap();
            eid += 1;
            // A back-edge every few nodes to force cycles.
            if i % 5 == 0 && i > 0 {
                cyclic
                    .apply_event(&KernelEvent::CreateEdge {
                        id: EdgeId(eid),
                        from: NodeId(i),
                        to: NodeId(i - 5),
                        kind: EdgeKind::Relation,
                    })
                    .unwrap();
                eid += 1;
            }
        }
        time_it(
            "traversal on a graph with cycles (50 nodes, depth=4)",
            1_000,
            || {
                let q = GraphQuery {
                    max_depth: MAX_DEPTH,
                    ..base_query(0)
                };
                std::hint::black_box(query_graph(&cyclic, 0, &q));
            },
        );

        // A "larger" (still modest — 1,000 nodes) graph: a fan-out tree,
        // branching factor 3, so depth 4 touches a meaningful fraction of it.
        // Deliberately NOT an enormous graph — this phase establishes a
        // baseline, not a stress test (G1.1 Part 15's explicit instruction).
        let mut larger = KernelState::new();
        for i in 0u32..1000 {
            larger
                .apply_event(&KernelEvent::CreateNode {
                    id: NodeId(i),
                    kind: NodeKind::Concept,
                    record: None,
                })
                .unwrap();
        }
        let mut eid = 0u32;
        for i in 0u32..1000 {
            for child_offset in 1..=3u32 {
                let child = i * 3 + child_offset;
                if child < 1000 {
                    larger
                        .apply_event(&KernelEvent::CreateEdge {
                            id: EdgeId(eid),
                            from: NodeId(i),
                            to: NodeId(child),
                            kind: EdgeKind::Relation,
                        })
                        .unwrap();
                    eid += 1;
                }
            }
        }
        time_it(
            "traversal on a 1,000-node fan-out graph (depth=4)",
            100,
            || {
                let q = GraphQuery {
                    max_depth: MAX_DEPTH,
                    limit: MAX_QUERY_LIMIT,
                    ..base_query(0)
                };
                std::hint::black_box(query_graph(&larger, 0, &q));
            },
        );
    }

    // ── G1.3 — seed-resolution cost: O(live_nodes) scan per graphrag call ─────
    //
    // `resolve_seed_nodes` derives record->node from canonical state instead
    // of a cache, which is what makes it stateless/parity-safe — but it costs
    // one node-pool scan per call. This measures that cost directly so the
    // tradeoff is recorded, not assumed.
    //
    // `cargo test -p valori-rag --release --lib graph::tests::resolve_seed_nodes_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn resolve_seed_nodes_cost() {
        use std::time::Instant;
        use valori_kernel::types::vector::FxpVector;
        const D: usize = 4;

        for &n in &[1_000u32, 10_000, 100_000] {
            // Every 10th node carries a record, mirroring a realistic mix of
            // record-backed (Chunk) and structural (Concept) nodes.
            let mut s = KernelState::new();
            let mut record_ids = Vec::new();
            for i in 0..n {
                let rec = if i % 10 == 0 {
                    let rid = RecordId(s.next_record_id().0);
                    s.apply_event(&KernelEvent::InsertRecord {
                        id: rid,
                        vector: FxpVector::new_zeros(D),
                        metadata: None,
                        tag: 0,
                    })
                    .unwrap();
                    record_ids.push(rid.0);
                    Some(rid)
                } else {
                    None
                };
                s.apply_event(&KernelEvent::CreateNode {
                    id: NodeId(i),
                    kind: NodeKind::Chunk,
                    record: rec,
                })
                .unwrap();
            }

            // A typical graphrag k=10 lookup.
            let probe: Vec<u32> = record_ids.iter().copied().take(10).collect();
            let iters = 200u32;
            let start = Instant::now();
            for _ in 0..iters {
                std::hint::black_box(resolve_seed_nodes(&s, &probe));
            }
            let elapsed = start.elapsed();
            println!(
                "resolve_seed_nodes: {} live nodes, k=10 -> {:?}/call",
                n,
                elapsed / iters
            );
        }
    }

    // ── G1.2 — scale benchmark: sizes × shapes × depths × filters × direction ──
    //
    // `cargo test -p valori-rag --release --lib graph::tests::query_graph_g1_2_scale -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn query_graph_g1_2_scale() {
        use std::time::Instant;

        fn build_chain(n: u32) -> KernelState {
            let mut s = KernelState::new();
            for i in 0..n {
                s.apply_event(&KernelEvent::CreateNode {
                    id: NodeId(i),
                    kind: NodeKind::Concept,
                    record: None,
                })
                .unwrap();
            }
            for i in 0..n.saturating_sub(1) {
                s.apply_event(&KernelEvent::CreateEdge {
                    id: EdgeId(i),
                    from: NodeId(i),
                    to: NodeId(i + 1),
                    kind: EdgeKind::Relation,
                })
                .unwrap();
            }
            s
        }

        /// Medium-degree fan-out tree, branching factor `b`.
        fn build_fanout(n: u32, b: u32) -> KernelState {
            let mut s = KernelState::new();
            for i in 0..n {
                s.apply_event(&KernelEvent::CreateNode {
                    id: NodeId(i),
                    kind: NodeKind::Concept,
                    record: None,
                })
                .unwrap();
            }
            let mut eid = 0u32;
            for i in 0..n {
                for c in 1..=b {
                    let child = i * b + c;
                    if child < n {
                        s.apply_event(&KernelEvent::CreateEdge {
                            id: EdgeId(eid),
                            from: NodeId(i),
                            to: NodeId(child),
                            kind: EdgeKind::Relation,
                        })
                        .unwrap();
                        eid += 1;
                    }
                }
            }
            s
        }

        /// One hub with `n-1` direct spokes — the high-degree, single-hop
        /// stress case: does walking one huge intrusive adjacency list cost
        /// meaningfully more than N independent O(1) lookups would?
        fn build_hub_spoke(n: u32) -> KernelState {
            let mut s = KernelState::new();
            for i in 0..n {
                s.apply_event(&KernelEvent::CreateNode {
                    id: NodeId(i),
                    kind: NodeKind::Concept,
                    record: None,
                })
                .unwrap();
            }
            for i in 1..n {
                s.apply_event(&KernelEvent::CreateEdge {
                    id: EdgeId(i - 1),
                    from: NodeId(0),
                    to: NodeId(i),
                    kind: EdgeKind::Relation,
                })
                .unwrap();
            }
            s
        }

        /// Fan-out tree (b=3) plus a back-edge every 7 nodes, forcing the
        /// BFS visited-set dedup path to actually do work.
        fn build_cyclic(n: u32) -> KernelState {
            let mut s = build_fanout(n, 3);
            let mut eid = s.edge_count() as u32;
            for i in (7..n).step_by(7) {
                let _ = s.apply_event(&KernelEvent::CreateEdge {
                    id: EdgeId(eid),
                    from: NodeId(i),
                    to: NodeId(i - 7),
                    kind: EdgeKind::Relation,
                });
                eid += 1;
            }
            s
        }

        fn time_it(label: &str, iters: u32, mut f: impl FnMut() -> usize) -> usize {
            let start = Instant::now();
            let mut last_n = 0;
            for _ in 0..iters {
                last_n = f();
            }
            let elapsed = start.elapsed();
            println!(
                "{label}: {iters} iters, {:?}/iter, {} results",
                elapsed / iters,
                last_n
            );
            last_n
        }

        let run = |state: &KernelState, depth: u32, dir: Direction, ek: Option<EdgeKind>| {
            let q = GraphQuery {
                start: NodeId(0),
                direction: dir,
                edge_kind: ek,
                node_kind: None,
                max_depth: depth,
                limit: MAX_QUERY_LIMIT,
            };
            query_graph(state, 0, &q).unwrap().len()
        };

        for &n in &[1_000u32, 10_000, 100_000] {
            println!("\n=== N = {n} ===");

            let chain = build_chain(n);
            for depth in [1u32, 2, 3] {
                // depth is capped at MAX_DEPTH=4 by query_graph itself.
                time_it(&format!("chain(n={n}) depth={depth} outgoing"), 200, || {
                    run(&chain, depth, Direction::Outgoing, None)
                });
            }
            time_it(&format!("chain(n={n}) depth=4 incoming"), 200, || {
                run(&chain, 4, Direction::Incoming, None)
            });

            let fanout = build_fanout(n, 3);
            time_it(&format!("fanout(b=3,n={n}) depth=4 outgoing"), 200, || {
                run(&fanout, 4, Direction::Outgoing, None)
            });
            time_it(
                &format!("fanout(b=3,n={n}) depth=4 outgoing edge_kind filter"),
                200,
                || run(&fanout, 4, Direction::Outgoing, Some(EdgeKind::Relation)),
            );
            time_it(&format!("fanout(b=3,n={n}) depth=4 both"), 200, || {
                run(&fanout, 4, Direction::Both, None)
            });

            let hub = build_hub_spoke(n);
            time_it(
                &format!(
                    "hub_spoke(n={n}) depth=1 outgoing (walks {} direct edges)",
                    n - 1
                ),
                50,
                || run(&hub, 1, Direction::Outgoing, None),
            );

            let cyclic = build_cyclic(n);
            time_it(&format!("cyclic(n={n}) depth=4 outgoing"), 200, || {
                run(&cyclic, 4, Direction::Outgoing, None)
            });
        }
    }
}
