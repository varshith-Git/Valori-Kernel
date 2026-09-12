//! GraphRAG reachability over one namespace-scoped snapshot.
//!
//! Follow semantic edges forward and `ParentOf` edges in either direction so
//! a chunk can reach its document and siblings. Distances come from the same
//! budgeted walk as the returned subgraph; there is no second unbounded BFS.

use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::{json, Value};
use valori_kernel::{
    state::kernel::KernelState,
    types::{enums::EdgeKind, id::NodeId},
};

use crate::graph::MAX_DEPTH;

/// All live graph nodes referencing the vector hits, sorted by node ID.
/// A record may have several graph nodes, each with different relationships.
pub fn resolve_all_seed_nodes(
    state: &KernelState,
    namespace_id: u16,
    record_ids: &[u32],
) -> Vec<u32> {
    let records: HashSet<_> = record_ids.iter().copied().collect();
    let mut seeds: Vec<_> = state
        .iter_nodes()
        .filter(|n| n.namespace_id == namespace_id)
        .filter(|n| n.record.is_some_and(|r| records.contains(&r.0)))
        .map(|n| n.id.0)
        .collect();
    seeds.sort_unstable();
    seeds
}

#[derive(Debug, PartialEq)]
pub struct ReachableSubgraph {
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
    pub distances: HashMap<u32, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraversalPolicy {
    pub edge_kinds: Option<HashSet<u8>>,
    pub reverse_parent_of: bool,
}

impl Default for TraversalPolicy {
    fn default() -> Self {
        Self {
            edge_kinds: None,
            reverse_parent_of: true,
        }
    }
}

impl TraversalPolicy {
    pub fn from_edge_kinds(edge_kinds: Option<Vec<u8>>, reverse_parent_of: bool) -> Self {
        Self {
            edge_kinds: edge_kinds.map(|kinds| kinds.into_iter().collect()),
            reverse_parent_of,
        }
    }

    fn allows(&self, kind: EdgeKind) -> bool {
        self.edge_kinds
            .as_ref()
            .map_or(true, |kinds| kinds.contains(&(kind as u8)))
    }
}

/// Depth counts every traversed edge, including reverse `ParentOf` steps.
/// `max_nodes` bounds discovered nodes, including seeds. `max_edges` bounds
/// adjacency entries examined, including reverse/non-traversable entries.
/// `None` retains the caller's explicitly unbounded budget for that dimension.
/// Edges retain their stored orientation and both endpoints are always returned.
pub fn expand_retrieval_subgraph(
    state: &KernelState,
    namespace_id: u16,
    seeds: &[u32],
    depth: u32,
    max_nodes: Option<u32>,
    max_edges: Option<u32>,
) -> ReachableSubgraph {
    expand_retrieval_subgraph_with_policy(
        state,
        namespace_id,
        seeds,
        depth,
        max_nodes,
        max_edges,
        &TraversalPolicy::default(),
    )
}

pub fn expand_retrieval_subgraph_with_policy(
    state: &KernelState,
    namespace_id: u16,
    seeds: &[u32],
    depth: u32,
    max_nodes: Option<u32>,
    max_edges: Option<u32>,
    policy: &TraversalPolicy,
) -> ReachableSubgraph {
    let mut out = ReachableSubgraph {
        nodes: Vec::new(),
        edges: Vec::new(),
        distances: HashMap::new(),
    };
    let node_limit = max_nodes.map_or(usize::MAX, |n| n as usize);
    let edge_limit = max_edges.map_or(usize::MAX, |n| n as usize);
    let mut examined = 0usize;
    let mut emitted = HashSet::new();
    let mut queue = VecDeque::new();
    let mut sorted_seeds = seeds.to_vec();
    sorted_seeds.sort_unstable();
    sorted_seeds.dedup();
    for id in sorted_seeds {
        if out.nodes.len() >= node_limit {
            break;
        }
        if let Some(node) = state.get_node(NodeId(id)) {
            if node.namespace_id != namespace_id {
                continue;
            }
            out.nodes.push(json!({"id": id, "kind": node.kind as u8,
                "record": node.record.map(|r| r.0)}));
            out.distances.insert(id, 0);
            queue.push_back(id);
        }
    }

    while let Some(id) = queue.pop_front() {
        let distance = out.distances[&id];
        if distance >= depth.min(MAX_DEPTH) || examined >= edge_limit {
            continue;
        }
        let outgoing = state.outgoing_edges(NodeId(id)).into_iter().flatten();
        let incoming = state.incoming_edges(NodeId(id)).into_iter().flatten();
        for (edge, reverse) in outgoing
            .map(|e| (e, false))
            .chain(incoming.map(|e| (e, true)))
        {
            if examined >= edge_limit {
                break;
            }
            examined += 1;
            if reverse && edge.kind != EdgeKind::ParentOf {
                continue;
            }
            if reverse && !policy.reverse_parent_of {
                continue;
            }
            if !policy.allows(edge.kind) {
                continue;
            }
            let next = if reverse { edge.from } else { edge.to };
            let Some(node) = state.get_node(next) else {
                continue;
            };
            if node.namespace_id != namespace_id {
                continue;
            }
            if !out.distances.contains_key(&next.0) {
                if out.nodes.len() >= node_limit {
                    continue;
                }
                out.distances.insert(next.0, distance + 1);
                out.nodes.push(json!({"id": next.0, "kind": node.kind as u8,
                    "record": node.record.map(|r| r.0)}));
                queue.push_back(next.0);
            }
            if emitted.insert(edge.id.0) {
                out.edges.push(json!({"id": edge.id.0, "from": edge.from.0,
                    "to": edge.to.0, "kind": edge.kind as u8}));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_kernel::{
        event::KernelEvent,
        types::{enums::NodeKind, id::EdgeId},
    };

    fn family() -> KernelState {
        let mut state = KernelState::new();
        for id in 0..4 {
            state
                .apply_event(&KernelEvent::CreateNode {
                    id: NodeId(id),
                    kind: NodeKind::Concept,
                    record: None,
                })
                .unwrap();
        }
        for (id, from, to, kind) in [
            (0, 0, 1, EdgeKind::ParentOf),
            (1, 0, 2, EdgeKind::ParentOf),
            (2, 3, 1, EdgeKind::RefersTo),
        ] {
            state
                .apply_event(&KernelEvent::CreateEdge {
                    id: EdgeId(id),
                    from: NodeId(from),
                    to: NodeId(to),
                    kind,
                })
                .unwrap();
        }
        state
    }

    #[test]
    fn chunk_reaches_parent_and_sibling_without_reversing_semantic_edges() {
        let out = expand_retrieval_subgraph(&family(), 0, &[1], 2, None, None);
        assert_eq!(out.distances.get(&0), Some(&1));
        assert_eq!(out.distances.get(&2), Some(&2));
        assert!(!out.distances.contains_key(&3));
        assert_eq!(out.edges.len(), 2);
        assert!(out.edges.iter().all(|e| e["from"] == 0));
    }

    #[test]
    fn depth_zero_and_one_do_not_reach_siblings() {
        let state = family();
        assert_eq!(
            expand_retrieval_subgraph(&state, 0, &[1], 0, None, None)
                .nodes
                .len(),
            1
        );
        let out = expand_retrieval_subgraph(&state, 0, &[1], 1, None, None);
        assert_eq!(out.distances.get(&0), Some(&1));
        assert!(!out.distances.contains_key(&2));
    }

    #[test]
    fn budgets_bound_distances_and_keep_both_edge_endpoints() {
        let state = family();
        for nodes in 0..5 {
            for edges in 0..5 {
                let out = expand_retrieval_subgraph(&state, 0, &[1], 4, Some(nodes), Some(edges));
                assert!(out.nodes.len() <= nodes as usize);
                assert!(out.edges.len() <= edges as usize);
                assert_eq!(out.nodes.len(), out.distances.len());
                for edge in &out.edges {
                    for key in ["from", "to"] {
                        assert!(out
                            .distances
                            .contains_key(&(edge[key].as_u64().unwrap() as u32)));
                    }
                }
            }
        }
    }

    #[test]
    fn seeds_are_namespace_checked_and_order_independent() {
        let state = family();
        assert!(expand_retrieval_subgraph(&state, 1, &[1], 4, None, None)
            .nodes
            .is_empty());
        assert_eq!(
            expand_retrieval_subgraph(&state, 0, &[2, 1, 1], 4, None, None),
            expand_retrieval_subgraph(&state, 0, &[1, 2], 4, None, None),
        );
    }

    #[test]
    fn traversal_policy_filters_edge_kinds_and_reverse_parent_steps() {
        let state = family();

        let no_reverse = expand_retrieval_subgraph_with_policy(
            &state,
            0,
            &[1],
            2,
            None,
            None,
            &TraversalPolicy::from_edge_kinds(None, false),
        );
        assert!(
            !no_reverse.distances.contains_key(&0),
            "chunk should not climb to its parent when reverse_parent_of=false"
        );

        let parent_only = expand_retrieval_subgraph_with_policy(
            &state,
            0,
            &[1],
            2,
            None,
            None,
            &TraversalPolicy::from_edge_kinds(Some(vec![EdgeKind::ParentOf as u8]), true),
        );
        assert_eq!(parent_only.distances.get(&0), Some(&1));
        assert_eq!(parent_only.distances.get(&2), Some(&2));
        assert!(
            !parent_only.distances.contains_key(&3),
            "RefersTo edges should be excluded by the ParentOf allowlist"
        );

        let refers_only = expand_retrieval_subgraph_with_policy(
            &state,
            0,
            &[3],
            1,
            None,
            None,
            &TraversalPolicy::from_edge_kinds(Some(vec![EdgeKind::RefersTo as u8]), true),
        );
        assert_eq!(refers_only.distances.get(&1), Some(&1));
        assert!(
            !refers_only.distances.contains_key(&0),
            "ParentOf should be excluded by the RefersTo allowlist"
        );
    }
}
