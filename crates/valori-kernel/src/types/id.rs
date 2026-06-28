// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Identity types.

use serde::{Serialize, Deserialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct RecordId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct EdgeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Version(pub u64);

impl Version {
    pub fn next(&self) -> Self {
        Version(self.0 + 1)
    }
}

/// Logical namespace (collection) identifier. `NamespaceId(0)` is the
/// "default" namespace, which always exists and cannot be dropped.
/// The Node layer resolves human-readable collection names to this integer;
/// the kernel only ever sees the raw ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct NamespaceId(pub u16);

/// The default (auto-created, never droppable) namespace.
pub const DEFAULT_NS: NamespaceId = NamespaceId(0);

/// Sentinel for the intrusive namespace linked lists — "no next / no prev".
/// Stored as a raw `u32` in `Record.next_in_ns` / `Record.prev_in_ns` etc.
pub const NS_LIST_NIL: u32 = u32::MAX;

/// Maximum number of namespaces per kernel instance.
/// The per-namespace head arrays cost `MAX_NAMESPACES × 4 bytes` = 4 KB each.
pub const MAX_NAMESPACES: usize = 1024;
