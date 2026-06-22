//! Record definition.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::types::id::{RecordId, NS_LIST_NIL};
use crate::types::vector::FxpVector;

/// Bit-flag: record has been soft-deleted (tombstone).
/// The slot is retained for crash-safe WAL replay but excluded from
/// all search, record_count, and state-hash operations.
pub const FLAG_SOFT_DELETED: u8 = 0x01;

/// Bit-flag: record payload is encrypted (Phase 3.6 crypto-shredding).
/// `metadata` holds the AES-256-GCM ciphertext of the original
/// `(vector_bytes ‖ metadata_bytes)` payload. The in-memory `vector` is
/// zeroed — this record does not participate in nearest-neighbour search.
/// It IS counted in state-hash and record slots (provable existence).
pub const FLAG_ENCRYPTED: u8 = 0x02;

/// Bit-flag: the DEK for this record has been destroyed (shredded).
/// Set by `KernelEvent::ShredKey`. The ciphertext in `metadata` remains
/// in the log (audit chain intact) but is permanently unrecoverable.
/// Excluded from search, record_count, and active iteration.
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

    /// Returns `true` when the record is live (not soft-deleted, not shredded).
    /// Encrypted records ARE considered active (they exist and contribute to
    /// the state hash), but are not searchable — use `is_searchable()` for search.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.flags & (FLAG_SOFT_DELETED | FLAG_SHREDDED) == 0
    }

    /// Returns `true` when the record can appear in nearest-neighbour search
    /// results. Encrypted records have a zero vector and must be excluded.
    #[inline]
    pub fn is_searchable(&self) -> bool {
        self.flags & (FLAG_SOFT_DELETED | FLAG_ENCRYPTED | FLAG_SHREDDED) == 0
    }
}
