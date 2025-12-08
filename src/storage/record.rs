//! Record definition.

use crate::types::id::RecordId;
use crate::types::vector::FxpVector;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Record<const D: usize> {
    pub id: RecordId,
    pub vector: FxpVector<D>,
    pub flags: u8,
}

impl<const D: usize> Record<D> {
    pub fn new(id: RecordId, vector: FxpVector<D>) -> Self {
        Self {
            id,
            vector,
            flags: 0,
        }
    }
}
