// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Control-plane persistence for the Valori platform.
//!
//! `valori-metadata` owns the durable store for everything that does not belong
//! in the kernel's hot-path (`KernelState`): project configuration, collection
//! name→NamespaceId mappings, shard topology, snapshot catalog, execution
//! history, and the planner cache.
//!
//! Storage backend: `redb` — the same embedded key-value store used by the Raft
//! log in `valori-consensus`.

pub mod collection;
pub mod db;
pub mod error;
pub mod history;
pub mod planner_cache;
pub mod project;
pub mod shard;
pub mod snapshot;

pub use collection::{Collection, CollectionRegistry, MAX_COLLECTIONS};
pub use db::MetadataDb;
pub use error::{MetadataError, MetadataResult};
pub use history::{ExecutionRecord, ExecutionRetentionPolicy, ExecutionStatus};
pub use planner_cache::{PlannerCacheEntry, PlannerCacheKey};
pub use project::{ClusterNodeConfig, IndexKind, Project, ProjectMode};
pub use shard::{ShardConfig, ShardMember, ShardTopology};
pub use snapshot::{SnapshotCatalog, SnapshotRecord, SNAPSHOT_FORMAT_V5, SNAPSHOT_FORMAT_V6};
