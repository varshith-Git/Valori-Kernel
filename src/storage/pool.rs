//! Static Record Pool.

use crate::storage::record::Record;
use crate::types::id::RecordId;
use crate::types::vector::FxpVector;
use crate::error::{Result, KernelError};

pub struct RecordPool<const CAP: usize, const D: usize> {
    pub(crate) records: [Option<Record<D>>; CAP],
}

impl<const CAP: usize, const D: usize> RecordPool<CAP, D> {
    pub(crate) fn raw_records(&self) -> &[Option<Record<D>>] {
        &self.records
    }

    pub fn new() -> Self {
        Self {
            records: [None; CAP],
        }
    }

    /// Inserts a vector into the first available slot.
    /// Returns the RecordId (which corresponds to the index).
    pub fn insert(&mut self, vector: FxpVector<D>) -> Result<RecordId> {
        // Deterministic scan for first empty slot
        for (i, slot) in self.records.iter_mut().enumerate() {
            if slot.is_none() {
                let id = RecordId(i as u32);
                *slot = Some(Record::new(id, vector));
                return Ok(id);
            }
        }
        Err(KernelError::CapacityExceeded)
    }

    /// Deletes the record at the specified ID (index).
    pub fn delete(&mut self, id: RecordId) -> Result<()> {
        let idx = id.0 as usize;
        if idx >= CAP {
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
    pub fn get(&self, id: RecordId) -> Option<&Record<D>> {
        let idx = id.0 as usize;
        if idx >= CAP {
            return None;
        }
        self.records[idx].as_ref()
    }

    /// Iterates over all active records in deterministic order (by index).
    pub fn iter(&self) -> impl Iterator<Item = &Record<D>> {
        self.records.iter().filter_map(|opt| opt.as_ref())
    }

    pub fn len(&self) -> usize {
        self.records.iter().filter(|s| s.is_some()).count()
    }

    pub fn is_full(&self) -> bool {
        self.len() >= CAP
    }
}
