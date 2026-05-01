//! Record definition.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::id::RecordId;
use crate::types::vector::FxpVector;

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
}
