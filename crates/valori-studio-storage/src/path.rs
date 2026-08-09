// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Resolves where `studio.redb` lives on disk.
//!
//! # Why this duplicates `valori_daemon::default_home()` instead of calling it
//!
//! `valori-studio-storage` must stay leaf-ward (it may depend only on
//! `valori-domain` — see `Cargo.toml` and
//! `crates/valori-node/tests/dependency_direction.rs`'s `SEALED_CRATES`).
//! Depending on `valori-daemon` just to reuse its four-line home-dir
//! resolver would pull the daemon's process-supervision, project-registry
//! and workspace code into the Studio storage crate for no reason, and
//! would violate the one-way `desktop/src-tauri → valori-studio-storage`
//! edge the architecture requires.
//!
//! So this function is a **deliberate, intentional duplicate** of
//! `valori_daemon::default_home()`'s resolution rule: `$VALORI_HOME`, else
//! `$HOME` (Unix/macOS) or `$USERPROFILE` (Windows) + `.valori`. Both
//! Studio's `studio.redb` and the daemon's `metadata.redb` /
//! `projects/<name>/` must resolve to the **same** `~/.valori` root — a
//! user who sets `VALORI_HOME` expects every Valori file to move with it.
//! If this rule ever changes, both copies must change together; there are
//! exactly two places this logic is allowed to exist, and both name the
//! other in a doc comment.

use std::path::{Path, PathBuf};

use valori_domain::ModelId;

/// The database's file name inside the Valori home directory.
pub const STUDIO_DB_FILENAME: &str = "studio.redb";

/// `$VALORI_HOME`, or `$HOME`/`$USERPROFILE` + `.valori` — the same root
/// `valori-daemon`, `valori-metadata`, and `valori-cli` resolve to. See the
/// module doc for why this is a duplicate, not a shared dependency.
///
/// Does not create the directory — callers that need it to exist (i.e.
/// [`StudioDatabase::open_default`](crate::StudioDatabase::open_default))
/// are responsible for that.
pub fn default_home_dir() -> PathBuf {
    if let Ok(h) = std::env::var("VALORI_HOME") {
        return PathBuf::from(h);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".valori")
}

/// `default_home_dir()` joined with `studio.redb` — the default path
/// [`StudioDatabase::open_default`](crate::StudioDatabase::open_default) opens.
///
/// Delegates to [`StudioPaths`] (S6) so there is exactly one definition of
/// the canonical layout; kept as a free function for every pre-S6 call
/// site (`db.rs`, `recovery.rs`, and their tests) so none of them needed
/// to change.
pub fn default_db_path() -> PathBuf {
    StudioPaths::from_env().studio_db()
}

/// Where rolling `studio.redb` backups live — `$VALORI_HOME/backups/`.
/// Shared with nothing else; see `crate::recovery` module docs. Matches
/// the canonical `$VALORI_HOME/{studio.redb,projects,models,logs,crashes,
/// cache,downloads}` layout — `backups/` is Studio's own subdirectory
/// within it, not a sibling of `$VALORI_HOME` itself.
pub fn default_backups_dir() -> PathBuf {
    StudioPaths::from_env().backups_dir()
}

/// The durable recovery-event log — `$VALORI_HOME/studio-recovery.jsonl`.
/// Deliberately a **sibling of** `studio.redb`, not a table inside it: a
/// corruption event that destroys `studio.redb` must not also destroy the
/// record that corruption happened. See `crate::recovery` module docs.
pub fn default_recovery_log_path() -> PathBuf {
    StudioPaths::from_env().recovery_log_path()
}

// ── StudioPaths (S6 — Desktop Filesystem Consolidation) ────────────────────
//
// The single canonical resolver for every path under `$VALORI_HOME`. See
// `docs/reviews/studio-filesystem-audit.md` for the audit that preceded
// this, and `docs/phases/phase-studio-S6-filesystem-management.md` for the
// full rationale. `StudioPaths` resolves locations; it never touches the
// filesystem itself (no `create_dir_all`, no reads, no writes) — that is
// `desktop/src-tauri`'s `FileSystemService`'s job (kept in the desktop
// crate deliberately: `valori-studio-storage` must stay leaf-ward and has
// no reason to depend on `tauri` or perform I/O policy decisions like
// atomic-write semantics). This module answers "where," never "how."

/// Directory names for the canonical layout under `$VALORI_HOME`. Centralized
/// so `StudioPaths` and any test/tool that needs the same literal never
/// drift apart.
pub const BACKUPS_DIR: &str = "backups";
pub const PROJECTS_DIR: &str = "projects";
pub const MODELS_DIR: &str = "models";
pub const LOGS_DIR: &str = "logs";
pub const CRASHES_DIR: &str = "crashes";
pub const CACHE_DIR: &str = "cache";
pub const DOWNLOADS_DIR: &str = "downloads";
pub const TEMP_DIR: &str = "temp";

/// Resolves every canonical Studio filesystem location from one root.
///
/// # What this does and does not own
///
/// - **Owns path resolution only.** No method here creates a directory,
///   opens a file, or touches the filesystem in any way — see the module
///   doc above.
/// - **`projects_dir()`/`project_dir()`/`models_dir()`/`model_dir()`
///   describe the *default* layout** (no user override). They are correct
///   whenever `StudioPaths::root()` and the daemon's actual `VALORI_HOME`
///   coincide — the common case — but **not authoritative** once a user
///   sets a custom `workspaceDir` preference, which becomes the spawned
///   daemon's own `VALORI_HOME` and can diverge from Studio's root. See
///   `docs/reviews/studio-filesystem-audit.md` §5. `StudioPaths` has no
///   access to that preference (it lives in `studio.redb`, resolved by the
///   caller, not this crate) and does not attempt to guess it.
/// - **Does not own project internals.** `project_dir(name)` returns the
///   directory a local project lives in — never the WAL, snapshot, index,
///   or vector files inside it. Those remain entirely
///   `valori-kernel`/`valori-storage`/`valori-node`'s concern; this crate
///   has no type or method that names them.
/// - **Does not own `metadata.redb`.** Confirmed unwired in production by
///   the S4 and S6 audits; not part of this resolver's surface.
/// - **`crashes_dir()` is the *new* canonical location for future
///   Studio-owned crash-adjacent files.** The existing panic-hook crash
///   marker (`desktop/src-tauri/src/telemetry.rs`) deliberately still uses
///   Tauri's `app_config_dir()`, not this path — see the filesystem
///   audit's §4 for why that is a permanent, documented exception, not an
///   oversight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StudioPaths {
    root: PathBuf,
}

impl StudioPaths {
    /// Resolves from the process environment — `$VALORI_HOME`, or
    /// `$HOME`/`$USERPROFILE` + `.valori`. Equivalent to
    /// `StudioPaths::new(default_home_dir())`.
    pub fn from_env() -> Self {
        Self {
            root: default_home_dir(),
        }
    }

    /// Wraps an explicit root — for tests, or any caller that has already
    /// resolved (or overridden) the Studio home directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn studio_db(&self) -> PathBuf {
        self.root.join(STUDIO_DB_FILENAME)
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.root.join(BACKUPS_DIR)
    }

    /// A **sibling of** `studio_db()`, not `recovery_dir().join(...)` — see
    /// this module's doc comment and the filesystem audit's §4 for why.
    pub fn recovery_log_path(&self) -> PathBuf {
        self.root.join("studio-recovery.jsonl")
    }

    /// Default (no-override) projects root — see this type's doc comment.
    pub fn projects_dir(&self) -> PathBuf {
        self.root.join(PROJECTS_DIR)
    }

    /// A local project's directory, keyed by **name** — matching
    /// `valori-daemon::JsonProjectStore`'s actual on-disk convention, not
    /// `ProjectId`. A project's stable identity is `ProjectId`, but its
    /// directory has always been name-keyed (predates `ProjectId`'s
    /// existence — see `docs/architecture/ownership.md`); resolving
    /// `ProjectId → name` requires the project registry, which this
    /// leaf-ward, dependency-light resolver deliberately does not have
    /// access to. Callers that have a `ProjectId` and need its directory
    /// must resolve the name first, through the registry.
    pub fn project_dir(&self, name: &str) -> PathBuf {
        self.projects_dir().join(name)
    }

    /// Default (no-override) models root — Node-owned (`valori-models`,
    /// wired into `valori-node`), not Studio-owned. Provided for path
    /// resolution and display; Studio itself never reads or writes here.
    pub fn models_dir(&self) -> PathBuf {
        self.root.join(MODELS_DIR)
    }

    /// A model's directory. Mirrors `valori-models::sanitize()`'s
    /// character-replacement rule exactly (alphanumeric, `-`, `.` pass
    /// through; everything else becomes `_`) — a **deliberate duplicate**,
    /// the same pattern `default_home_dir()` already uses for
    /// `valori_daemon::default_home()`, for the same reason: this crate
    /// must not depend on `valori-models`.
    pub fn model_dir(&self, model: &ModelId) -> PathBuf {
        self.models_dir().join(sanitize_model_id(model.as_str()))
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join(LOGS_DIR)
    }

    /// The new canonical crash-file location — not the same path the
    /// existing panic-hook marker uses today. See this type's doc comment.
    pub fn crashes_dir(&self) -> PathBuf {
        self.root.join(CRASHES_DIR)
    }

    /// Explicitly disposable — anything here must survive being deleted
    /// wholesale with the application still working. See
    /// `docs/phases/phase-studio-S6-filesystem-management.md`'s cache
    /// ownership section.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(CACHE_DIR)
    }

    /// Staging area for in-progress downloads — never an installed
    /// artifact's final resting place. See the same phase doc's downloads
    /// section for the stage → verify → atomic-move contract.
    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join(DOWNLOADS_DIR)
    }

    /// Studio-owned scratch space, safe to clean up on startup — never a
    /// location any other component treats as durable.
    pub fn temp_dir(&self) -> PathBuf {
        self.root.join(TEMP_DIR)
    }
}

/// Mirrors `valori-models`'s private `sanitize()` exactly — see
/// `StudioPaths::model_dir`'s doc comment for why this is a deliberate
/// duplicate rather than a shared dependency.
fn sanitize_model_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // std::env::set_var races across parallel tests in the same process;
    // serialize the handful of tests that touch VALORI_HOME/HOME.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn respects_valori_home_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("VALORI_HOME").ok();
        unsafe {
            std::env::set_var("VALORI_HOME", "/tmp/valori-studio-storage-test-home");
        }

        assert_eq!(
            default_home_dir(),
            PathBuf::from("/tmp/valori-studio-storage-test-home")
        );
        assert_eq!(
            default_db_path(),
            PathBuf::from("/tmp/valori-studio-storage-test-home/studio.redb")
        );

        unsafe {
            match prev {
                Some(v) => std::env::set_var("VALORI_HOME", v),
                None => std::env::remove_var("VALORI_HOME"),
            }
        }
    }

    #[test]
    fn falls_back_to_home_dot_valori() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_vh = std::env::var("VALORI_HOME").ok();
        let prev_home = std::env::var("HOME").ok();
        unsafe {
            std::env::remove_var("VALORI_HOME");
            std::env::set_var("HOME", "/tmp/valori-studio-storage-fake-home");
        }

        assert_eq!(
            default_home_dir(),
            PathBuf::from("/tmp/valori-studio-storage-fake-home/.valori")
        );

        unsafe {
            match prev_vh {
                Some(v) => std::env::set_var("VALORI_HOME", v),
                None => std::env::remove_var("VALORI_HOME"),
            }
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    // ── StudioPaths (S6) ─────────────────────────────────────────────────

    #[test]
    fn studio_paths_from_env_respects_valori_home() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("VALORI_HOME").ok();
        unsafe {
            std::env::set_var("VALORI_HOME", "/tmp/valori-studio-paths-test");
        }

        let paths = StudioPaths::from_env();
        assert_eq!(paths.root(), Path::new("/tmp/valori-studio-paths-test"));

        unsafe {
            match prev {
                Some(v) => std::env::set_var("VALORI_HOME", v),
                None => std::env::remove_var("VALORI_HOME"),
            }
        }
    }

    #[test]
    fn studio_paths_from_env_falls_back_to_home_dot_valori() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_vh = std::env::var("VALORI_HOME").ok();
        let prev_home = std::env::var("HOME").ok();
        unsafe {
            std::env::remove_var("VALORI_HOME");
            std::env::set_var("HOME", "/tmp/valori-studio-paths-fake-home");
        }

        let paths = StudioPaths::from_env();
        assert_eq!(
            paths.root(),
            Path::new("/tmp/valori-studio-paths-fake-home/.valori")
        );

        unsafe {
            match prev_vh {
                Some(v) => std::env::set_var("VALORI_HOME", v),
                None => std::env::remove_var("VALORI_HOME"),
            }
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn studio_paths_new_accepts_a_custom_root_a_k_a_a_custom_workspace() {
        // Mirrors the real precedence: a user-configured `workspaceDir`
        // preference wins over any environment default — a caller that has
        // already resolved that preference constructs `StudioPaths::new`
        // with it directly, bypassing `from_env()` entirely.
        let paths = StudioPaths::new("/Users/demo/my-custom-workspace");
        assert_eq!(paths.root(), Path::new("/Users/demo/my-custom-workspace"));
        assert_eq!(
            paths.projects_dir(),
            Path::new("/Users/demo/my-custom-workspace/projects")
        );
    }

    #[test]
    fn every_accessor_resolves_directly_under_root() {
        let paths = StudioPaths::new("/tmp/valori-root");
        assert_eq!(paths.studio_db(), Path::new("/tmp/valori-root/studio.redb"));
        assert_eq!(paths.backups_dir(), Path::new("/tmp/valori-root/backups"));
        assert_eq!(
            paths.recovery_log_path(),
            Path::new("/tmp/valori-root/studio-recovery.jsonl"),
            "recovery log is a sibling of studio.redb, not inside a recovery/ subdirectory"
        );
        assert_eq!(paths.projects_dir(), Path::new("/tmp/valori-root/projects"));
        assert_eq!(paths.models_dir(), Path::new("/tmp/valori-root/models"));
        assert_eq!(paths.logs_dir(), Path::new("/tmp/valori-root/logs"));
        assert_eq!(paths.crashes_dir(), Path::new("/tmp/valori-root/crashes"));
        assert_eq!(paths.cache_dir(), Path::new("/tmp/valori-root/cache"));
        assert_eq!(
            paths.downloads_dir(),
            Path::new("/tmp/valori-root/downloads")
        );
        assert_eq!(paths.temp_dir(), Path::new("/tmp/valori-root/temp"));
    }

    #[test]
    fn project_dir_is_keyed_by_name_not_project_id() {
        let paths = StudioPaths::new("/tmp/valori-root");
        assert_eq!(
            paths.project_dir("my-project"),
            Path::new("/tmp/valori-root/projects/my-project")
        );
    }

    #[test]
    fn model_dir_sanitizes_the_model_id_exactly_like_valori_models_does() {
        let paths = StudioPaths::new("/tmp/valori-root");
        let model = ModelId::parse("openai/text-embedding-3-small").unwrap();
        assert_eq!(
            paths.model_dir(&model),
            Path::new("/tmp/valori-root/models/openai_text-embedding-3-small"),
            "must match valori-models::sanitize()'s exact character-replacement rule"
        );
    }

    #[test]
    fn default_free_functions_agree_with_studio_paths_from_env() {
        // The pre-S6 free functions now delegate to StudioPaths — this
        // pins that they still produce byte-identical output, so every
        // existing caller (db.rs, recovery.rs, and their tests) is
        // unaffected.
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("VALORI_HOME").ok();
        unsafe {
            std::env::set_var("VALORI_HOME", "/tmp/valori-studio-paths-parity-test");
        }

        let paths = StudioPaths::from_env();
        assert_eq!(default_db_path(), paths.studio_db());
        assert_eq!(default_backups_dir(), paths.backups_dir());
        assert_eq!(default_recovery_log_path(), paths.recovery_log_path());

        unsafe {
            match prev {
                Some(v) => std::env::set_var("VALORI_HOME", v),
                None => std::env::remove_var("VALORI_HOME"),
            }
        }
    }
}
