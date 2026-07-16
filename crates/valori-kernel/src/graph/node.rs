//! Graph Node definition.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::types::enums::NodeKind;
use crate::types::id::{EdgeId, NodeId, RecordId, NS_LIST_NIL};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub record: Option<RecordId>,
    /// Head of this node's **outgoing** edge linked list.
    pub first_out_edge: Option<EdgeId>,
    /// Head of this node's **incoming** edge linked list (back-pointer).
    /// Allows instant lookup of "who points at me?" without a full edge scan.
    pub first_in_edge: Option<EdgeId>,
    /// Namespace this node belongs to (0 = default).
    pub namespace_id: u16,
    /// Next node in this namespace's intrusive linked list (NS_LIST_NIL = end).
    pub next_in_ns: u32,
    /// Previous node in this namespace's intrusive linked list (NS_LIST_NIL = head).
    pub prev_in_ns: u32,
}

impl GraphNode {
    pub fn new(id: NodeId, kind: NodeKind, record: Option<RecordId>, namespace_id: u16) -> Self {
        Self {
            id,
            kind,
            record,
            first_out_edge: None,
            first_in_edge: None,
            namespace_id,
            next_in_ns: NS_LIST_NIL,
            prev_in_ns: NS_LIST_NIL,
        }
    }
}
