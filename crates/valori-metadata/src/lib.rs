// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Control-plane persistence for the Valori platform.
//!
//! `valori-metadata` owns the durable store for project configuration,
//! collection name→NamespaceId mappings, and the planner cache.
//!
//! Storage backend: `redb` — the same embedded key-value store used by the Raft
//! log in `valori-consensus`.
//!
//! # Status: `MetadataDb` is not opened by any production binary (S7)
//!
//! Confirmed by the S4, S6, and S7 persistence audits
//! (`docs/reviews/studio-persistence-consolidation-audit.md`,
//! `docs/reviews/studio-filesystem-audit.md`): no call to `MetadataDb::open`
//! exists in `valori-node`, `valori-daemon`, or `desktop/src-tauri` today.
//! `valori-planner`'s `plan_with_cache()` accepts `db: Option<&MetadataDb>`,
//! and every real call site passes `None` — the durable planner-cache layer
//! is fully coded and tested but not deployed.
//!
//! **`PROJECTS`/`COLLECTIONS` are not this crate's current, active
//! responsibility.** `valori-daemon`'s `project.json` and
//! `valori-studio-storage`'s `projects` table are the sole authorities for
//! project identity/registry today — there is no conflict in practice
//! because nothing opens this file. [`domain_adapter`] exists as
//! deliberately-staged, tested infrastructure for **M3** (see
//! `docs/phases/phase-M0-M2-platform-contracts.md`: *"Nothing deleted or
//! migrated — M3 stopped for review"*), not as active production code.
//! `crates/valori-node/tests/dependency_direction.rs`'s
//! `metadata_db_open_stays_out_of_production_binaries` test enforces this
//! stays true — reactivating `MetadataDb` in a real binary is a deliberate
//! architecture decision that must update that test, not something that can
//! happen by accident.
//!
//! This crate was deliberately **not** deleted or trimmed to retire it: its
//! `PLANNER_CACHE` table/API is genuinely referenced (if currently dormant)
//! by `valori-planner`, and `domain_adapter`'s `Project`/`Collection`
//! conversion code is real, tested M3-preparation work a past phase chose to
//! pause rather than abandon — removing it would destroy that work for no
//! safety gain, since the actual risk (two live authorities disagreeing) is
//! not currently possible.

pub mod collection;
pub mod db;
pub mod domain_adapter;
pub mod error;
pub mod planner_cache;
pub mod project;

pub use collection::{Collection, CollectionRegistry, MAX_COLLECTIONS};
pub use db::MetadataDb;
pub use error::{MetadataError, MetadataResult};
pub use planner_cache::{PlannerCacheEntry, PlannerCacheKey};
pub use project::{ClusterNodeConfig, Project, ProjectMode};
