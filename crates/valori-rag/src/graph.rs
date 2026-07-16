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
use valori_kernel::types::id::NodeId;

/// Hard cap on traversal depth — mirrors the existing `/graph/subgraph` limit so
/// a hostile `depth` can't fan out the whole graph.
pub const MAX_DEPTH: u32 = 4;

/// Resolve `record_id → node_id` for a specific set of records by scanning the
/// node pool once.
///
/// The standalone `Engine` keeps an O(1) `record_to_node` map; the cluster data
/// plane has no `Engine`, only `KernelState`, so it resolves seeds straight from
/// the kernel here. First node wins per record (deterministic in iteration
/// order), which for the standard ingest path is the record's `Chunk` node.
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

/// Breadth-first expansion from one or more seed nodes, returning the visited
/// nodes and traversed edges as JSON. Nodes and edges are de-duplicated; a node
/// is emitted once even when reached from multiple seeds.
///
/// `depth` is clamped to [`MAX_DEPTH`]. The JSON shapes match the long-standing
/// `/graph/subgraph` response so existing clients keep working.
pub fn expand_subgraph(state: &KernelState, seeds: &[u32], depth: u32) -> (Vec<Value>, Vec<Value>) {
    let depth = depth.min(MAX_DEPTH);

    let mut visited_nodes: HashSet<u32> = HashSet::new();
    let mut visited_edges: HashSet<u32> = HashSet::new();
    let mut nodes_out: Vec<Value> = Vec::new();
    let mut edges_out: Vec<Value> = Vec::new();
    let mut queue: VecDeque<(u32, u32)> = VecDeque::new();

    for &seed in seeds {
        queue.push_back((seed, depth));
    }

    while let Some((nid, rem)) = queue.pop_front() {
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
}
