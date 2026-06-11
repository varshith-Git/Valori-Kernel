//! Static Record Pool.

// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use crate::storage::record::{Record, FLAG_SOFT_DELETED};
use crate::types::id::RecordId;
use crate::types::vector::FxpVector;
use crate::error::{Result, KernelError};

#[derive(Clone)]
pub struct RecordPool {
    pub(crate) records: alloc::vec::Vec<Option<Record>>,
}

impl RecordPool {
    pub(crate) fn raw_records(&self) -> &[Option<Record>] {
        &self.records
    }

    pub fn new() -> Self {
        Self {
            records: alloc::vec::Vec::new(),
        }
    }

    /// Inserts a vector into the pool (always appends to maintain monotonic IDs).
    /// Returns the RecordId (which corresponds to the index).
    pub fn insert(&mut self, vector: FxpVector, metadata: Option<alloc::vec::Vec<u8>>, tag: u64) -> Result<RecordId> {
        let id = RecordId(self.records.len() as u32);
        self.records.push(Some(Record::new(id, vector, metadata, tag)));
        Ok(id)
    }

    /// Deletes the record at the specified ID (index).
    pub fn delete(&mut self, id: RecordId) -> Result<()> {
        let idx = id.0 as usize;
        if idx >= self.records.len() {
            return Err(KernelError::NotFound); 
        }
        
        if self.records[idx].is_some() {
            self.records[idx] = None;
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    }

    /// Gets a reference to the record.
    pub fn get(&self, id: RecordId) -> Option<&Record> {
        let idx = id.0 as usize;
        if idx >= self.records.len() {
            return None;
        }
        self.records[idx].as_ref()
    }

    /// Marks a record as soft-deleted (tombstone).
    /// The slot is kept so WAL replay can reconstruct the deletion, but the
    /// record is excluded from `iter()`, searches, and `record_count`.
    pub fn soft_delete(&mut self, id: RecordId) -> Result<()> {
        let idx = id.0 as usize;
        if idx >= self.records.len() {
            return Err(KernelError::NotFound);
        }
        match self.records[idx].as_mut() {
            Some(record) => {
                record.flags |= FLAG_SOFT_DELETED;
                Ok(())
            }
            None => Err(KernelError::NotFound),
        }
    }

    /// Iterates over all **live** records (excludes hard-deleted and soft-deleted slots).
    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        self.records
            .iter()
            .filter_map(|opt| opt.as_ref())
            .filter(|r| r.is_active())
    }

    /// Total number of allocated slots (live + soft-deleted; excludes hard-deleted `None` gaps).
    pub fn total_slots(&self) -> usize {
        self.records.len()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_full(&self) -> bool {
        false
    }
}
