//! Graph Edge definition.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::id::{NodeId, EdgeId};
use crate::types::enums::EdgeKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphEdge {
    pub id: EdgeId,
    pub kind: EdgeKind,
    pub from: NodeId,
    pub to: NodeId,
    pub next_out: Option<EdgeId>,
}

impl GraphEdge {
    pub fn new(id: EdgeId, kind: EdgeKind, from: NodeId, to: NodeId) -> Self {
        Self {
            id,
            kind,
            from,
            to,
            next_out: None,
        }
    }
}
