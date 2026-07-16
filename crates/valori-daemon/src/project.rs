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

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

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
        Self { max_records: default_max_records(), protect_at_rest: false }
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
    #[serde(default = "crate::new_id")]
    pub id: String,
    pub name: String,
    /// Vector dimension — immutable after first insert (maps to `VALORI_DIM`).
    pub dim: usize,
    /// Index kind: `brute` | `hnsw` | `ivf` | `bq` | `auto` (maps to `VALORI_INDEX`).
    #[serde(default = "default_index")]
    pub index: String,
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
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }

    fn write_manifest(&self, project: &Project) -> DaemonResult<()> {
        let path = self.manifest_path(&project.config.name);
        // write-then-rename so a crash mid-write never leaves a half file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&project.config)?)?;
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
        if config.dim == 0 {
            return Err(DaemonError::InvalidInput("dim must be > 0".into()));
        }
        let dir = self.projects_root.join(&config.name);
        if dir.exists() {
            return Err(DaemonError::AlreadyExists(config.name.clone()));
        }
        std::fs::create_dir_all(&dir)?;
        let project = Project { config, dir };
        self.write_manifest(&project)?;
        Ok(project)
    }

    fn get(&self, name: &str) -> DaemonResult<Project> {
        let manifest = self.manifest_path(name);
        let bytes = std::fs::read(&manifest)
            .map_err(|_| DaemonError::NotFound(name.to_string()))?;
        let config: ProjectManifest = serde_json::from_slice(&bytes)?;
        Ok(Project { config, dir: self.projects_root.join(name) })
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
        let project = Project { config, dir };
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
        let project = Project { config, dir: new_dir };
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
            dim: 128,
            index: "brute".into(),
            workspace: "default".into(),
            restart_policy: crate::policy::RestartPolicy::Never,
            created_at: 0,
            last_opened_at: None,
            cluster: None,
            embedding: EmbeddingConfig::default(),
            storage: StorageConfig::default(),
        }
    }

    #[test]
    fn create_list_get_delete() {
        let home = tempfile::tempdir().unwrap();
        let pm = JsonProjectStore::new(home.path()).unwrap();

        assert!(pm.list().unwrap().is_empty());
        pm.create(cfg("healthcare")).unwrap();
        pm.create(cfg("finance")).unwrap();

        let names: Vec<_> = pm.list().unwrap().into_iter().map(|p| p.config.name).collect();
        assert_eq!(names, vec!["finance", "healthcare"]); // sorted

        let hc = pm.get("healthcare").unwrap();
        assert_eq!(hc.config.dim, 128);
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
        assert!(matches!(pm.create(cfg("ok")), Err(DaemonError::AlreadyExists(_))));
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
                ProjectNode { id: 1, http_port: 4010, raft_port: Some(4110) },
                ProjectNode { id: 2, http_port: 4011, raft_port: Some(4111) },
                ProjectNode { id: 3, http_port: 4012, raft_port: Some(4112) },
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
        let reloaded = JsonProjectStore::new(home.path()).unwrap().get("clustered").unwrap().config;
        assert_eq!(reloaded, config);
    }
}
