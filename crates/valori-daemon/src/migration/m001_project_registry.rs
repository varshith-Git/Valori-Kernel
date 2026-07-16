// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Migration 001 — one-time import of `ui/`'s legacy project registry
//! (`~/.valori/ui-projects.json`, written by `ui/src/lib/server/projects.ts`)
//! into the daemon's own per-project `project.json` manifests.
//!
//! Before this migration, there were two separate sources of truth for "what
//! projects exist": `ui/`'s flat manifest file, and the daemon's (until now,
//! empty) per-project directories. This migration makes the daemon canonical
//! without losing any existing project.
//!
//! Safety properties (see `docs` in `super`):
//! - **Never deletes the source.** `ui-projects.json` is renamed to
//!   `ui-projects.json.migrated` only after every entry has been
//!   successfully imported (or already existed) — never before, never
//!   destructively.
//! - **Idempotent per project.** An entry whose name already has a
//!   `project.json` in the daemon's store is skipped, not overwritten.
//! - **Idempotent as a whole.** If any entry fails to import, `run()` returns
//!   `Err` (the marker in `.migrations.json` is NOT recorded — see
//!   `super::run_all`), and the *next* daemon startup retries: already-
//!   imported entries are skipped, only the failures are retried.
//! - **Best-effort snapshot adoption.** Single-node projects (`replication ==
//!   1`) already live at the exact directory the daemon expects
//!   (`~/.valori/projects/<name>/`) with a matching `events.log` — only the
//!   snapshot filename differs (`current.snap` vs the daemon's
//!   `snapshot.val`), so it's copied (never moved) when present. Cluster
//!   projects (`replication == 3`) are imported as metadata only — the
//!   daemon can't launch a cluster yet (RFC-0006 Phase B.0), so their data
//!   files are left untouched.

use std::path::Path;

use serde::Deserialize;

use crate::error::{DaemonError, DaemonResult};
use crate::policy::RestartPolicy;
use crate::project::{ClusterConfig, EmbeddingConfig, ProjectManifest, ProjectNode, StorageConfig};
use crate::store::ProjectStore;
use crate::workspace::DEFAULT_WORKSPACE;

const LEGACY_MANIFEST_FILE: &str = "ui-projects.json";
const MIGRATED_SUFFIX: &str = "migrated";

/// Mirrors `ui/src/lib/server/projects.ts`'s `ProjectEntry` — a read-only,
/// migration-local copy. Never used outside this file; do not export.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyProjectEntry {
    name: String,
    #[serde(default = "default_replication")]
    replication: u8,
    #[serde(default)]
    nodes: Vec<LegacyProjectNode>,
    #[serde(default = "default_shard_count")]
    shard_count: u32,
    dim: usize,
    #[serde(default = "default_index")]
    index: String,
    #[serde(default = "default_max_records")]
    max_records: usize,
    created_at: String,
    #[serde(default)]
    last_opened_at: Option<String>,
    #[serde(default)]
    embed: Option<LegacyEmbedConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyProjectNode {
    id: u32,
    http_port: u16,
    #[serde(default)]
    raft_port: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct LegacyEmbedConfig {
    provider: String,
    model: String,
    #[serde(default)]
    endpoint: Option<String>,
    // Deliberately NOT imported: ui/'s `apiKey` is a raw secret. Phase B.0's
    // `EmbeddingConfig.api_key_ref` is a *reference*, not a secret store —
    // mislabeling a plaintext key as a "ref" would be worse than dropping it.
    // Whoever wires up embedding-driven ingest decides real secret storage.
}

fn default_replication() -> u8 {
    1
}
fn default_shard_count() -> u32 {
    1
}
fn default_index() -> String {
    "brute".to_string()
}
fn default_max_records() -> usize {
    1_000_000
}

fn parse_iso8601_to_unix(s: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp().max(0) as u64)
}

pub struct Migration001ProjectRegistry;

impl super::Migration for Migration001ProjectRegistry {
    fn id(&self) -> &'static str {
        "001_project_registry"
    }

    fn run(&self, home: &Path, projects: &dyn ProjectStore) -> DaemonResult<()> {
        let legacy_path = home.join(LEGACY_MANIFEST_FILE);
        if !legacy_path.exists() {
            return Ok(()); // nothing to migrate — fresh install, or already done
        }

        let bytes = std::fs::read(&legacy_path)?;
        let entries: Vec<LegacyProjectEntry> = serde_json::from_slice(&bytes)?;

        let mut all_ok = true;
        for entry in &entries {
            match import_one(home, projects, entry) {
                Ok(imported) => {
                    if imported {
                        tracing::info!("migration 001: imported project '{}'", entry.name);
                    }
                }
                Err(e) => {
                    all_ok = false;
                    tracing::warn!(
                        "migration 001: failed to import project '{}': {e} (will retry next startup)",
                        entry.name
                    );
                }
            }
        }

        if !all_ok {
            return Err(DaemonError::InvalidInput(
                "one or more legacy projects failed to import".into(),
            ));
        }

        // Every entry present is now either freshly imported or already was —
        // safe to retire the source. Rename, never delete. (Appends
        // `.migrated` as a suffix — NOT `Path::with_extension`, which would
        // replace `.json` instead of appending after it.)
        let mut retired = legacy_path.clone().into_os_string();
        retired.push(".");
        retired.push(MIGRATED_SUFFIX);
        let retired = std::path::PathBuf::from(retired);
        std::fs::rename(&legacy_path, &retired)?;
        tracing::info!(
            "migration 001: {} project(s) migrated; {} -> {}",
            entries.len(),
            legacy_path.display(),
            retired.display()
        );
        Ok(())
    }
}

/// Returns `Ok(true)` if newly imported, `Ok(false)` if it already existed
/// (already-imported — not an error, just a no-op).
fn import_one(home: &Path, projects: &dyn ProjectStore, entry: &LegacyProjectEntry) -> DaemonResult<bool> {
    if projects.get(&entry.name).is_ok() {
        return Ok(false); // already imported (or a same-named project already exists) — skip
    }

    let cluster = if entry.replication > 1 {
        Some(ClusterConfig {
            replication: entry.replication,
            nodes: entry
                .nodes
                .iter()
                .map(|n| ProjectNode { id: n.id, http_port: n.http_port, raft_port: n.raft_port })
                .collect(),
            shard_count: entry.shard_count,
        })
    } else {
        None
    };

    let embedding = entry
        .embed
        .as_ref()
        .map(|e| EmbeddingConfig {
            provider: Some(e.provider.clone()),
            model: Some(e.model.clone()),
            endpoint: e.endpoint.clone(),
            api_key_ref: None,
        })
        .unwrap_or_default();

    let manifest = ProjectManifest {
        id: crate::new_id(),
        name: entry.name.clone(),
        dim: entry.dim,
        index: entry.index.clone(),
        workspace: DEFAULT_WORKSPACE.to_string(),
        restart_policy: RestartPolicy::Never,
        created_at: parse_iso8601_to_unix(&entry.created_at).unwrap_or(0),
        last_opened_at: entry.last_opened_at.as_deref().and_then(parse_iso8601_to_unix),
        cluster,
        embedding,
        storage: StorageConfig { max_records: entry.max_records, protect_at_rest: true },
    };

    projects.import(manifest)?;

    // Single-node projects live in exactly the directory the daemon expects
    // (~/.valori/projects/<name>/) with a matching events.log — only the
    // snapshot filename differs. Adopt it via copy (never move: the legacy
    // file must survive even if something downstream goes wrong).
    if entry.replication == 1 {
        let dir = home.join("projects").join(&entry.name);
        let legacy_snapshot = dir.join("current.snap");
        let daemon_snapshot = dir.join("snapshot.val");
        if legacy_snapshot.exists() && !daemon_snapshot.exists() {
            std::fs::copy(&legacy_snapshot, &daemon_snapshot)?;
        }
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::Migration;
    use crate::project::JsonProjectStore;

    fn write_legacy_manifest(home: &Path, json: &str) {
        std::fs::write(home.join(LEGACY_MANIFEST_FILE), json).unwrap();
    }

    #[test]
    fn imports_single_node_project_and_retires_source() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("projects").join("healthcare");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("events.log"), b"").unwrap();
        std::fs::write(dir.join("current.snap"), b"fake-snapshot-bytes").unwrap();

        write_legacy_manifest(
            home.path(),
            r#"[{
                "name": "healthcare", "dir": "ignored", "replication": 1,
                "nodes": [{"id": 3010, "httpPort": 3010}], "shardCount": 1,
                "port": 3010, "dim": 768, "index": "hnsw", "maxRecords": 500000,
                "createdAt": "2024-01-01T00:00:00.000Z"
            }]"#,
        );

        let store = JsonProjectStore::new(home.path()).unwrap();
        Migration001ProjectRegistry.run(home.path(), &store).unwrap();

        let project = store.get("healthcare").unwrap();
        assert_eq!(project.config.dim, 768);
        assert_eq!(project.config.index, "hnsw");
        assert_eq!(project.config.storage.max_records, 500_000);
        assert!(project.config.storage.protect_at_rest);
        assert_eq!(project.config.cluster, None);
        assert_eq!(project.config.created_at, 1_704_067_200); // 2024-01-01T00:00:00Z

        // Snapshot adopted via copy — legacy file untouched.
        assert!(dir.join("snapshot.val").exists());
        assert!(dir.join("current.snap").exists());
        assert_eq!(std::fs::read(dir.join("snapshot.val")).unwrap(), b"fake-snapshot-bytes");

        // Source retired, never deleted.
        assert!(!home.path().join(LEGACY_MANIFEST_FILE).exists());
        assert!(home.path().join("ui-projects.json.migrated").exists());
    }

    #[test]
    fn no_legacy_file_is_a_no_op() {
        let home = tempfile::tempdir().unwrap();
        let store = JsonProjectStore::new(home.path()).unwrap();
        assert!(Migration001ProjectRegistry.run(home.path(), &store).is_ok());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn already_imported_project_is_skipped_not_overwritten() {
        let home = tempfile::tempdir().unwrap();
        let store = JsonProjectStore::new(home.path()).unwrap();
        // A project already under daemon management, with different dim than
        // what the (contrived) legacy file below claims.
        store
            .create(ProjectManifest {
                id: crate::new_id(),
                name: "finance".into(),
                dim: 1536,
                index: "brute".into(),
                workspace: DEFAULT_WORKSPACE.to_string(),
                restart_policy: RestartPolicy::Never,
                created_at: 0,
                last_opened_at: None,
                cluster: None,
                embedding: EmbeddingConfig::default(),
                storage: StorageConfig::default(),
            })
            .unwrap();

        write_legacy_manifest(
            home.path(),
            r#"[{
                "name": "finance", "dir": "ignored", "replication": 1,
                "nodes": [{"id": 3010, "httpPort": 3010}], "shardCount": 1,
                "port": 3010, "dim": 42, "index": "brute", "maxRecords": 1,
                "createdAt": "2024-01-01T00:00:00.000Z"
            }]"#,
        );

        Migration001ProjectRegistry.run(home.path(), &store).unwrap();
        assert_eq!(store.get("finance").unwrap().config.dim, 1536); // untouched
    }

    #[test]
    fn cluster_project_imports_metadata_without_touching_snapshot() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("projects").join("clustered");
        std::fs::create_dir_all(&dir).unwrap();

        write_legacy_manifest(
            home.path(),
            r#"[{
                "name": "clustered", "dir": "ignored", "replication": 3,
                "nodes": [
                    {"id": 1, "httpPort": 4010, "raftPort": 4110},
                    {"id": 2, "httpPort": 4011, "raftPort": 4111},
                    {"id": 3, "httpPort": 4012, "raftPort": 4112}
                ],
                "shardCount": 2, "port": 4010, "dim": 768, "index": "brute",
                "maxRecords": 1000000, "createdAt": "2024-06-15T12:30:00.000Z",
                "embed": {"provider": "openai", "model": "text-embedding-3-small", "apiKey": "sk-secret"}
            }]"#,
        );

        let store = JsonProjectStore::new(home.path()).unwrap();
        Migration001ProjectRegistry.run(home.path(), &store).unwrap();

        let project = store.get("clustered").unwrap();
        let cluster = project.config.cluster.as_ref().unwrap();
        assert_eq!(cluster.replication, 3);
        assert_eq!(cluster.nodes.len(), 3);
        assert_eq!(cluster.shard_count, 2);
        assert_eq!(project.config.embedding.provider.as_deref(), Some("openai"));
        // The raw API key is never carried into api_key_ref.
        assert_eq!(project.config.embedding.api_key_ref, None);
        assert!(!dir.join("snapshot.val").exists());
    }
}
