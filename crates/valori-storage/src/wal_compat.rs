// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Legacy WAL serialization type for v1 backward-compatibility reading.
//!
//! `LegacyWalCommand` mirrors the `Command` enum that was stored in v1 WAL
//! files (before Phase K2). It is used ONLY by `WalReader` to deserialize
//! v1-format files; new writes use the v2 format (`KernelEvent` + namespace).
//! Do not add new variants or use this type outside of `wal_reader`.

use serde::{Deserialize, Serialize};
use valori_kernel::event::KernelEvent;
use valori_kernel::types::enums::{EdgeKind, NodeKind};
use valori_kernel::types::id::{EdgeId, NodeId, RecordId};
use valori_kernel::types::vector::FxpVector;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum LegacyWalCommand {
    InsertRecord {
        namespace_id: u16,
        id: RecordId,
        vector: FxpVector,
        metadata: Option<Vec<u8>>,
        tag: u64,
    },
    DeleteRecord {
        id: RecordId,
    },
    CreateNode {
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
    CreateNamespace {
        namespace_id: u16,
    },
    DropNamespace {
        namespace_id: u16,
    },
    InsertRecordEncrypted {
        namespace_id: u16,
        id: RecordId,
        key_id: [u8; 16],
        ciphertext: Vec<u8>,
        tag: u64,
    },
    ShredKey {
        key_id: [u8; 16],
    },
}

/// Translate a v1 `LegacyWalCommand` into `(KernelEvent, namespace_id)`.
pub(crate) fn legacy_to_event(cmd: LegacyWalCommand) -> (KernelEvent, u16) {
    match cmd {
        LegacyWalCommand::InsertRecord {
            namespace_id,
            id,
            vector,
            metadata,
            tag,
        } => (
            KernelEvent::InsertRecord {
                id,
                vector,
                metadata,
                tag,
            },
            namespace_id,
        ),
        LegacyWalCommand::DeleteRecord { id } => (KernelEvent::DeleteRecord { id }, 0),
        LegacyWalCommand::SoftDeleteRecord { id } => (KernelEvent::SoftDeleteRecord { id }, 0),
        LegacyWalCommand::CreateNode {
            namespace_id,
            node_id,
            kind,
            record,
        } => (
            KernelEvent::CreateNode {
                id: node_id,
                kind,
                record,
            },
            namespace_id,
        ),
        LegacyWalCommand::CreateEdge {
            edge_id,
            kind,
            from,
            to,
        } => (
            KernelEvent::CreateEdge {
                id: edge_id,
                from,
                to,
                kind,
            },
            0,
        ),
        LegacyWalCommand::DeleteNode { node_id } => (KernelEvent::DeleteNode { id: node_id }, 0),
        LegacyWalCommand::DeleteEdge { edge_id } => (KernelEvent::DeleteEdge { id: edge_id }, 0),
        LegacyWalCommand::CreateNamespace { namespace_id } => (
            KernelEvent::AutoCreateNamespace {
                name: String::new(),
            },
            namespace_id,
        ),
        LegacyWalCommand::DropNamespace { namespace_id } => (
            KernelEvent::DropNamespace {
                name: String::new(),
            },
            namespace_id,
        ),
        LegacyWalCommand::InsertRecordEncrypted {
            namespace_id,
            id,
            key_id,
            ciphertext,
            tag,
        } => (
            KernelEvent::InsertRecordEncrypted {
                id,
                key_id,
                ciphertext,
                metadata_ciphertext: None,
                tag,
            },
            namespace_id,
        ),
        LegacyWalCommand::ShredKey { key_id } => (KernelEvent::ShredKey { key_id }, 0),
    }
}
