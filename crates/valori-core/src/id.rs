// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Identity types for the Valori platform.
//!
//! All IDs are `#[repr(transparent)]` newtype wrappers over primitive integers.
//! They are `Copy`, `Eq`, `Hash`, and `serde`-serializable.

use serde::{Deserialize, Serialize};

// ── Record / Graph IDs ────────────────────────────────────────────────────────

/// Identifies a vector record in the kernel's record pool.
/// Sequential, 0-based, never reused within a KernelState instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
         Serialize, Deserialize)]
#[repr(transparent)]
pub struct RecordId(pub u32);

/// Identifies a node in the knowledge graph.
/// Sequential, 0-based within a KernelState instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
         Serialize, Deserialize)]
#[repr(transparent)]
pub struct NodeId(pub u32);

/// Identifies a directed edge in the knowledge graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
         Serialize, Deserialize)]
#[repr(transparent)]
pub struct EdgeId(pub u32);

// ── Namespace / Collection IDs ────────────────────────────────────────────────

/// Logical namespace (collection) identifier.
///
/// `NamespaceId(0)` is the "default" namespace — always exists, cannot be
/// dropped. The node layer resolves human-readable collection names to this
/// integer; the kernel only ever sees the raw ID.
///
/// Alias: `CollectionId` is the user-facing name for the same concept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
         Serialize, Deserialize)]
#[repr(transparent)]
pub struct NamespaceId(pub u16);

/// User-facing alias for `NamespaceId`.
/// Use `CollectionId` in HTTP handlers and SDK types; `NamespaceId` in kernel
/// internals. They are the same type.
pub type CollectionId = NamespaceId;

/// The default namespace — `NamespaceId(0)`.
/// Always exists. Cannot be dropped.
pub const DEFAULT_NS: NamespaceId = NamespaceId(0);

/// Sentinel for the intrusive namespace linked lists in the record pool.
/// Stored as a raw `u32` in `Record.next_in_ns` / `Record.prev_in_ns`.
pub const NS_LIST_NIL: u32 = u32::MAX;

/// Maximum namespaces per kernel instance.
/// The per-namespace head arrays cost `MAX_NAMESPACES × 4 bytes` = 4 KB each.
pub const MAX_NAMESPACES: usize = 1024;

// ── Cluster IDs ───────────────────────────────────────────────────────────────

/// Identifies a logical shard.
/// Namespace → shard routing: `shard_id = namespace_id % shard_count`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
         Serialize, Deserialize)]
#[repr(transparent)]
pub struct ShardId(pub u32);

/// Monotonically increasing cluster epoch.
/// Bumped on every membership change (node join/leave, shard reassignment).
/// Used by the planner to detect stale execution plans.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
         Serialize, Deserialize)]
#[repr(transparent)]
pub struct ClusterEpoch(pub u64);

impl ClusterEpoch {
    pub fn next(self) -> Self {
        ClusterEpoch(self.0 + 1)
    }
}

// ── Execution ID ──────────────────────────────────────────────────────────────

/// Unique identifier for a single execution (insert, search, snapshot, replay).
///
/// Used by the planner / runtime to correlate tasks, receipts, and traces.
/// 128-bit UUID-compatible — stored as two u64s for `no_std` compatibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExecutionId {
    pub hi: u64,
    pub lo: u64,
}

impl ExecutionId {
    pub const ZERO: Self = ExecutionId { hi: 0, lo: 0 };

    /// Construct from a 16-byte UUID array.
    pub fn from_bytes(b: [u8; 16]) -> Self {
        let hi = u64::from_be_bytes([b[0],b[1],b[2],b[3],b[4],b[5],b[6],b[7]]);
        let lo = u64::from_be_bytes([b[8],b[9],b[10],b[11],b[12],b[13],b[14],b[15]]);
        ExecutionId { hi, lo }
    }

    pub fn to_bytes(self) -> [u8; 16] {
        let hi = self.hi.to_be_bytes();
        let lo = self.lo.to_be_bytes();
        [hi[0],hi[1],hi[2],hi[3],hi[4],hi[5],hi[6],hi[7],
         lo[0],lo[1],lo[2],lo[3],lo[4],lo[5],lo[6],lo[7]]
    }
}

impl Default for ExecutionId {
    fn default() -> Self { Self::ZERO }
}

impl core::fmt::Display for ExecutionId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:016x}{:016x}", self.hi, self.lo)
    }
}

/// Parses the `Display` format: exactly 32 lowercase-or-uppercase hex digits.
impl core::str::FromStr for ExecutionId {
    type Err = crate::CoreError;

    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        if s.len() != 32 || !s.is_ascii() {
            return Err(crate::CoreError::InvalidInput(
                "ExecutionId must be 32 hex digits",
            ));
        }
        let hi = u64::from_str_radix(&s[..16], 16);
        let lo = u64::from_str_radix(&s[16..], 16);
        match (hi, lo) {
            (Ok(hi), Ok(lo)) => Ok(ExecutionId { hi, lo }),
            _ => Err(crate::CoreError::InvalidInput(
                "ExecutionId must be 32 hex digits",
            )),
        }
    }
}

// ── std-only: UUID interop ────────────────────────────────────────────────────

#[cfg(feature = "std")]
impl ExecutionId {
    /// Generate a random ExecutionId using the OS RNG.
    ///
    /// Panics if the OS entropy source is unavailable — an ID collision is
    /// worse than a crash here, since these IDs correlate jobs and receipts.
    pub fn new_random() -> Self {
        let mut b = [0u8; 16];
        getrandom::getrandom(&mut b).expect("OS RNG unavailable");
        Self::from_bytes(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_id_roundtrip() {
        let id = ExecutionId { hi: 0x0102030405060708, lo: 0x090a0b0c0d0e0f10 };
        let bytes = id.to_bytes();
        assert_eq!(ExecutionId::from_bytes(bytes), id);
    }

    #[test]
    fn default_ns_is_zero() {
        assert_eq!(DEFAULT_NS.0, 0);
    }

    #[test]
    fn cluster_epoch_increment() {
        let e = ClusterEpoch(5);
        assert_eq!(e.next().0, 6);
    }
}
