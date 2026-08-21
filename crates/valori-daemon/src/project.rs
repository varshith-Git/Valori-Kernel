// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Project registry — the filesystem-backed catalog of projects.
//!
//! Rust port of `ui/src/lib/server/projects.ts`. A **project** is a directory
//! under `<home>/projects/<name>/` holding a `project.json` manifest plus the
//! per-project data (`events.log`, snapshots, indexes). One project maps to one
//! `valori-node` instance (RFC-0006: Supervised mode).
//!
//! This module owns *persistence and layout only* — starting/stopping the node
//! is the [`crate::supervisor::Supervisor`]'s job.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{DaemonError, DaemonResult};

/// Cluster topology (RFC-0006 Phase B.0) — **persisted only**. `ui/`'s
/// `ProjectEntry.replication`/`nodes`/`shardCount` land here so the schema is
/// a complete superset before the lifecycle routes migrate; no cluster launch
/// behavior is implemented yet (`replication` is always 1 in practice —
/// `LocalRuntime` only ever starts a single node per project). A later phase
/// teaches `Runtime`/`Supervisor` to actually honor `replication == 3` without
/// another manifest migration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClusterConfig {
    /// 1 (single node) or 3 (Raft cluster).
    pub replication: u8,
    /// Length matches `replication`, ordered by `id` ascending.
    #[serde(default)]
    pub nodes: Vec<ProjectNode>,
    /// Independent shards (Raft groups) per node. Cluster-only; meaningless
    /// when `replication == 1`.
    #[serde(default = "default_shard_count")]
    pub shard_count: u32,
}

fn default_shard_count() -> u32 {
    1
}

/// One node's ports within a [`ClusterConfig`] — persisted only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectNode {
    /// Raft-semantic id, unique within the project (1, 2, 3).
    pub id: u32,
    pub http_port: u16,
    /// Present only when the project is a cluster (`replication > 1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raft_port: Option<u16>,
}

/// Per-project embedding provider config (RFC-0006 Phase B.0) — **persisted
/// only**; nothing in `valori-ingest`/`valori-node` reads this yet.
///
/// `api_key_ref` deliberately holds a *reference* (env var name, keychain
/// entry id, etc.), never the raw secret — the manifest file is plain JSON
/// on disk, unlike `ui/`'s current `ProjectEntry.embed.apiKey`.
///
/// **Studio S3 compatibility note** (`docs/phases/phase-studio-S3-credentials.md`):
/// `valori_domain::CredentialRef`'s wire form is a bare UUID string
/// (`#[serde(transparent)]`), so `credential_ref.to_string()` is a valid
/// `api_key_ref` value with no adapter and no field rename — this field's
/// type intentionally stays `Option<String>`, not `Option<CredentialRef>`,
/// so existing `project.json` manifests are unaffected. Nothing currently
/// populates this field from the desktop credential-storage flow (S3
/// deliberately did not wire project creation to it — see that phase doc's
/// "deferred" section); it remains schema-complete, not behavior-complete.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EmbeddingConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_ref: Option<String>,
}

/// Storage-related options (RFC-0006 Phase B.0) — **persisted only**.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageConfig {
    #[serde(default = "default_max_records")]
    pub max_records: usize,
    /// Whether this project's data files get the immutable/read-only
    /// "at rest" protection `ui/`'s `protect()`/`unprotect()` already apply
    /// (`chflags uchg` / `0o400`) when the project is stopped. Not enforced
    /// by the daemon yet.
    #[serde(default)]
    pub protect_at_rest: bool,
}

fn default_max_records() -> usize {
    1_000_000
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            max_records: default_max_records(),
            protect_at_rest: false,
        }
    }
}

/// Persisted per-project manifest (`project.json`) — the canonical
/// description of a project's identity, topology, and configuration.
///
/// Renamed from `ProjectConfig` (RFC-0006 Phase B.0): this is no longer just
/// "config", it's the full manifest that `ui/`'s `ProjectEntry`
/// (`ui/src/lib/server/projects.ts`) used to be the sole source of truth for.
/// `cluster`, `embedding`, and parts of `storage` are **persisted only** —
/// schema-complete so the lifecycle routes can migrate in one pass, with no
/// behavior behind those fields yet. Later phases (cluster launch, embedding-
/// driven ingest) consume them without another manifest migration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectManifest {
    /// Stable id (UUID). Never changes; `name` is a mutable label.
    ///
    /// Defaults to the **empty string**, not a fresh UUID. That distinction is
    /// load-bearing: `#[serde(default = "crate::new_id")]` made a manifest
    /// written before this field existed mint a *different* id on every read,
    /// because `get()` never wrote the value back. An empty id therefore means
    /// "not yet assigned", and [`JsonProjectStore::get`] backfills it exactly
    /// once. See `docs/reviews/m2-project-review.md` finding F3.
    ///
    /// Never empty once a manifest has been read through the store.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// Legacy vector dimension (optional; vector config is now Collection-scoped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dim: Option<usize>,
    /// Legacy index kind (optional; index is now Collection-scoped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
    /// Owning workspace (RFC-0006). Defaults to `default` for older manifests.
    #[serde(default = "default_workspace")]
    pub workspace: String,
    /// Auto-restart policy (operational). Defaults to `never` (no auto-restart).
    #[serde(default)]
    pub restart_policy: crate::policy::RestartPolicy,
    /// Unix seconds at creation.
    pub created_at: u64,
    /// Unix seconds this project was last started, if ever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_opened_at: Option<u64>,
    /// Cluster topology. `None` = single node — today's only real behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster: Option<ClusterConfig>,
    /// Embedding provider config.
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    /// Storage options.
    #[serde(default)]
    pub storage: StorageConfig,
}

fn default_index() -> String {
    "brute".to_string()
}

fn default_workspace() -> String {
    crate::workspace::DEFAULT_WORKSPACE.to_string()
}

/// A project on disk: its manifest plus resolved paths.
#[derive(Debug, Clone)]
pub struct Project {
    pub config: ProjectManifest,
    pub dir: PathBuf,
}

impl Project {
    /// Durable event log — the source of truth; the node replays it on start.
    pub fn event_log_path(&self) -> PathBuf {
        self.dir.join("events.log")
    }
    /// Snapshot file for fast restore.
    pub fn snapshot_path(&self) -> PathBuf {
        self.dir.join("snapshot.val")
    }

    /// Per-node event log path for a cluster project. Naming (`events-n{id}.log`,
    /// flat in the project dir) matches `projectNodePaths()` in
    /// `ui/src/lib/server/projects.ts` byte-for-byte — this is load-bearing:
    /// cluster projects created through the pre-daemon (`process-manager.ts`)
    /// path already have data on disk at these exact paths, and diverging here
    /// would silently orphan it on first daemon-launched start.
    pub fn node_event_log_path(&self, id: u32) -> PathBuf {
        self.dir.join(format!("events-n{id}.log"))
    }
    /// Per-node snapshot path for a cluster project. See `node_event_log_path`.
    pub fn node_snapshot_path(&self, id: u32) -> PathBuf {
        self.dir.join(format!("current-n{id}.snap"))
    }
    /// Per-node Raft log (redb) path for a cluster project. See `node_event_log_path`.
    pub fn node_raft_log_path(&self, id: u32) -> PathBuf {
        self.dir.join(format!("raft-n{id}.redb"))
    }
    /// Per-node launcher log (stdout+stderr capture) for a cluster project. No
    /// pre-existing convention to match — the legacy `pm`-based path only kept
    /// logs in memory, never on disk.
    pub fn node_log_path(&self, id: u32) -> PathBuf {
        self.dir.join(format!("node-n{id}.log"))
    }
}

/// Filesystem-backed [`ProjectStore`](crate::store::ProjectStore) rooted at
/// `<home>/projects/`. One `project.json` manifest per project directory.
pub struct JsonProjectStore {
    projects_root: PathBuf,
}

impl JsonProjectStore {
    /// `home` is the daemon data root (e.g. `~/.valori`). Projects live under
    /// `home/projects/`. The directory is created if missing.
    pub fn new(home: impl AsRef<Path>) -> DaemonResult<Self> {
        let projects_root = home.as_ref().join("projects");
        std::fs::create_dir_all(&projects_root)?;
        Ok(Self { projects_root })
    }

    fn manifest_path(&self, name: &str) -> PathBuf {
        self.projects_root.join(name).join("project.json")
    }

    /// Valid project name: non-empty, filesystem-safe, no path traversal.
    pub fn is_valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 64
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }

    /// Assign and persist a stable id for a manifest written before the `id`
    /// field existed. Idempotent: a manifest that already has one is untouched.
    ///
    /// # Why this is a read-path repair
    ///
    /// A legacy `project.json` has no `id`. Minting one per read — the previous
    /// behaviour — produced a different identity every time, which makes the id
    /// useless for correlating a project across the daemon, the control plane
    /// and Cloud. The id is therefore minted once and written back immediately.
    ///
    /// # The new id is random, never derived
    ///
    /// It is a fresh UUID v4. It is deliberately **not** derived from the
    /// project's name or directory path: both are mutable, and deriving
    /// identity from either would silently change the id on rename or move —
    /// exactly the property this repair exists to provide.
    ///
    /// # If the manifest cannot be written
    ///
    /// The project keeps the freshly minted id for this process, and a warning
    /// is logged. Identity remains unstable until the manifest becomes
    /// writable (a project at rest may be `chflags uchg`-protected). Returning
    /// an error instead would make such a project unlistable, which is a worse
    /// failure — but the condition is never silent.
    fn backfill_id(&self, project: &mut Project) {
        if !project.config.id.is_empty() {
            return;
        }
        project.config.id = crate::new_id();
        if let Err(e) = self.write_manifest(project) {
            tracing::warn!(
                project = %project.config.name,
                error = %e,
                "could not persist a backfilled project id; the id will not be \
                 stable across restarts until the manifest is writable"
            );
        } else {
            tracing::info!(
                project = %project.config.name,
                id = %project.config.id,
                "assigned a stable id to a legacy project manifest"
            );
        }
    }

    fn write_manifest(&self, project: &Project) -> DaemonResult<()> {
        let path = self.manifest_path(&project.config.name);
        // write-then-fsync-then-rename (S6 — Desktop Filesystem
        // Consolidation) so a crash mid-write never leaves a half file, and
        // an fsync-less power loss between the rename and the OS actually
        // flushing the page cache can't leave `path` truncated either.
        // Same write-temp/fsync/atomic-rename contract
        // `desktop/src-tauri`'s `FileSystemService::atomic_write` documents
        // — kept as a local, dependency-free implementation here rather
        // than depending on the Studio-side service, since `valori-daemon`
        // runs as its own independent process (see
        // `docs/architecture/control-plane.md`) and must not depend on
        // `desktop/src-tauri`.
        let tmp = path.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(&serde_json::to_vec_pretty(&project.config)?)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}

impl crate::store::ProjectStore for JsonProjectStore {
    fn create(&self, config: ProjectManifest) -> DaemonResult<Project> {
        if !Self::is_valid_name(&config.name) {
            return Err(DaemonError::InvalidInput(format!(
                "invalid project name '{}': use letters, digits, '-' or '_' (<=64 chars)",
                config.name
            )));
        }
        if config.dim == Some(0) {
            return Err(DaemonError::InvalidInput("dim must be > 0".into()));
        }
        let dir = self.projects_root.join(&config.name);
        if dir.exists() {
            return Err(DaemonError::AlreadyExists(config.name.clone()));
        }
        std::fs::create_dir_all(&dir)?;
        let mut project = Project { config, dir };
        if project.config.id.is_empty() {
            project.config.id = crate::new_id();
        }
        self.write_manifest(&project)?;
        Ok(project)
    }

    fn get(&self, name: &str) -> DaemonResult<Project> {
        let manifest = self.manifest_path(name);
        let bytes =
            std::fs::read(&manifest).map_err(|_| DaemonError::NotFound(name.to_string()))?;
        let config: ProjectManifest = serde_json::from_slice(&bytes)?;
        let mut project = Project {
            config,
            dir: self.projects_root.join(name),
        };
        self.backfill_id(&mut project);
        Ok(project)
    }

    fn import(&self, config: ProjectManifest) -> DaemonResult<Project> {
        if !Self::is_valid_name(&config.name) {
            return Err(DaemonError::InvalidInput(format!(
                "invalid project name '{}': use letters, digits, '-' or '_' (<=64 chars)",
                config.name
            )));
        }
        let dir = self.projects_root.join(&config.name);
        std::fs::create_dir_all(&dir)?;
        let mut project = Project { config, dir };
        if project.config.id.is_empty() {
            project.config.id = crate::new_id();
        }
        self.write_manifest(&project)?;
        Ok(project)
    }

    fn list(&self) -> DaemonResult<Vec<Project>> {
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&self.projects_root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    if let Ok(project) = self.get(name) {
                        out.push(project);
                    }
                }
            }
        }
        out.sort_by(|a, b| a.config.name.cmp(&b.config.name));
        Ok(out)
    }

    fn delete(&self, name: &str) -> DaemonResult<()> {
        let dir = self.projects_root.join(name);
        if !dir.exists() {
            return Err(DaemonError::NotFound(name.to_string()));
        }
        clear_immutable(&dir);
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    fn rename(&self, old_name: &str, new_name: &str) -> DaemonResult<Project> {
        if !Self::is_valid_name(new_name) {
            return Err(DaemonError::InvalidInput(format!(
                "invalid project name '{}': use letters, digits, '-' or '_' (<=64 chars)",
                new_name
            )));
        }
        let old_dir = self.projects_root.join(old_name);
        if !old_dir.exists() {
            return Err(DaemonError::NotFound(old_name.to_string()));
        }
        let new_dir = self.projects_root.join(new_name);
        if new_dir.exists() {
            return Err(DaemonError::AlreadyExists(new_name.to_string()));
        }
        std::fs::rename(&old_dir, &new_dir)?;
        // Re-read the manifest from the new location and update the name field.
        let manifest_path = new_dir.join("project.json");
        let bytes = std::fs::read(&manifest_path)
            .map_err(|_| DaemonError::NotFound(old_name.to_string()))?;
        let mut config: ProjectManifest = serde_json::from_slice(&bytes)?;
        config.name = new_name.to_string();
        let project = Project {
            config,
            dir: new_dir,
        };
        self.write_manifest(&project)?;
        Ok(project)
    }
}

/// Clear the "at rest" immutable flag `ui/`'s `protect()`/`unprotect()`
/// (`ui/src/lib/server/projects.ts`) sets on data files while a project is
/// closed/stopped (`chflags uchg`, macOS only). Without this,
/// `remove_dir_all` fails with `EPERM` on any file still flagged, surfacing
/// as a 500 on delete. Best-effort — deletion should proceed even if this
/// fails (e.g. the flag was never set, or we're not on macOS).
fn clear_immutable(dir: &Path) {
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("chflags")
            .args(["-R", "nouchg"])
            .arg(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProjectStore;

    fn cfg(name: &str) -> ProjectManifest {
        ProjectManifest {
            id: crate::new_id(),
            name: name.into(),
            dim: Some(128),
            index: Some("brute".into()),
            workspace: "default".into(),
            restart_policy: crate::policy::RestartPolicy::Never,
            created_at: 0,
            last_opened_at: None,
            cluster: None,
            embedding: EmbeddingConfig::default(),
            storage: StorageConfig::default(),
        }
    }

    // ── F3: stable project identity ───────────────────────────────────────────
    //
    // A manifest written before the `id` field existed must be assigned an id
    // exactly once, and must keep it forever after. See `backfill_id`.

    /// Write a manifest with no `id` key at all — the legacy on-disk shape.
    fn write_legacy_manifest(home: &Path, name: &str) {
        let dir = home.join("projects").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let legacy = serde_json::json!({
            "name": name,
            "dim": 128,
            "index": "brute",
            "workspace": "default",
            "created_at": 1_750_000_000u64,
        });
        std::fs::write(
            dir.join("project.json"),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn legacy_manifest_without_id_gets_one_stable_id() {
        let home = tempfile::tempdir().unwrap();
        write_legacy_manifest(home.path(), "legacy");
        let pm = JsonProjectStore::new(home.path()).unwrap();

        let first = pm.get("legacy").unwrap().config.id;
        assert!(!first.is_empty(), "an id must be assigned on first load");
        assert!(
            uuid::Uuid::parse_str(&first).is_ok(),
            "the assigned id must be a UUID, not a derived value: {first}"
        );

        let second = pm.get("legacy").unwrap().config.id;
        assert_eq!(
            first, second,
            "the id must be persisted on first load, not minted on every read"
        );
    }

    #[test]
    fn backfilled_id_survives_a_fresh_store_instance() {
        // Stands in for restarting the application: a brand-new store object
        // reading the same directory must see the same identity.
        let home = tempfile::tempdir().unwrap();
        write_legacy_manifest(home.path(), "legacy");

        let first = JsonProjectStore::new(home.path())
            .unwrap()
            .get("legacy")
            .unwrap()
            .config
            .id;
        let after_restart = JsonProjectStore::new(home.path())
            .unwrap()
            .get("legacy")
            .unwrap()
            .config
            .id;

        assert_eq!(first, after_restart);
    }

    #[test]
    fn backfilled_id_is_written_to_disk() {
        let home = tempfile::tempdir().unwrap();
        write_legacy_manifest(home.path(), "legacy");
        let pm = JsonProjectStore::new(home.path()).unwrap();

        let id = pm.get("legacy").unwrap().config.id;
        let raw =
            std::fs::read_to_string(home.path().join("projects/legacy/project.json")).unwrap();
        let on_disk: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            on_disk["id"].as_str(),
            Some(id.as_str()),
            "the id must be persisted, not held only in memory"
        );
    }

    #[test]
    fn existing_id_is_never_reassigned() {
        let home = tempfile::tempdir().unwrap();
        let pm = JsonProjectStore::new(home.path()).unwrap();
        let created = pm.create(cfg("healthcare")).unwrap().config.id;

        for _ in 0..3 {
            assert_eq!(pm.get("healthcare").unwrap().config.id, created);
        }
    }

    #[test]
    fn identity_is_not_derived_from_the_project_name() {
        // Two projects created with identical settings but different names must
        // get different ids; and the id must not be a function of the name.
        let home = tempfile::tempdir().unwrap();
        write_legacy_manifest(home.path(), "alpha");
        write_legacy_manifest(home.path(), "beta");
        let pm = JsonProjectStore::new(home.path()).unwrap();

        assert_ne!(
            pm.get("alpha").unwrap().config.id,
            pm.get("beta").unwrap().config.id
        );

        // Same name, different store root => different id. If identity were
        // derived from the name these would collide.
        let other_home = tempfile::tempdir().unwrap();
        write_legacy_manifest(other_home.path(), "alpha");
        let other = JsonProjectStore::new(other_home.path()).unwrap();
        assert_ne!(
            pm.get("alpha").unwrap().config.id,
            other.get("alpha").unwrap().config.id
        );
    }

    #[test]
    fn renaming_a_project_directory_preserves_the_id() {
        // A rename today is a directory move plus a manifest `name` change.
        // The id lives in the manifest, so it must survive both.
        let home = tempfile::tempdir().unwrap();
        write_legacy_manifest(home.path(), "before");
        let pm = JsonProjectStore::new(home.path()).unwrap();
        let id = pm.get("before").unwrap().config.id;

        let projects = home.path().join("projects");
        std::fs::rename(projects.join("before"), projects.join("after")).unwrap();
        let path = projects.join("after/project.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        manifest["name"] = serde_json::json!("after");
        std::fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

        assert_eq!(pm.get("after").unwrap().config.id, id);
    }

    #[test]
    fn moving_a_project_to_another_root_preserves_the_id() {
        // The supported migration path: copy the project directory, manifest
        // included. Identity travels with the manifest, not with the path.
        let home = tempfile::tempdir().unwrap();
        write_legacy_manifest(home.path(), "movable");
        let pm = JsonProjectStore::new(home.path()).unwrap();
        let id = pm.get("movable").unwrap().config.id;

        let new_home = tempfile::tempdir().unwrap();
        let dest = new_home.path().join("projects/movable");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::copy(
            home.path().join("projects/movable/project.json"),
            dest.join("project.json"),
        )
        .unwrap();

        let moved = JsonProjectStore::new(new_home.path()).unwrap();
        assert_eq!(moved.get("movable").unwrap().config.id, id);
    }

    #[test]
    fn list_sees_the_same_ids_as_get() {
        let home = tempfile::tempdir().unwrap();
        write_legacy_manifest(home.path(), "legacy");
        let pm = JsonProjectStore::new(home.path()).unwrap();

        let via_list = pm.list().unwrap().remove(0).config.id;
        let via_get = pm.get("legacy").unwrap().config.id;
        assert_eq!(via_list, via_get);
        assert!(!via_list.is_empty());
    }

    #[test]
    fn create_list_get_delete() {
        let home = tempfile::tempdir().unwrap();
        let pm = JsonProjectStore::new(home.path()).unwrap();

        assert!(pm.list().unwrap().is_empty());
        pm.create(cfg("healthcare")).unwrap();
        pm.create(cfg("finance")).unwrap();

        let names: Vec<_> = pm
            .list()
            .unwrap()
            .into_iter()
            .map(|p| p.config.name)
            .collect();
        assert_eq!(names, vec!["finance", "healthcare"]); // sorted

        let hc = pm.get("healthcare").unwrap();
        assert_eq!(hc.config.dim, Some(128));
        assert!(hc.event_log_path().ends_with("healthcare/events.log"));

        pm.delete("healthcare").unwrap();
        assert!(pm.get("healthcare").is_err());
        assert_eq!(pm.list().unwrap().len(), 1);
    }

    #[test]
    fn rejects_bad_names_and_duplicates() {
        let home = tempfile::tempdir().unwrap();
        let pm = JsonProjectStore::new(home.path()).unwrap();
        assert!(pm.create(cfg("../escape")).is_err());
        assert!(pm.create(cfg("")).is_err());
        pm.create(cfg("ok")).unwrap();
        assert!(matches!(
            pm.create(cfg("ok")),
            Err(DaemonError::AlreadyExists(_))
        ));
    }

    #[test]
    fn manifest_survives_reload() {
        let home = tempfile::tempdir().unwrap();
        let created = {
            let pm = JsonProjectStore::new(home.path()).unwrap();
            pm.create(cfg("persist")).unwrap().config
        };
        let pm2 = JsonProjectStore::new(home.path()).unwrap();
        // id and all fields round-trip identically (stable id, not regenerated).
        assert_eq!(pm2.get("persist").unwrap().config, created);
    }

    /// A `project.json` written before Phase B.0 (no `last_opened_at`,
    /// `cluster`, `embedding`, `storage`) must still load, with the new
    /// fields defaulting to "single node, no embedding, 1M max records".
    #[test]
    fn legacy_manifest_without_new_fields_still_loads() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("projects").join("legacy");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("project.json"),
            r#"{"id":"abc","name":"legacy","dim":128,"index":"brute","workspace":"default","restart_policy":"never","created_at":0}"#,
        )
        .unwrap();

        let pm = JsonProjectStore::new(home.path()).unwrap();
        let project = pm.get("legacy").unwrap();
        assert_eq!(project.config.last_opened_at, None);
        assert_eq!(project.config.cluster, None);
        assert_eq!(project.config.embedding, EmbeddingConfig::default());
        assert_eq!(project.config.storage, StorageConfig::default());
    }

    /// A manifest with a full cluster topology + embedding config round-trips
    /// through write/read unchanged (schema-complete, even though nothing
    /// acts on these fields yet).
    #[test]
    fn cluster_and_embedding_fields_round_trip() {
        let home = tempfile::tempdir().unwrap();
        let pm = JsonProjectStore::new(home.path()).unwrap();
        let mut config = cfg("clustered");
        config.cluster = Some(ClusterConfig {
            replication: 3,
            nodes: vec![
                ProjectNode {
                    id: 1,
                    http_port: 4010,
                    raft_port: Some(4110),
                },
                ProjectNode {
                    id: 2,
                    http_port: 4011,
                    raft_port: Some(4111),
                },
                ProjectNode {
                    id: 3,
                    http_port: 4012,
                    raft_port: Some(4112),
                },
            ],
            shard_count: 2,
        });
        config.embedding = EmbeddingConfig {
            provider: Some("openai".into()),
            model: Some("text-embedding-3-small".into()),
            endpoint: None,
            api_key_ref: Some("env:OPENAI_API_KEY".into()),
        };
        config.storage.protect_at_rest = true;

        pm.create(config.clone()).unwrap();
        let reloaded = JsonProjectStore::new(home.path())
            .unwrap()
            .get("clustered")
            .unwrap()
            .config;
        assert_eq!(reloaded, config);
    }
}
