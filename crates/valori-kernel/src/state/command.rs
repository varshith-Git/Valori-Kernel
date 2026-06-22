// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Kernel Command enum definitions.

use crate::types::id::{RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
use crate::types::enums::{NodeKind, EdgeKind};

use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    InsertRecord {
        /// Namespace this record belongs to (0 = default).
        namespace_id: u16,
        id: RecordId,
        vector: FxpVector,
        metadata: Option<alloc::vec::Vec<u8>>,
        tag: u64,
    },
    DeleteRecord {
        id: RecordId,
    },
    CreateNode {
        /// Namespace this node belongs to (0 = default).
        namespace_id: u16,
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
    SoftDeleteRecord {
        id: RecordId,
    },
    /// Register a new namespace. Idempotent — re-registering an existing namespace is a no-op.
    CreateNamespace {
        namespace_id: u16,
    },
    /// Drop a namespace and hard-delete every record and node it owns.
    /// The default namespace (0) cannot be dropped.
    DropNamespace {
        namespace_id: u16,
    },
    // Phase 3.6 — Crypto-shredding
    /// Insert a record whose payload is AES-256-GCM encrypted by the node vault.
    /// The kernel stores a zero vector (not searchable) and the raw ciphertext.
    InsertRecordEncrypted {
        namespace_id: u16,
        id: RecordId,
        key_id: [u8; 16],
        ciphertext: alloc::vec::Vec<u8>,
        tag: u64,
    },
    /// Destroy the DEK identified by `key_id` and mark all records encrypted
    /// under it as FLAG_SHREDDED. Irreversible.
    ShredKey {
        key_id: [u8; 16],
    },
}
