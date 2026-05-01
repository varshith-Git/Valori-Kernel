//! Static Record Pool.

// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
use crate::storage::record::Record;
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

    /// Iterates over all active records in deterministic order (by index).
    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        self.records.iter().filter_map(|opt| opt.as_ref())
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_full(&self) -> bool {
        false
    }
}
