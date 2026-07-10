// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Control-plane persistence for the Valori platform.
//!
//! `valori-metadata` owns the durable store for project configuration,
//! collection name→NamespaceId mappings, and the planner cache.
//!
//! Storage backend: `redb` — the same embedded key-value store used by the Raft
//! log in `valori-consensus`.

pub mod collection;
pub mod db;
pub mod error;
pub mod history;
pub mod planner_cache;
pub mod project;

pub use collection::{Collection, CollectionRegistry, MAX_COLLECTIONS};
pub use db::MetadataDb;
pub use error::{MetadataError, MetadataResult};
pub use history::ExecutionRetentionPolicy;
pub use planner_cache::{PlannerCacheEntry, PlannerCacheKey};
pub use project::{ClusterNodeConfig, IndexKind, Project, ProjectMode};
