// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::graph::pool::{NodePool, EdgePool};
use crate::graph::node::GraphNode;
use crate::types::enums::{NodeKind, EdgeKind};
use crate::types::id::{NodeId, RecordId};
use crate::graph::adjacency::{add_edge, OutEdgeIterator};
use std::vec::Vec; // For test collection

#[test]
fn test_graph_creation_adjacency() {
    const N: usize = 5;
    const E: usize = 5;
    let mut nodes = NodePool::<N>::new();
    let mut edges = EdgePool::<E>::new();

    // Create Node A (id 0)
    let n0 = GraphNode::new(NodeId(0), NodeKind::Concept, None);
    let id0 = nodes.insert(n0).unwrap();
    assert_eq!(id0, NodeId(0));

    // Create Node B (id 1)
    let n1 = GraphNode::new(NodeId(0), NodeKind::Concept, None);
    let id1 = nodes.insert(n1).unwrap();

    // Create Node C (id 2)
    let n2 = GraphNode::new(NodeId(0), NodeKind::Concept, None);
    let id2 = nodes.insert(n2).unwrap();

    // Add Edge A -> B
    let e1 = add_edge(&mut nodes, &mut edges, EdgeKind::Relation, id0, id1).unwrap();
    
    // Add Edge A -> C
    let e2 = add_edge(&mut nodes, &mut edges, EdgeKind::Relation, id0, id2).unwrap();

    // Check A's outgoing edges
    // List should be LIFO: e2 -> e1 -> None
    let node_a = nodes.get(id0).unwrap();
    let iter = OutEdgeIterator::new(&edges, node_a.first_out_edge);
    
    let visited: Vec<u32> = iter.map(|e| e.to.0).collect();
    // A points to C (id 2) and B (id 1).
    // add_edge updates head.
    // 1. A -> B. Head = e1.
    // 2. A -> C. Head = e2. e2.next = e1.
    // Order: C (2), B (1).
    assert_eq!(visited, vec![2, 1]);
}
