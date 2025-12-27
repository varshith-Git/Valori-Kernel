//! Record definition.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::types::id::RecordId;
use crate::types::vector::FxpVector;

#[derive(Clone, Debug, PartialEq)]
pub struct Record<const D: usize> {
    pub id: RecordId,
    pub vector: FxpVector<D>,
    pub metadata: Option<alloc::vec::Vec<u8>>,
    pub flags: u8,
}

impl<const D: usize> Record<D> {
    pub fn new(id: RecordId, vector: FxpVector<D>, metadata: Option<alloc::vec::Vec<u8>>) -> Self {
        Self {
            id,
            vector,
            metadata,
            flags: 0,
        }
    }
}
