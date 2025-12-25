// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Event Log as Primary Truth
//!
//! This module defines the canonical event representation for Valori.
//! Every state transition must be expressed as a `KernelEvent`.
//!
//! # Determinism Guarantees
//! - No timestamps
//! - No randomness
//! - No implicit metadata
//! - No side-effect derived state
//! - No optional ordering dependence
//!
//! # Invariants
//! - Same event log => Same final state (on any architecture)
//! - Events are immutable once committed
//! - Replay must be deterministic and reproducible

use crate::types::id::{RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
use crate::types::enums::{NodeKind, EdgeKind};
use serde::{Serialize, Deserialize};

/// KernelEvent represents the canonical event language for Valori.
/// This is the ONLY way to express state transitions.
///
/// Each variant represents an atomic, deterministic operation on the kernel state.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum KernelEvent<const D: usize> {
    /// Insert a new vector record into the kernel
    InsertRecord {
        id: RecordId,
        vector: FxpVector<D>,
    },

    /// Delete an existing vector record from the kernel
    DeleteRecord {
        id: RecordId,
    },

    /// Create a new graph node
    CreateNode {
        id: NodeId,
        kind: NodeKind,
        record: Option<RecordId>,
    },

    /// Create a new graph edge
    CreateEdge {
        id: EdgeId,
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
    },

    /// Delete an existing graph edge
    DeleteEdge {
        id: EdgeId,
    },
}

impl<const D: usize> KernelEvent<D> {
    /// Returns a human-readable description of the event type
    pub fn event_type(&self) -> &'static str {
        match self {
            KernelEvent::InsertRecord { .. } => "InsertRecord",
            KernelEvent::DeleteRecord { .. } => "DeleteRecord",
            KernelEvent::CreateNode { .. } => "CreateNode",
            KernelEvent::CreateEdge { .. } => "CreateEdge",
            KernelEvent::DeleteEdge { .. } => "DeleteEdge",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization_determinism() {
        // Verify that serialization is deterministic
        let event = KernelEvent::<16>::InsertRecord {
            id: RecordId(42),
            vector: FxpVector::new_zeros(),
        };

        let bytes1 = bincode::serde::encode_to_vec(&event, bincode::config::standard()).unwrap();
        let bytes2 = bincode::serde::encode_to_vec(&event, bincode::config::standard()).unwrap();

        assert_eq!(bytes1, bytes2, "Event serialization must be deterministic");
    }

    #[test]
    fn test_event_roundtrip() {
        // Verify serialize/deserialize roundtrip
        let original = KernelEvent::<16>::CreateNode {
            id: NodeId(1),
            kind: NodeKind::Document,
            record: Some(RecordId(42)),
        };

        let bytes = bincode::serde::encode_to_vec(&original, bincode::config::standard()).unwrap();
        let (decoded, _): (KernelEvent<16>, _) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();

        assert_eq!(original, decoded, "Event must survive serialization roundtrip");
    }
}
