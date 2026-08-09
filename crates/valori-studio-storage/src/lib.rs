// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Durable Studio-local metadata store — `~/.valori/studio.redb`.
//!
//! # What this crate is
//!
//! `valori-studio-storage` owns the single redb database backing Valori
//! Studio's own local application state: preferences, the project
//! registry, application sessions, a durable telemetry queue, sync
//! bookkeeping, and updater state. It is the storage foundation described
//! in `docs/architecture/studio-storage-audit.md` (S1: the audit) and
//! `docs/architecture/studio-storage.md` (this crate's own contract).
//!
//! # What this crate is not
//!
//! - **Not the Valori project data store.** Vectors, documents, WAL,
//!   snapshots, indexes, graph data, and model artifacts are owned by
//!   `valori-kernel`, `valori-wire`, `valori-storage`, and `valori-models`
//!   — never here. See `crate::project` module docs.
//! - **Not `valori-metadata`.** `~/.valori/metadata.redb` is a *separate*
//!   file owned by the node/daemon control plane (`Project`, `Collection`,
//!   planner cache). `studio.redb` is a different file with a different
//!   owner — see `docs/architecture/studio-storage.md`
//!   §"Database ownership".
//! - **Not a secrets store.** API keys, OAuth tokens, and any other
//!   credential must never be serialized into any table this crate
//!   defines — see `docs/architecture/studio-storage.md` §"Security".
//! - **Not wired into the running desktop app in S1.** This crate is
//!   self-contained and independently testable
//!   (`cargo test -p valori-studio-storage`); `desktop/src-tauri` does not
//!   yet depend on it, and no existing Studio persistence
//!   (`preferences.json`, `events.jsonl`, `localStorage`, the project
//!   manifest format) is touched. That migration is a separate, later
//!   phase.
//!
//! # Dependency direction
//!
//! This crate depends on `valori-domain` only (for `ProjectId`,
//! `SessionId`, `InstallationId`) and must never depend on
//! `valori-daemon`, `valori-node`, `valori-metadata`, `valori-consensus`,
//! or any Cloud crate. Enforced by
//! `crates/valori-node/tests/dependency_direction.rs`'s `SEALED_CRATES`.
//!
//! # Authoritative vs. cache
//!
//! Every store module documents, in its own doc comment, what it owns and
//! what it merely mirrors. The one universal rule: [`crate::project_cache`]
//! is disposable and must never be treated as a source of truth; deleting
//! it must never affect [`crate::project`]'s registry.

pub mod db;
pub mod error;
pub mod migration;
pub mod path;
pub mod preferences;
pub mod project;
pub mod project_cache;
pub mod recovery;
mod schema;
pub mod session;
pub mod sync;
pub mod telemetry;
pub mod update;

pub use db::{LegacyMigrationSummary, LegacyStudioPaths, StudioDatabase};
pub use error::{StudioStorageError, StudioStorageResult};
pub use path::StudioPaths;
pub use recovery::{RecoveryLogEntry, RecoveryOutcome, RecoveryState};
pub use schema::CURRENT_SCHEMA_VERSION;
