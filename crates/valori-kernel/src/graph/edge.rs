//! Graph Edge definition.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::types::id::{NodeId, EdgeId};
use crate::types::enums::EdgeKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphEdge {
    pub id: EdgeId,
    pub kind: EdgeKind,
    pub from: NodeId,
    pub to: NodeId,
    /// Next edge in `from` node's **outgoing** linked list.
    pub next_out: Option<EdgeId>,
    /// Next edge in `to` node's **incoming** linked list (back-pointer).
    /// Enables O(degree) cascade-delete instead of O(E) full scan.
    pub next_in: Option<EdgeId>,
}

impl GraphEdge {
    pub fn new(id: EdgeId, kind: EdgeKind, from: NodeId, to: NodeId) -> Self {
        Self {
            id,
            kind,
            from,
            to,
            next_out: None,
            next_in: None,
        }
    }
}
