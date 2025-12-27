// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::state::kernel::KernelState;
use crate::state::command::Command;
use crate::types::id::{RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
use crate::types::enums::{NodeKind, EdgeKind};

#[test]
fn test_kernel_ops() {
    const R: usize = 5;
    const D: usize = 2;
    const N: usize = 5;
    const E: usize = 5;
    
    let mut kernel = KernelState::<R, D, N, E>::new();

    // 1. Insert Record
    let vec0 = FxpVector::new_zeros();
    let cmd1 = Command::InsertRecord { id: RecordId(0), vector: vec0, metadata: None };
    kernel.apply(&cmd1).unwrap();
    
    assert!(kernel.get_record(RecordId(0)).is_some());

    // 2. Create Nodes
    let cmd_n0 = Command::CreateNode { 
        node_id: NodeId(0), 
        kind: NodeKind::Record, 
        record: Some(RecordId(0)) 
    };
    kernel.apply(&cmd_n0).unwrap();
    assert!(kernel.get_node(NodeId(0)).is_some());

    let cmd_n1 = Command::CreateNode { 
        node_id: NodeId(1), 
        kind: NodeKind::Concept, 
        record: None 
    };
    kernel.apply(&cmd_n1).unwrap();
    
    let cmd_n2 = Command::CreateNode { 
        node_id: NodeId(2), 
        kind: NodeKind::Concept, 
        record: None 
    };
    kernel.apply(&cmd_n2).unwrap();

    // 3. Create Edges
    // 0 -> 1
    let cmd_e0 = Command::CreateEdge { edge_id: EdgeId(0), kind: EdgeKind::Relation, from: NodeId(0), to: NodeId(1) };
    kernel.apply(&cmd_e0).unwrap();
    // 2 -> 0
    let cmd_e1 = Command::CreateEdge { edge_id: EdgeId(1), kind: EdgeKind::Relation, from: NodeId(2), to: NodeId(0) };
    kernel.apply(&cmd_e1).unwrap();
    
    assert!(kernel.edges.get(EdgeId(0)).is_some());
    assert!(kernel.edges.get(EdgeId(1)).is_some());

    // 4. Test delete edge directly works
    // (create dummy edge 1->2 first)
    let cmd_e2 = Command::CreateEdge { edge_id: EdgeId(2), kind: EdgeKind::Relation, from: NodeId(1), to: NodeId(2) };
    kernel.apply(&cmd_e2).unwrap();
    let cmd_del_e2 = Command::DeleteEdge { edge_id: EdgeId(2) };
    kernel.apply(&cmd_del_e2).unwrap();
    assert!(kernel.edges.get(EdgeId(2)).is_none());

    // 5. Test Cascading Delete Node
    // Delete Node 0. Should auto-remove:
    // - Edge 0 (from 0 to 1)
    // - Edge 1 (from 2 to 0)
    
    let cmd_del_n0 = Command::DeleteNode { node_id: NodeId(0) };
    kernel.apply(&cmd_del_n0).unwrap();
    
    assert!(kernel.get_node(NodeId(0)).is_none());
    assert!(kernel.edges.get(EdgeId(0)).is_none(), "Outgoing edge from deleted node should be removed");
    assert!(kernel.edges.get(EdgeId(1)).is_none(), "Incoming edge to deleted node should be removed");
    
    // Check Node 2's list updated (Edge 1 removed)
    // Node 2 had edge 1. Now it should be empty (assuming only edge 1 existed)
    let node2 = kernel.get_node(NodeId(2)).unwrap();
    assert_eq!(node2.first_out_edge, None);
}
