// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Kernel Command enum.definitions.

use crate::types::id::{RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
use crate::types::enums::{NodeKind, EdgeKind};

use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    InsertRecord {
        id: RecordId,
        vector: FxpVector,
        metadata: Option<alloc::vec::Vec<u8>>,
        tag: u64,
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
    SoftDeleteRecord {
        id: RecordId,
    },
    // -------------------------------------------------------------------------
    // RESERVED — Phase 3 (crypto-shredding + security model).
    // These variants DO NOT exist yet. They are documented here so that:
    //  (a) the Command enum evolution is planned before production logs exist,
    //  (b) implementors know exactly where the variants slot in.
    //
    // See docs/phases/phase-1.5-crypto-shredding.md and
    //     docs/phases/phase-1.6-security-model.md for the full design.
    //
    // Phase 3 additions (append after SoftDeleteRecord — append-only policy):
    //
    //  InsertEncryptedRecord {
    //      id:         RecordId,
    //      vector:     FxpVector,   // derived from plaintext before encryption
    //      key_id:     [u8; 16],    // opaque DEK identifier in Key Vault
    //      nonce:      [u8; 12],    // AES-256-GCM 96-bit nonce
    //      ciphertext: Vec<u8>,     // AES-256-GCM encrypted metadata
    //      tag:        [u8; 16],    // AEAD authentication tag
    //      collection: Option<String>,
    //  }
    //
    //  EraseRecord {
    //      id:         RecordId,    // sets FLAG_SHREDDED; destroys DEK via KeyVaultTrait
    //      erased_by:  [u8; 16],    // admin key hash for audit attribution
    //  }
    // -------------------------------------------------------------------------
}
