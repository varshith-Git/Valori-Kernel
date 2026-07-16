// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! MetadataDb — redb-backed persistent store for all control-plane types.
//!
//! Table layout:
//!
//! | Table | Key | Value |
//! |---|---|---|
//! | `PROJECTS` | project name (str) | JSON-encoded `Project` |
//! | `COLLECTIONS` | `"project/collection"` (str) | JSON-encoded `Collection` |
//! | `PLANNER_CACHE` | `PlannerCacheKey::to_db_key()` | JSON-encoded `PlannerCacheEntry` |

use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;

use crate::collection::{Collection, CollectionRegistry};
use crate::error::MetadataResult;
use crate::planner_cache::{PlannerCacheEntry, PlannerCacheKey};
use crate::project::Project;

// ── Table definitions ─────────────────────────────────────────────────────────

const PROJECTS: TableDefinition<&str, &[u8]> = TableDefinition::new("projects");
const COLLECTIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("collections");
const PLANNER_CACHE: TableDefinition<&str, &[u8]> = TableDefinition::new("planner_cache");

// ── MetadataDb ────────────────────────────────────────────────────────────────

/// The single redb database that backs all control-plane metadata.
///
/// One `MetadataDb` per valori installation (`~/.valori/metadata.redb`).
/// It is shared across all projects — project names are used as key prefixes.
pub struct MetadataDb {
    db: Database,
}

impl MetadataDb {
    /// Open (or create) the metadata database at `path`.
    pub fn open(path: &Path) -> MetadataResult<Self> {
        let db = Database::create(path)?;
        // Ensure all tables exist.
        let tx = db.begin_write()?;
        tx.open_table(PROJECTS)?;
        tx.open_table(COLLECTIONS)?;
        tx.open_table(PLANNER_CACHE)?;
        tx.commit()?;
        Ok(Self { db })
    }

    // ── Projects ──────────────────────────────────────────────────────────────

    pub fn upsert_project(&self, project: &Project) -> MetadataResult<()> {
        let json = serde_json::to_vec(project)?;
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(PROJECTS)?;
            table.insert(project.name.as_str(), json.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_project(&self, name: &str) -> MetadataResult<Option<Project>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(PROJECTS)?;
        match table.get(name)? {
            None => Ok(None),
            Some(v) => Ok(Some(serde_json::from_slice(v.value())?)),
        }
    }

    pub fn list_projects(&self) -> MetadataResult<Vec<Project>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(PROJECTS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (_, v) = entry?;
            out.push(serde_json::from_slice(v.value())?);
        }
        Ok(out)
    }

    pub fn delete_project(&self, name: &str) -> MetadataResult<bool> {
        let tx = self.db.begin_write()?;
        let removed;
        {
            let mut table = tx.open_table(PROJECTS)?;
            removed = table.remove(name)?.is_some();
        }
        tx.commit()?;
        Ok(removed)
    }

    // ── Collections ───────────────────────────────────────────────────────────

    fn collection_key(project: &str, collection: &str) -> String {
        format!("{}/{}", project, collection)
    }

    pub fn upsert_collection(&self, col: &Collection) -> MetadataResult<()> {
        let key = Self::collection_key(&col.project, &col.name);
        let json = serde_json::to_vec(col)?;
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(COLLECTIONS)?;
            table.insert(key.as_str(), json.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_collection(&self, project: &str, name: &str) -> MetadataResult<Option<Collection>> {
        let key = Self::collection_key(project, name);
        let tx = self.db.begin_read()?;
        let table = tx.open_table(COLLECTIONS)?;
        match table.get(key.as_str())? {
            None => Ok(None),
            Some(v) => Ok(Some(serde_json::from_slice(v.value())?)),
        }
    }

    pub fn list_collections(&self, project: &str) -> MetadataResult<Vec<Collection>> {
        let prefix = format!("{}/", project);
        let tx = self.db.begin_read()?;
        let table = tx.open_table(COLLECTIONS)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            if k.value().starts_with(&prefix) {
                out.push(serde_json::from_slice(v.value())?);
            }
        }
        Ok(out)
    }

    pub fn delete_collection(&self, project: &str, name: &str) -> MetadataResult<bool> {
        let key = Self::collection_key(project, name);
        let tx = self.db.begin_write()?;
        let removed;
        {
            let mut table = tx.open_table(COLLECTIONS)?;
            removed = table.remove(key.as_str())?.is_some();
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Load all collections for `project` into a `CollectionRegistry`.
    pub fn load_collection_registry(&self, project: &str) -> MetadataResult<CollectionRegistry> {
        let cols = self.list_collections(project)?;
        let mut reg = CollectionRegistry::new();
        for col in cols {
            reg.map.insert(col.name.clone(), col.namespace_id);
            if col.namespace_id >= reg.next_id {
                reg.next_id = col.namespace_id + 1;
            }
        }
        Ok(reg)
    }

    // ── PlannerCache ──────────────────────────────────────────────────────────

    pub fn cache_put(
        &self,
        key: &PlannerCacheKey,
        entry: &PlannerCacheEntry,
    ) -> MetadataResult<()> {
        let db_key = key.to_db_key();
        let json = serde_json::to_vec(entry)?;
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(PLANNER_CACHE)?;
            table.insert(db_key.as_str(), json.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn cache_get(&self, key: &PlannerCacheKey) -> MetadataResult<Option<PlannerCacheEntry>> {
        let db_key = key.to_db_key();
        let tx = self.db.begin_read()?;
        let table = tx.open_table(PLANNER_CACHE)?;
        match table.get(db_key.as_str())? {
            None => Ok(None),
            Some(v) => {
                let entry: PlannerCacheEntry = serde_json::from_slice(v.value())?;
                Ok(Some(entry))
            }
        }
    }

    pub fn cache_invalidate(&self, key: &PlannerCacheKey) -> MetadataResult<bool> {
        let db_key = key.to_db_key();
        let tx = self.db.begin_write()?;
        let removed;
        {
            let mut table = tx.open_table(PLANNER_CACHE)?;
            removed = table.remove(db_key.as_str())?.is_some();
        }
        tx.commit()?;
        Ok(removed)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{IndexKind, ProjectMode};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn test_db() -> (MetadataDb, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db = MetadataDb::open(&dir.path().join("metadata.redb")).unwrap();
        (db, dir)
    }

    fn sample_project(name: &str) -> Project {
        Project {
            name: name.to_string(),
            dir: PathBuf::from(format!("/tmp/{}", name)),
            port: 3010,
            dim: 128,
            index: IndexKind::Brute,
            shard_count: 1,
            node_count: 1,
            mode: ProjectMode::Standalone,
            created_at: 1_000_000,
            last_opened_at: None,
            record_count: None,
            nodes: vec![],
        }
    }

    #[test]
    fn project_crud() {
        let (db, _dir) = test_db();

        let p = sample_project("alpha");
        db.upsert_project(&p).unwrap();

        let got = db.get_project("alpha").unwrap().unwrap();
        assert_eq!(got.name, "alpha");
        assert_eq!(got.port, 3010);

        let all = db.list_projects().unwrap();
        assert_eq!(all.len(), 1);

        assert!(db.delete_project("alpha").unwrap());
        assert!(db.get_project("alpha").unwrap().is_none());
    }

    #[test]
    fn collection_crud() {
        let (db, _dir) = test_db();

        let col = Collection {
            name: "papers".to_string(),
            project: "alpha".to_string(),
            namespace_id: 1,
            created_at: 0,
        };
        db.upsert_collection(&col).unwrap();

        let got = db.get_collection("alpha", "papers").unwrap().unwrap();
        assert_eq!(got.namespace_id, 1);

        let list = db.list_collections("alpha").unwrap();
        assert_eq!(list.len(), 1);

        assert!(db.delete_collection("alpha", "papers").unwrap());
        assert!(db.list_collections("alpha").unwrap().is_empty());
    }

    #[test]
    fn collection_registry_roundtrip() {
        let (db, _dir) = test_db();

        for (name, ns_id) in [("alpha", 1u16), ("beta", 2), ("gamma", 3)] {
            db.upsert_collection(&Collection {
                name: name.to_string(),
                project: "proj".to_string(),
                namespace_id: ns_id,
                created_at: 0,
            })
            .unwrap();
        }

        let reg = db.load_collection_registry("proj").unwrap();
        assert_eq!(reg.resolve(Some("alpha")), Some(1));
        assert_eq!(reg.resolve(Some("beta")), Some(2));
        assert_eq!(reg.next_id, 4);
    }

    #[test]
    fn planner_cache_roundtrip() {
        let (db, _dir) = test_db();

        let key = PlannerCacheKey {
            operation_hash: "aaa".to_string(),
            planner_fingerprint_hash: "bbb".to_string(),
            planning_context_hash: "ccc".to_string(),
        };
        let entry = PlannerCacheEntry {
            graph_json: r#"{"tasks":[]}"#.to_string(),
            cached_at: 1000,
            expires_at: 0,
        };

        assert!(db.cache_get(&key).unwrap().is_none());
        db.cache_put(&key, &entry).unwrap();
        let got = db.cache_get(&key).unwrap().unwrap();
        assert_eq!(got.graph_json, r#"{"tasks":[]}"#);
        assert!(!got.is_expired(999_999));

        assert!(db.cache_invalidate(&key).unwrap());
        assert!(db.cache_get(&key).unwrap().is_none());
    }
}
