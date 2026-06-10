//! Graph Node definition.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::types::id::{NodeId, RecordId, EdgeId};
use crate::types::enums::NodeKind;

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
}

impl GraphNode {
    pub fn new(id: NodeId, kind: NodeKind, record: Option<RecordId>) -> Self {
        Self {
            id,
            kind,
            record,
            first_out_edge: None,
            first_in_edge: None,
        }
    }
}
