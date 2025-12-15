//! Graph Node definition.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::id::{NodeId, RecordId, EdgeId};
use crate::types::enums::NodeKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub record: Option<RecordId>,
    pub first_out_edge: Option<EdgeId>,
}

impl GraphNode {
    pub fn new(id: NodeId, kind: NodeKind, record: Option<RecordId>) -> Self {
        Self {
            id,
            kind,
            record,
            first_out_edge: None,
        }
    }
}
