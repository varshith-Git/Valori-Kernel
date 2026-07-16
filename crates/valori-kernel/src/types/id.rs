// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Identity types — re-exported from `valori-core`.
//!
//! All kernel code should import from `valori_kernel::types::id` (this file)
//! so that callers do not need to add a direct `valori-core` dependency yet.
//! Once the workspace is fully migrated, imports can switch to `valori_core::id`.

pub use valori_core::{
    ClusterEpoch, CollectionId, EdgeId, ExecutionId, NamespaceId, NodeId, RecordId, ShardId,
    DEFAULT_NS, MAX_NAMESPACES, NS_LIST_NIL,
};

// Version lives here too for kernel consumers.
pub use valori_core::Version;
