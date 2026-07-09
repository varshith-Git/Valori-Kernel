// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! # valori-core
//!
//! Minimal-dependency shared foundation for the Valori platform.
//!
//! Every Valori crate depends on `valori-core`. Its only dependencies are
//! `serde` (serialization) and `thiserror` (error derives) — both `no_std` —
//! plus `getrandom` behind the `std` feature for `ExecutionId::new_random`.
//!
//! ## Contents
//! - **IDs** — `RecordId`, `NodeId`, `EdgeId`, `NamespaceId`, `CollectionId`,
//!   `ExecutionId`, `ShardId`, `ClusterEpoch`
//! - **Enums** — `NodeKind`, `EdgeKind`
//! - **Version** — monotonic schema/data version counter
//! - **Errors** — `CoreError`, `Result<T>`
//! - **Constants** — `DEFAULT_NS`, `NS_LIST_NIL`, `MAX_NAMESPACES`

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod id;
pub mod enums;
pub mod error;
pub mod version;

pub use id::{
    RecordId, NodeId, EdgeId, NamespaceId, CollectionId, ExecutionId,
    ShardId, ClusterEpoch,
    DEFAULT_NS, NS_LIST_NIL, MAX_NAMESPACES,
};
pub use enums::{NodeKind, EdgeKind};
pub use error::{CoreError, Result};
pub use version::Version;
