//! Record definition.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::types::id::RecordId;
use crate::types::vector::FxpVector;

/// Bit-flag: record has been soft-deleted (tombstone).
/// The slot is retained for crash-safe WAL replay but excluded from
/// all search, record_count, and state-hash operations.
pub const FLAG_SOFT_DELETED: u8 = 0x01;

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub id: RecordId,
    pub vector: FxpVector,
    pub metadata: Option<alloc::vec::Vec<u8>>,
    pub tag: u64,
    pub flags: u8,
}

impl Record {
    pub fn new(id: RecordId, vector: FxpVector, metadata: Option<alloc::vec::Vec<u8>>, tag: u64) -> Self {
        Self {
            id,
            vector,
            metadata,
            tag,
            flags: 0,
        }
    }

    /// Returns `true` when the record is live (not soft-deleted).
    #[inline]
    pub fn is_active(&self) -> bool {
        self.flags & FLAG_SOFT_DELETED == 0
    }
}
