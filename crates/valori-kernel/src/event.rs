// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
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
use serde::ser::SerializeStructVariant;
use serde::de::{self, Visitor, SeqAccess};
use core::fmt;

/// KernelEvent represents the canonical event language for Valori.
/// This is the ONLY way to express state transitions.
///
/// Each variant represents an atomic, deterministic operation on the kernel state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelEvent {
    /// Insert a new vector record into the kernel
    InsertRecord {
        id: RecordId,
        vector: FxpVector,
        metadata: Option<alloc::vec::Vec<u8>>,
        tag: u64,
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

    /// Soft-delete a record: mark it as a tombstone without removing the slot.
    /// The record is excluded from search and record_count but the slot is
    /// retained so WAL replay can reconstruct the tombstone state.
    SoftDeleteRecord {
        id: RecordId,
    },

    /// Delete a graph node and cascade-delete all its incident edges.
    DeleteNode {
        id: NodeId,
    },

    // ── Phase 1.5 reserved variants — schema exists now so future code is
    //   additive, not a wire-format break. apply_event() refuses both with
    //   KernelError::NotImplemented until the encryption engine is wired in.

    /// Insert a vector record whose payload has been encrypted under `key_id`.
    /// Stored in the log forever; becomes permanently unreadable after the key
    /// is shredded — satisfying GDPR erasure without breaking the audit chain.
    InsertRecordEncrypted {
        id: RecordId,
        key_id: [u8; 16],
        ciphertext: alloc::vec::Vec<u8>,
        metadata_ciphertext: Option<alloc::vec::Vec<u8>>,
        tag: u64,
    },

    /// Destroy the key `key_id` in the vault. All records encrypted under this
    /// key become unrecoverable. The log entry itself is permanent; the data
    /// content is erased.
    ShredKey {
        key_id: [u8; 16],
    },

    /// Insert a record with the ID assigned by the state machine at apply time.
    /// Cluster-mode only. Using a server-assigned ID eliminates the
    /// pre-allocation race that `InsertRecord` requires the handler to work
    /// around with a retry loop and a per-node mutex.
    AutoInsertRecord {
        vector: FxpVector,
        metadata: Option<alloc::vec::Vec<u8>>,
        tag: u64,
    },

    /// Create a graph node with an ID assigned at apply time (cluster-mode).
    /// Analogous to `AutoInsertRecord` — every replica calls `next_node_id()`
    /// in the same Raft-ordered sequence and arrives at the same ID.
    AutoCreateNode {
        kind: NodeKind,
        record: Option<RecordId>,
    },

    /// Create a graph edge with an ID assigned at apply time (cluster-mode).
    AutoCreateEdge {
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
    },

    /// Insert an encrypted record with ID assigned at apply time (cluster-mode).
    /// The ciphertext was computed by the leader's vault before submitting to Raft.
    AutoInsertRecordEncrypted {
        namespace_id: u16,
        key_id: [u8; 16],
        ciphertext: alloc::vec::Vec<u8>,
        tag: u64,
    },

    /// Store a metadata key-value pair in the kernel (cluster-mode).
    /// Value is a pre-serialized JSON string. Applied on every replica so all
    /// nodes share the same metadata sidecar after ingest.
    SetMeta {
        key: alloc::string::String,
        value: alloc::string::String,
    },

    /// Register a namespace name -> id mapping, id assigned at apply time
    /// (cluster-mode; Phase S2). Analogous to `AutoInsertRecord` — the
    /// consensus layer allocates the id deterministically (every replica
    /// applies entries in the same Raft-ordered sequence) and calls
    /// `KernelState::apply_event_ns` with the id already decided. The name
    /// itself is not stored in `KernelState` (namespaces are pure integers
    /// here); it rides in the event for audit-log readability and so the
    /// consensus-layer registry can replay it.
    AutoCreateNamespace {
        name: alloc::string::String,
    },

    /// Drop a namespace by name (cluster-mode; Phase S2), cascading exactly
    /// like `Command::DropNamespace`. Takes a name rather than a pre-resolved
    /// id: the consensus layer resolves name -> id from its own registry
    /// immediately before calling `apply_event_ns`, inside the same
    /// Raft-apply critical section, so there is no time-of-check/time-of-use
    /// race between resolving and dropping.
    DropNamespace {
        name: alloc::string::String,
    },
}

impl KernelEvent {
    /// Returns a human-readable description of the event type
    pub fn event_type(&self) -> &'static str {
        match self {
            KernelEvent::InsertRecord { .. } => "InsertRecord",
            KernelEvent::DeleteRecord { .. } => "DeleteRecord",
            KernelEvent::CreateNode { .. } => "CreateNode",
            KernelEvent::CreateEdge { .. } => "CreateEdge",
            KernelEvent::DeleteEdge { .. } => "DeleteEdge",
            KernelEvent::SoftDeleteRecord { .. } => "SoftDeleteRecord",
            KernelEvent::DeleteNode { .. } => "DeleteNode",
            KernelEvent::InsertRecordEncrypted { .. } => "InsertRecordEncrypted",
            KernelEvent::ShredKey { .. } => "ShredKey",
            KernelEvent::AutoInsertRecord { .. } => "AutoInsertRecord",
            KernelEvent::AutoCreateNode { .. } => "AutoCreateNode",
            KernelEvent::AutoCreateEdge { .. } => "AutoCreateEdge",
            KernelEvent::AutoInsertRecordEncrypted { .. } => "AutoInsertRecordEncrypted",
            KernelEvent::SetMeta { .. } => "SetMeta",
            KernelEvent::AutoCreateNamespace { .. } => "AutoCreateNamespace",
            KernelEvent::DropNamespace { .. } => "DropNamespace",
        }
    }
}

// Custom Serialization to support strict V2 Metadata format
impl Serialize for KernelEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            KernelEvent::InsertRecord { id, vector, metadata, tag } => {
                // serialized as struct variant with *4* fields now
                let mut state = serializer.serialize_struct_variant("KernelEvent", 0, "InsertRecord", 4)?;
                state.serialize_field("id", id)?;
                state.serialize_field("vector", vector)?;
                
                let meta_wrapper = RawMetadata(metadata.as_ref());
                state.serialize_field("metadata", &meta_wrapper)?;

                state.serialize_field("tag", tag)?;
                
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
            KernelEvent::SoftDeleteRecord { id } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 5, "SoftDeleteRecord", 1)?;
                state.serialize_field("id", id)?;
                state.end()
            }
            KernelEvent::DeleteNode { id } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 6, "DeleteNode", 1)?;
                state.serialize_field("id", id)?;
                state.end()
            }
            KernelEvent::InsertRecordEncrypted { id, key_id, ciphertext, metadata_ciphertext, tag } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 7, "InsertRecordEncrypted", 5)?;
                state.serialize_field("id", id)?;
                state.serialize_field("key_id", key_id)?;
                state.serialize_field("ciphertext", ciphertext)?;
                state.serialize_field("metadata_ciphertext", metadata_ciphertext)?;
                state.serialize_field("tag", tag)?;
                state.end()
            }
            KernelEvent::ShredKey { key_id } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 8, "ShredKey", 1)?;
                state.serialize_field("key_id", key_id)?;
                state.end()
            }
            KernelEvent::AutoInsertRecord { vector, metadata, tag } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 9, "AutoInsertRecord", 3)?;
                state.serialize_field("vector", vector)?;
                state.serialize_field("metadata", &RawMetadata(metadata.as_ref()))?;
                state.serialize_field("tag", tag)?;
                state.end()
            }
            KernelEvent::AutoCreateNode { kind, record } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 10, "AutoCreateNode", 2)?;
                state.serialize_field("kind", kind)?;
                state.serialize_field("record", record)?;
                state.end()
            }
            KernelEvent::AutoCreateEdge { from, to, kind } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 11, "AutoCreateEdge", 3)?;
                state.serialize_field("from", from)?;
                state.serialize_field("to", to)?;
                state.serialize_field("kind", kind)?;
                state.end()
            }
            KernelEvent::AutoInsertRecordEncrypted { namespace_id, key_id, ciphertext, tag } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 12, "AutoInsertRecordEncrypted", 4)?;
                state.serialize_field("namespace_id", namespace_id)?;
                state.serialize_field("key_id", key_id)?;
                state.serialize_field("ciphertext", ciphertext)?;
                state.serialize_field("tag", tag)?;
                state.end()
            }
            KernelEvent::SetMeta { key, value } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 13, "SetMeta", 2)?;
                state.serialize_field("key", key)?;
                state.serialize_field("value", value)?;
                state.end()
            }
            KernelEvent::AutoCreateNamespace { name } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 14, "AutoCreateNamespace", 1)?;
                state.serialize_field("name", name)?;
                state.end()
            }
            KernelEvent::DropNamespace { name } => {
                let mut state = serializer.serialize_struct_variant("KernelEvent", 15, "DropNamespace", 1)?;
                state.serialize_field("name", name)?;
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
impl<'de> Deserialize<'de> for KernelEvent {
    fn deserialize<Deser>(deserializer: Deser) -> Result<Self, Deser::Error>
    where
        Deser: Deserializer<'de>,
    {
        // Use a Shadow Enum that matches the structure but uses a custom type for Metadata
        // This allows us to intercept the metadata deserialization for backward compatibility logic
        #[derive(Serialize, Deserialize)]
        enum KernelEventHelper {
             InsertRecord {
                 id: RecordId,
                 vector: FxpVector,
                 #[serde(with = "raw_metadata_serde")]
                 metadata: Option<alloc::vec::Vec<u8>>,
                 tag: u64,
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
             SoftDeleteRecord {
                 id: RecordId,
             },
             DeleteNode {
                 id: NodeId,
             },
             InsertRecordEncrypted {
                 id: RecordId,
                 key_id: [u8; 16],
                 ciphertext: alloc::vec::Vec<u8>,
                 metadata_ciphertext: Option<alloc::vec::Vec<u8>>,
                 tag: u64,
             },
             ShredKey {
                 key_id: [u8; 16],
             },
             AutoInsertRecord {
                 vector: FxpVector,
                 #[serde(with = "raw_metadata_serde")]
                 metadata: Option<alloc::vec::Vec<u8>>,
                 tag: u64,
             },
             AutoCreateNode {
                 kind: NodeKind,
                 record: Option<RecordId>,
             },
             AutoCreateEdge {
                 from: NodeId,
                 to: NodeId,
                 kind: EdgeKind,
             },
             AutoInsertRecordEncrypted {
                 namespace_id: u16,
                 key_id: [u8; 16],
                 ciphertext: alloc::vec::Vec<u8>,
                 tag: u64,
             },
             SetMeta {
                 key: alloc::string::String,
                 value: alloc::string::String,
             },
             AutoCreateNamespace {
                 name: alloc::string::String,
             },
             DropNamespace {
                 name: alloc::string::String,
             },
        }

        // Delegate to the Helper
        let helper = KernelEventHelper::deserialize(deserializer)?;

        Ok(match helper {
            KernelEventHelper::InsertRecord { id, vector, metadata, tag } => KernelEvent::InsertRecord { id, vector, metadata, tag },
            KernelEventHelper::DeleteRecord { id } => KernelEvent::DeleteRecord { id },
            KernelEventHelper::CreateNode { id, kind, record } => KernelEvent::CreateNode { id, kind, record },
            KernelEventHelper::CreateEdge { id, from, to, kind } => KernelEvent::CreateEdge { id, from, to, kind },
            KernelEventHelper::DeleteEdge { id } => KernelEvent::DeleteEdge { id },
            KernelEventHelper::SoftDeleteRecord { id } => KernelEvent::SoftDeleteRecord { id },
            KernelEventHelper::DeleteNode { id } => KernelEvent::DeleteNode { id },
            KernelEventHelper::InsertRecordEncrypted { id, key_id, ciphertext, metadata_ciphertext, tag } =>
                KernelEvent::InsertRecordEncrypted { id, key_id, ciphertext, metadata_ciphertext, tag },
            KernelEventHelper::ShredKey { key_id } => KernelEvent::ShredKey { key_id },
            KernelEventHelper::AutoInsertRecord { vector, metadata, tag } =>
                KernelEvent::AutoInsertRecord { vector, metadata, tag },
            KernelEventHelper::AutoCreateNode { kind, record } =>
                KernelEvent::AutoCreateNode { kind, record },
            KernelEventHelper::AutoCreateEdge { from, to, kind } =>
                KernelEvent::AutoCreateEdge { from, to, kind },
            KernelEventHelper::AutoInsertRecordEncrypted { namespace_id, key_id, ciphertext, tag } =>
                KernelEvent::AutoInsertRecordEncrypted { namespace_id, key_id, ciphertext, tag },
            KernelEventHelper::SetMeta { key, value } => KernelEvent::SetMeta { key, value },
            KernelEventHelper::AutoCreateNamespace { name } => KernelEvent::AutoCreateNamespace { name },
            KernelEventHelper::DropNamespace { name } => KernelEvent::DropNamespace { name },
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
        let event = KernelEvent::InsertRecord {
            id: RecordId(42),
            vector: FxpVector::new_zeros(16),
            metadata: Some(alloc::vec![0xAA, 0xBB]),
            tag: 123,
        };

        let bytes1 = bincode::serde::encode_to_vec(&event, bincode::config::standard()).unwrap();
        let bytes2 = bincode::serde::encode_to_vec(&event, bincode::config::standard()).unwrap();

        assert_eq!(bytes1, bytes2, "Event serialization must be deterministic");
    }

    #[test]
    fn test_event_roundtrip() {
        // Verify serialize/deserialize roundtrip
        let original = KernelEvent::CreateNode {
            id: NodeId(1),
            kind: NodeKind::Document,
            record: Some(RecordId(42)),
        };

        let bytes = bincode::serde::encode_to_vec(&original, bincode::config::standard()).unwrap();
        let (decoded, _): (KernelEvent, _) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();

        assert_eq!(original, decoded, "Event must survive serialization roundtrip");
    }

    #[test]
    fn test_auto_create_namespace_roundtrip() {
        let original = KernelEvent::AutoCreateNamespace { name: "tenant-acme".into() };
        let bytes = bincode::serde::encode_to_vec(&original, bincode::config::standard()).unwrap();
        let (decoded, _): (KernelEvent, _) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(original, decoded);
        assert_eq!(original.event_type(), "AutoCreateNamespace");
    }

    #[test]
    fn test_drop_namespace_by_name_roundtrip() {
        let original = KernelEvent::DropNamespace { name: "tenant-acme".into() };
        let bytes = bincode::serde::encode_to_vec(&original, bincode::config::standard()).unwrap();
        let (decoded, _): (KernelEvent, _) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(original, decoded);
        assert_eq!(original.event_type(), "DropNamespace");
    }

    #[test]
    fn test_namespace_events_serialization_determinism() {
        let create = KernelEvent::AutoCreateNamespace { name: "docs".into() };
        let b1 = bincode::serde::encode_to_vec(&create, bincode::config::standard()).unwrap();
        let b2 = bincode::serde::encode_to_vec(&create, bincode::config::standard()).unwrap();
        assert_eq!(b1, b2);

        let drop = KernelEvent::DropNamespace { name: "docs".into() };
        let b3 = bincode::serde::encode_to_vec(&drop, bincode::config::standard()).unwrap();
        let b4 = bincode::serde::encode_to_vec(&drop, bincode::config::standard()).unwrap();
        assert_eq!(b3, b4);
    }
}
