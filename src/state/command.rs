// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Kernel Command enum.definitions.

use crate::types::id::{RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
use crate::types::enums::{NodeKind, EdgeKind};

#[derive(Clone, Debug, PartialEq)]
pub enum Command<const D: usize> {
    InsertRecord {
        id: RecordId,
        vector: FxpVector<D>,
    },
    DeleteRecord {
        id: RecordId,
    },
    CreateNode {
        node_id: NodeId,
        kind: NodeKind,
        record: Option<RecordId>,
    },
    CreateEdge {
        edge_id: EdgeId,
        kind: EdgeKind,
        from: NodeId,
        to: NodeId,
    },
    DeleteNode {
        node_id: NodeId,
    },
    DeleteEdge {
        edge_id: EdgeId,
    },
}
