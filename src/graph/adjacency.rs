// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Adjacency List for graph traversal.

use crate::graph::pool::{NodePool, EdgePool};
use crate::graph::edge::GraphEdge;
use crate::types::id::{NodeId, EdgeId};
use crate::types::enums::EdgeKind;
use crate::error::{Result, KernelError};

/// Adds an edge to the graph, maintaining both outgoing and incoming linked lists.
///
/// Both `from.first_out_edge` and `to.first_in_edge` are updated so that
/// cascade-deletes are O(degree) rather than O(E).
///
/// Returns the new EdgeId.
pub fn add_edge(
    nodes: &mut NodePool,
    edges: &mut EdgePool,
    kind: EdgeKind,
    from: NodeId,
    to: NodeId,
) -> Result<EdgeId> {
    // Verify both endpoints exist
    if nodes.get(from).is_none() || nodes.get(to).is_none() {
        return Err(KernelError::NotFound);
    }

    // Snapshot current list heads — immutable borrows are dropped after these lines
    let head_out = nodes.get(from).and_then(|n| n.first_out_edge);
    let head_in  = nodes.get(to).and_then(|n| n.first_in_edge);

    // Build the new edge, prepending it to both lists
    let mut edge = GraphEdge::new(EdgeId(0), kind, from, to);
    edge.next_out = head_out;
    edge.next_in  = head_in;

    let edge_id = edges.insert(edge)?;

    // Update outgoing head on the source node
    nodes.get_mut(from).unwrap().first_out_edge = Some(edge_id);
    // Update incoming head on the destination node
    nodes.get_mut(to).unwrap().first_in_edge    = Some(edge_id);

    Ok(edge_id)
}

/// Iterator over outgoing edges of a node (follows `next_out` pointers).
pub struct OutEdgeIterator<'a> {
    edges: &'a EdgePool,
    current: Option<EdgeId>,
}

impl<'a> OutEdgeIterator<'a> {
    pub fn new(edges: &'a EdgePool, start: Option<EdgeId>) -> Self {
        Self { edges, current: start }
    }
}

impl<'a> Iterator for OutEdgeIterator<'a> {
    type Item = &'a GraphEdge;

    fn next(&mut self) -> Option<Self::Item> {
        let curr_id = self.current?;
        let edge = self.edges.get(curr_id)?;
        self.current = edge.next_out;
        Some(edge)
    }
}

/// Iterator over incoming edges of a node (follows `next_in` back-pointers).
pub struct InEdgeIterator<'a> {
    edges: &'a EdgePool,
    current: Option<EdgeId>,
}

impl<'a> InEdgeIterator<'a> {
    pub fn new(edges: &'a EdgePool, start: Option<EdgeId>) -> Self {
        Self { edges, current: start }
    }
}

impl<'a> Iterator for InEdgeIterator<'a> {
    type Item = &'a GraphEdge;

    fn next(&mut self) -> Option<Self::Item> {
        let curr_id = self.current?;
        let edge = self.edges.get(curr_id)?;
        self.current = edge.next_in;
        Some(edge)
    }
}
