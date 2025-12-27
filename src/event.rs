// Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
//! Event Log as Primary Truth
//!
//! This module defines the canonical event representation for Valori.
//! Every state transition must be expressed as a `KernelEvent`.
//!
//! # Determinism Guarantees
//! - No timestamps
//! - No randomness
//! - No implicit metadata
//! - No side-effect derived state
//! - No optional ordering dependence
//!
//! # Invariants
//! - Same event log => Same final state (on any architecture)
//! - Events are immutable once committed
//! - Replay must be deterministic and reproducible

use crate::types::id::{RecordId, NodeId, EdgeId};
use crate::types::vector::FxpVector;
use crate::types::enums::{NodeKind, EdgeKind};
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use serde::ser::{SerializeStruct, SerializeStructVariant};
use serde::de::{self, Visitor, MapAccess, SeqAccess, EnumAccess, VariantAccess};
use core::fmt;

/// KernelEvent represents the canonical event language for Valori.
/// This is the ONLY way to express state transitions.
///
/// Each variant represents an atomic, deterministic operation on the kernel state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelEvent<const D: usize> {
    /// Insert a new vector record into the kernel
    InsertRecord {
        id: RecordId,
        vector: FxpVector<D>,
        metadata: Option<alloc::vec::Vec<u8>>,
    },

    /// Delete an existing vector record from the kernel
    DeleteRecord {
        id: RecordId,
    },

    /// Create a new graph node
    CreateNode {
        id: NodeId,
        kind: NodeKind,
        record: Option<RecordId>,
    },

    /// Create a new graph edge
    CreateEdge {
        id: EdgeId,
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
    },

    /// Delete an existing graph edge
    DeleteEdge {
        id: EdgeId,
    },
}

impl<const D: usize> KernelEvent<D> {
    /// Returns a human-readable description of the event type
    pub fn event_type(&self) -> &'static str {
        match self {
            KernelEvent::InsertRecord { .. } => "InsertRecord",
            KernelEvent::DeleteRecord { .. } => "DeleteRecord",
            KernelEvent::CreateNode { .. } => "CreateNode",
            KernelEvent::CreateEdge { .. } => "CreateEdge",
            KernelEvent::DeleteEdge { .. } => "DeleteEdge",
        }
    }
}

// Custom Serialization to support strict V2 Metadata format
impl<const D: usize> Serialize for KernelEvent<D> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            KernelEvent::InsertRecord { id, vector, metadata } => {
                // We serialize as a struct variant with 3 fields for Serialize
                // But specifically for metadata, we manually encode the length + bytes
                // To achieve "No version flag", we just write the fields.
                // Bincode enum serialization: [VariantIdx][Field1][Field2][...]
                let mut state = serializer.serialize_struct_variant("KernelEvent", 0, "InsertRecord", 3)?;
                state.serialize_field("id", id)?;
                state.serialize_field("vector", vector)?;
                
                // Custom Metadata Serialization: u32 Len + Bytes
                // We wrap this in a helper or just serialize a "RawMetadata" struct
                let meta_wrapper = RawMetadata(metadata.as_ref());
                state.serialize_field("metadata", &meta_wrapper)?;
                
                state.end()
            }
            KernelEvent::DeleteRecord { id } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 1, "DeleteRecord", 1)?;
                state.serialize_field("id", id)?;
                state.end()
            }
            KernelEvent::CreateNode { id, kind, record } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 2, "CreateNode", 3)?;
                state.serialize_field("id", id)?;
                state.serialize_field("kind", kind)?;
                state.serialize_field("record", record)?;
                state.end()
            }
            KernelEvent::CreateEdge { id, from, to, kind } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 3, "CreateEdge", 4)?;
                state.serialize_field("id", id)?;
                state.serialize_field("from", from)?;
                state.serialize_field("to", to)?;
                state.serialize_field("kind", kind)?;
                state.end()
            }
            KernelEvent::DeleteEdge { id } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 4, "DeleteEdge", 1)?;
                state.serialize_field("id", id)?;
                state.end()
            }
        }
    }
}

struct RawMetadata<'a>(Option<&'a alloc::vec::Vec<u8>>);

impl<'a> Serialize for RawMetadata<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        match self.0 {
            Some(bytes) => {
                let len = bytes.len() as u32;
                // We can't write two fields here easily if we are just one field in the parent struct?
                // Actually Bincode flattens structs.
                // So if we serialize as a tuple `(len, bytes)`, it writes [len][bytes].
                (len, bytes).serialize(serializer)
            }
            None => {
                let len: u32 = 0;
                // Write 0 length, no bytes.
                len.serialize(serializer)
            }
        }
    }
}

// Custom Deserialization
impl<'de, const D: usize> Deserialize<'de> for KernelEvent<D> {
    fn deserialize<Deser>(deserializer: Deser) -> Result<Self, Deser::Error>
    where
        Deser: Deserializer<'de>,
    {
        // Use a Shadow Enum that matches the structure but uses a custom type for Metadata
        // This allows us to intercept the metadata deserialization for backward compatibility logic
        #[derive(Serialize, Deserialize)]
        enum KernelEventHelper<const D: usize> {
             InsertRecord {
                 id: RecordId,
                 vector: FxpVector<D>,
                 #[serde(with = "raw_metadata_serde")]
                 metadata: Option<alloc::vec::Vec<u8>>,
             },
             DeleteRecord {
                 id: RecordId,
             },
             CreateNode {
                 id: NodeId,
                 kind: NodeKind,
                 record: Option<RecordId>,
             },
             CreateEdge {
                 id: EdgeId,
                 from: NodeId,
                 to: NodeId,
                 kind: EdgeKind,
             },
             DeleteEdge {
                 id: EdgeId,
             },
        }
        
        // Delegate to the Helper
        let helper = KernelEventHelper::<D>::deserialize(deserializer)?;
        
        Ok(match helper {
            KernelEventHelper::InsertRecord { id, vector, metadata } => KernelEvent::InsertRecord { id, vector, metadata },
            KernelEventHelper::DeleteRecord { id } => KernelEvent::DeleteRecord { id },
            KernelEventHelper::CreateNode { id, kind, record } => KernelEvent::CreateNode { id, kind, record },
            KernelEventHelper::CreateEdge { id, from, to, kind } => KernelEvent::CreateEdge { id, from, to, kind },
            KernelEventHelper::DeleteEdge { id } => KernelEvent::DeleteEdge { id },
        })
    }
}

mod raw_metadata_serde {
    use super::*;
    use serde::{Serializer, Deserializer};
    
    pub fn serialize<S>(metadata: &Option<alloc::vec::Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        match metadata {
            Some(bytes) => {
                let len = bytes.len() as u32;
                (len, bytes).serialize(serializer)
            }
            None => {
                let len: u32 = 0;
                len.serialize(serializer)
            }
        }
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<alloc::vec::Vec<u8>>, D::Error>
    where D: Deserializer<'de> {
         struct MetadataVisitor;
         impl<'de> Visitor<'de> for MetadataVisitor {
             type Value = Option<alloc::vec::Vec<u8>>;
             fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                 formatter.write_str("metadata length and bytes")
             }
             
             fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
             where A: SeqAccess<'de> {
                 let len: u32 = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
                 
                 if len == 0 {
                     return Ok(None);
                 }
                 
                 let bytes: alloc::vec::Vec<u8> = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
                 Ok(Some(bytes))
             }
         }
         
         deserializer.deserialize_tuple(2, MetadataVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization_determinism() {
        // Verify that serialization is deterministic
        let event = KernelEvent::<16>::InsertRecord {
            id: RecordId(42),
            vector: FxpVector::new_zeros(),
            metadata: Some(alloc::vec![0xAA, 0xBB]),
        };

        let bytes1 = bincode::serde::encode_to_vec(&event, bincode::config::standard()).unwrap();
        let bytes2 = bincode::serde::encode_to_vec(&event, bincode::config::standard()).unwrap();

        assert_eq!(bytes1, bytes2, "Event serialization must be deterministic");
    }

    #[test]
    fn test_event_roundtrip() {
        // Verify serialize/deserialize roundtrip
        let original = KernelEvent::<16>::CreateNode {
            id: NodeId(1),
            kind: NodeKind::Document,
            record: Some(RecordId(42)),
        };

        let bytes = bincode::serde::encode_to_vec(&original, bincode::config::standard()).unwrap();
        let (decoded, _): (KernelEvent<16>, _) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();

        assert_eq!(original, decoded, "Event must survive serialization roundtrip");
    }
}
