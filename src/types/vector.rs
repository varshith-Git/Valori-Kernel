// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! N-dimensional vector using Fixed-Point scalars.
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Fixed-Point Vector type.

use crate::types::scalar::FxpScalar;
use core::ops::{Index, IndexMut};

/// A fixed-dimension vector definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FxpVector<const D: usize> {
    pub data: [FxpScalar; D],
}

impl<const D: usize> Default for FxpVector<D> {
    fn default() -> Self {
        Self {
            data: [FxpScalar::ZERO; D],
        }
    }
}

impl<const D: usize> FxpVector<D> {
    /// Creates a new vector with all zeros.
    pub fn new_zeros() -> Self {
        Self::default()
    }

    /// Returns a slice of the vector data.
    pub fn as_slice(&self) -> &[FxpScalar] {
        &self.data
    }

    /// Returns a mutable slice of the vector data.
    pub fn as_mut_slice(&mut self) -> &mut [FxpScalar] {
        &mut self.data
    }
}

// Iterator support
impl<'a, const D: usize> IntoIterator for &'a FxpVector<D> {
    type Item = &'a FxpScalar;
    type IntoIter = core::slice::Iter<'a, FxpScalar>;

    fn into_iter(self) -> Self::IntoIter {
        self.data.iter()
    }
}

impl<const D: usize> Index<usize> for FxpVector<D> {
    type Output = FxpScalar;

    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}

impl<const D: usize> IndexMut<usize> for FxpVector<D> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index]
    }
}
