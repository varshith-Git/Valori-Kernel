// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! One-time, idempotent startup migrations — how the daemon's on-disk state
//! evolves without ever silently losing user data.
//!
//! Modeled on database migrations: each [`Migration`] has a stable `id()` and
//! a `run()` that must be safe to call more than once (checks its own
//! preconditions rather than assuming it's the first run). [`MigrationRunner`]
//! additionally tracks which ids have already completed in
//! `<home>/.migrations.json`, so a normal run skips work it's already done —
//! but a migration crashing halfway and getting re-invoked must *still*
//! produce a correct result on its own, since the marker is only written
//! after a migration returns `Ok`.
//!
//! Add new migrations by writing a new module (`m002_...`) and appending it
//! to [`all_migrations`] — never remove or renumber a landed one.

mod m001_project_registry;

use std::path::{Path, PathBuf};

use crate::error::DaemonResult;
use crate::store::ProjectStore;

pub use m001_project_registry::Migration001ProjectRegistry;

/// One idempotent, ordered step in the daemon's on-disk schema evolution.
pub trait Migration {
    /// Stable identifier — never reuse or change once shipped, it's the key
    /// recorded in `<home>/.migrations.json`.
    fn id(&self) -> &'static str;

    /// Apply the migration. Must be safe to call again after a partial or
    /// full prior success (check preconditions; skip work already done).
    fn run(&self, home: &Path, projects: &dyn ProjectStore) -> DaemonResult<()>;
}

/// Every migration that ships, in the order they must run.
pub fn all_migrations() -> Vec<Box<dyn Migration>> {
    vec![Box::new(Migration001ProjectRegistry)]
}

fn marker_path(home: &Path) -> PathBuf {
    home.join(".migrations.json")
}

fn read_applied(home: &Path) -> Vec<String> {
    std::fs::read(marker_path(home))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_applied(home: &Path, applied: &[String]) {
    if let Ok(bytes) = serde_json::to_vec_pretty(applied) {
        let _ = std::fs::write(marker_path(home), bytes);
    }
}

/// Run every not-yet-applied migration, in order, recording each as applied
/// only after it returns `Ok`. Never panics or aborts the daemon on a
/// migration failure — logs and moves on, so one bad migration can't block
/// startup (each migration is independently safe to retry next launch).
pub fn run_all(home: &Path, projects: &dyn ProjectStore) {
    let mut applied = read_applied(home);
    for migration in all_migrations() {
        if applied.iter().any(|id| id == migration.id()) {
            continue;
        }
        match migration.run(home, projects) {
            Ok(()) => {
                applied.push(migration.id().to_string());
                write_applied(home, &applied);
            }
            Err(e) => {
                tracing::warn!("migration '{}' failed: {e} — will retry next startup", migration.id());
            }
        }
    }
}
