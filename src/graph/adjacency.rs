// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Adjacency List for graph traversal.

use crate::graph::pool::{NodePool, EdgePool};
use crate::graph::edge::GraphEdge;
use crate::types::id::{NodeId, EdgeId};
use crate::types::enums::EdgeKind;
use crate::error::{Result, KernelError};

/// Adds an edge to the graph, updating the adjacency list.
/// 
/// Returns the new EdgeId.
pub fn add_edge(
    nodes: &mut NodePool,
    edges: &mut EdgePool,
    kind: EdgeKind,
    from: NodeId,
    to: NodeId,
) -> Result<EdgeId> {
    // Verify nodes exist
    if nodes.get(from).is_none() || nodes.get(to).is_none() {
        return Err(KernelError::NotFound);
    }

    // Create edge (id will be assigned by pool)
    // We init next_out to None temporarily, but we'll link it.
    let mut edge = GraphEdge::new(EdgeId(0), kind, from, to);
    
    // 1. Get current head of outgoing list from 'from' node
    let head = nodes.get(from).unwrap().first_out_edge;
    
    // 2. Set new edge's next_out to current head
    edge.next_out = head;

    // 3. Insert edge into pool
    let edge_id = edges.insert(edge)?;

    // 4. Update head of 'from' node to point to new edge
    // We must get mutable access again (re-borrow check might be tricky if we hold ref, but insert uses pool self)
    // edges.insert consumed 'edge', returned id.
    // 'nodes' is disjoint from 'edges', so we can borrow nodes mutably.
    
    if let Some(node) = nodes.get_mut(from) {
        node.first_out_edge = Some(edge_id);
    } else {
        // Should not happen as we checked existence, but for safety:
        // If node disappeared (?), we should rollback edge?
        // In single threaded kernel, it won't disappear.
        return Err(KernelError::NotFound);
    }

    Ok(edge_id)
}

/// Iterator for outgoing edges of a node.
pub struct OutEdgeIterator<'a> {
    edges: &'a EdgePool,
    current: Option<EdgeId>,
}

impl<'a> OutEdgeIterator<'a> {
    pub fn new(edges: &'a EdgePool, start: Option<EdgeId>) -> Self {
        Self {
            edges,
            current: start,
        }
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
