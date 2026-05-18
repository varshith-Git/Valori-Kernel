// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! N-dimensional vector using Fixed-Point scalars.
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Fixed-Point Vector type.

use crate::types::scalar::FxpScalar;
use core::ops::{Index, IndexMut};

use serde::{Serialize, Deserialize};

/// A dynamic-dimension vector definition.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FxpVector {
    pub data: alloc::vec::Vec<FxpScalar>,
}

impl Default for FxpVector {
    fn default() -> Self {
        Self {
            data: alloc::vec::Vec::new(),
        }
    }
}

impl FxpVector {
    /// Creates a new empty vector.
    pub fn new_empty() -> Self {
        Self::default()
    }

    /// Creates a new vector of dimension D with all zeros.
    pub fn new_zeros(dim: usize) -> Self {
        let mut data = alloc::vec::Vec::with_capacity(dim);
        data.resize(dim, FxpScalar::ZERO);
        Self { data }
    }

    /// Returns a slice of the vector data.
    pub fn as_slice(&self) -> &[FxpScalar] {
        &self.data
    }

    /// Returns a mutable slice of the vector data.
    pub fn as_mut_slice(&mut self) -> &mut [FxpScalar] {
        &mut self.data
    }
    
    pub fn len(&self) -> usize {
        self.data.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

// Iterator support
impl<'a> IntoIterator for &'a FxpVector {
    type Item = &'a FxpScalar;
    type IntoIter = core::slice::Iter<'a, FxpScalar>;

    fn into_iter(self) -> Self::IntoIter {
        self.data.iter()
    }
}

impl Index<usize> for FxpVector {
    type Output = FxpScalar;

    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}

impl IndexMut<usize> for FxpVector {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index]
    }
}
