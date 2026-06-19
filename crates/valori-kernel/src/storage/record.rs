//! Record definition.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::types::id::{RecordId, NS_LIST_NIL};
use crate::types::vector::FxpVector;

/// Bit-flag: record has been soft-deleted (tombstone).
/// The slot is retained for crash-safe WAL replay but excluded from
/// all search, record_count, and state-hash operations.
pub const FLAG_SOFT_DELETED: u8 = 0x01;

/// Bit-flag: record payload is encrypted (crypto-shredding envelope).
/// When set, `metadata` contains a serialised `EncryptedPayload` struct
/// rather than raw bytes. `vector` was derived from the plaintext before
/// encryption and is stored unencrypted (vectors are derived, not PII).
///
/// **Phase 1 — reserved only.** No code reads or writes this flag yet.
/// Implementation: Phase 3 (docs/phases/phase-1.5-crypto-shredding.md).
pub const FLAG_ENCRYPTED: u8 = 0x02;

/// Bit-flag: the DEK (Data Encryption Key) for this record has been
/// destroyed. The ciphertext in `metadata` is permanently unrecoverable.
/// The record slot is retained so the hash-chain and graph adjacency lists
/// remain intact; the record is excluded from search and state-hash
/// identically to soft-deleted records.
///
/// **Phase 1 — reserved only.** No code reads or writes this flag yet.
/// Implementation: Phase 3 (docs/phases/phase-1.5-crypto-shredding.md).
pub const FLAG_SHREDDED: u8 = 0x04;

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub id: RecordId,
    pub vector: FxpVector,
    pub metadata: Option<alloc::vec::Vec<u8>>,
    pub tag: u64,
    pub flags: u8,
    /// Namespace this record belongs to (0 = default).
    pub namespace_id: u16,
    /// Next record in this namespace's intrusive linked list (NS_LIST_NIL = end).
    pub next_in_ns: u32,
    /// Previous record in this namespace's intrusive linked list (NS_LIST_NIL = head).
    pub prev_in_ns: u32,
}

impl Record {
    pub fn new(id: RecordId, vector: FxpVector, metadata: Option<alloc::vec::Vec<u8>>, tag: u64, namespace_id: u16) -> Self {
        Self {
            id,
            vector,
            metadata,
            tag,
            flags: 0,
            namespace_id,
            next_in_ns: NS_LIST_NIL,
            prev_in_ns: NS_LIST_NIL,
        }
    }

    /// Returns `true` when the record is live (not soft-deleted).
    #[inline]
    pub fn is_active(&self) -> bool {
        self.flags & FLAG_SOFT_DELETED == 0
    }
}
