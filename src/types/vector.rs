// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! N-dimensional vector using Fixed-Point scalars.
// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Fixed-Point Vector type.

use crate::types::scalar::FxpScalar;
use core::ops::{Index, IndexMut};

use serde::{Serialize, Deserialize, Serializer, Deserializer};
use serde::ser::SerializeTuple;
use serde::de::{self, SeqAccess, Visitor};
use core::fmt;

/// A fixed-dimension vector definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FxpVector<const D: usize> {
    pub data: [FxpScalar; D],
}

impl<const D: usize> Serialize for FxpVector<D> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_tuple(D)?;
        for element in &self.data {
            seq.serialize_element(element)?;
        }
        seq.end()
    }
}

// Deserialize manual impl for const generic array
struct FxpVectorVisitor<const D: usize>;

impl<'de, const D: usize> Visitor<'de> for FxpVectorVisitor<D> {
    type Value = FxpVector<D>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_fmt(format_args!("a sequence of {} FxpScalars", D))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        // Must read D elements
        let mut data = [FxpScalar::default(); D];
        for i in 0..D {
            data[i] = seq.next_element()?
                .ok_or_else(|| de::Error::invalid_length(i, &self))?;
        }
        Ok(FxpVector { data })
    }
}

impl<'de, const D: usize> Deserialize<'de> for FxpVector<D> {
    fn deserialize<Desc>(deserializer: Desc) -> Result<Self, Desc::Error>
    where
        Desc: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(D, FxpVectorVisitor::<D>)
    }
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
