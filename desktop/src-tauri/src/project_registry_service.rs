// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Typed project registry service and Tauri command handlers for `studio.redb`.
//!
//! # Architecture (S2b-2b)
//!
//! ```text
//! Next.js / React (via `native.ts`)
//!        │
//!        ▼
//! Tauri commands (`registry_*`)
//!        │
//!        ▼
//! `ProjectRegistryService`
//!        │
//!        ▼
//! `Arc<StudioDatabase>`
//!        │
//!        ▼
//! `studio.redb` (`projects` table)
//! ```
//!
//! # Separation of Concerns
//!
//! - `studio.redb`'s `projects` table is a **Studio registry/reference layer**, NOT a project database.
//! - Actual local projects (vectors, WAL, snapshots, indexes, collections, records) remain
//!   exclusively owned by the Valori storage/daemon layer (`~/.valori/projects/<name>/`).
//! - Cloud projects remain authoritative in Valori Cloud.
//! - `ProjectId` ([`valori_domain::ProjectId`]) is the only canonical identity key.
//! - Renames and moves update display name and path reference while preserving `ProjectId`.
//! - Missing local project paths are represented with `available: false` and are never automatically deleted.
//! - Legacy name-only entries in `meta.legacy_project_names` are reconciled only when matched with real projects.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Manager;
use valori_domain::ProjectId;
use valori_studio_storage::{
    project::{ProjectKind, StudioProjectRecord},
    StudioDatabase, StudioStorageResult,
};

/// Public DTO representing a Studio project registry record.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioProjectDto {
    pub id: String,
    pub display_name: String,
    pub kind: ProjectKind,
    pub favorite: bool,
    pub last_opened_at: Option<i64>,
    pub registered_at: i64,
    /// Indicates whether a local project's path currently exists on disk.
    /// For cloud projects, this is always `true`.
    pub available: bool,
}

impl From<StudioProjectRecord> for StudioProjectDto {
    fn from(r: StudioProjectRecord) -> Self {
        let available = match &r.kind {
            ProjectKind::Local { path } => path.exists(),
            ProjectKind::Cloud { .. } => true,
        };
        Self {
            id: r.id.to_string(),
            display_name: r.display_name,
            kind: r.kind,
            favorite: r.favorite,
            last_opened_at: r.last_opened_at,
            registered_at: r.registered_at,
            available,
        }
    }
}

/// Typed service wrapping project registry operations on `StudioDatabase`.
#[derive(Clone)]
pub struct ProjectRegistryService {
    db: Arc<StudioDatabase>,
}

impl ProjectRegistryService {
    pub fn new(db: Arc<StudioDatabase>) -> Self {
        Self { db }
    }

    /// Lists all registered projects in the Studio registry.
    pub fn list_projects(&self) -> StudioStorageResult<Vec<StudioProjectDto>> {
        let records = self.db.projects().list()?;
        Ok(records.into_iter().map(StudioProjectDto::from).collect())
    }

    /// Looks up a registered project by its canonical `ProjectId`.
    pub fn get_project(&self, id: ProjectId) -> StudioStorageResult<Option<StudioProjectDto>> {
        let record = self.db.projects().get(id)?;
        Ok(record.map(StudioProjectDto::from))
    }

    /// Looks up a registered project by `ProjectId` or fallback `display_name`.
    pub fn find_project(&self, id_or_name: &str) -> StudioStorageResult<Option<StudioProjectDto>> {
        if let Ok(id) = id_or_name.parse::<ProjectId>() {
            if let Some(p) = self.get_project(id)? {
                return Ok(Some(p));
            }
        }
        let list = self.list_projects()?;
        Ok(list.into_iter().find(|p| p.display_name == id_or_name))
    }

    /// Returns registered projects marked as favorite.
    pub fn favorite_projects(&self) -> StudioStorageResult<Vec<StudioProjectDto>> {
        let records = self.db.projects().favorites()?;
        Ok(records.into_iter().map(StudioProjectDto::from).collect())
    }

    /// Returns registered projects sorted by `last_opened_at` descending.
    pub fn recent_projects(&self, limit: usize) -> StudioStorageResult<Vec<StudioProjectDto>> {
        let records = self.db.projects().recent(limit)?;
        Ok(records.into_iter().map(StudioProjectDto::from).collect())
    }

    /// Registers a local project reference in the Studio registry.
    /// Preserves `favorite`, `registered_at`, and `last_opened_at` across re-registration.
    pub fn register_local_project(
        &self,
        id: ProjectId,
        display_name: &str,
        path: &Path,
        now: i64,
    ) -> StudioStorageResult<StudioProjectDto> {
        let record = self
            .db
            .projects()
            .register_local(id, display_name, path, now)?;
        Ok(StudioProjectDto::from(record))
    }

    /// Registers a cloud project reference in the Studio registry.
    /// Authoritative data remains in Valori Cloud.
    pub fn register_cloud_project(
        &self,
        id: ProjectId,
        display_name: &str,
        organization_id: Option<String>,
        cloud_endpoint: &str,
        region: Option<String>,
        now: i64,
    ) -> StudioStorageResult<StudioProjectDto> {
        let record = self.db.projects().register_cloud_ref(
            id,
            display_name,
            organization_id,
            cloud_endpoint,
            region,
            now,
        )?;
        Ok(StudioProjectDto::from(record))
    }

    /// Renames a project in place (display name updated, `ProjectId` unchanged).
    pub fn rename_project(
        &self,
        id: ProjectId,
        new_name: &str,
    ) -> StudioStorageResult<StudioProjectDto> {
        let record = self.db.projects().rename(id, new_name)?;
        Ok(StudioProjectDto::from(record))
    }

    /// Updates the cached local path reference for a project (`ProjectId` unchanged).
    pub fn set_local_path(
        &self,
        id: ProjectId,
        new_path: &Path,
    ) -> StudioStorageResult<StudioProjectDto> {
        let record = self.db.projects().set_local_path(id, new_path)?;
        Ok(StudioProjectDto::from(record))
    }

    /// Sets or clears the favorite flag on a project.
    pub fn set_favorite(
        &self,
        id: ProjectId,
        favorite: bool,
    ) -> StudioStorageResult<StudioProjectDto> {
        let record = self.db.projects().set_favorite(id, favorite)?;
        Ok(StudioProjectDto::from(record))
    }

    /// Updates `last_opened_at` timestamp for a project after successful opening.
    pub fn touch_last_opened(
        &self,
        id: ProjectId,
        now: i64,
    ) -> StudioStorageResult<StudioProjectDto> {
        let record = self.db.projects().touch_last_opened(id, now)?;
        Ok(StudioProjectDto::from(record))
    }

    /// Removes a project from the Studio registry without deleting actual project files.
    pub fn unregister_project(&self, id: ProjectId) -> StudioStorageResult<bool> {
        self.db.projects().unregister(id)
    }

    /// Reconciles legacy project names against known local projects with canonical `ProjectId`s.
    /// Matched names attach real `ProjectId`s and inherit favorite/recent status.
    /// Unmatched names remain as inert residue in `meta.legacy_project_names` without fabricating IDs.
    pub fn reconcile_legacy_project_names(
        &self,
        known_projects: &[(ProjectId, String, PathBuf)],
        now: i64,
    ) -> StudioStorageResult<usize> {
        let legacy = self.db.legacy_project_names()?;
        let Some(legacy) = legacy else {
            return Ok(0);
        };

        let mut reconciled = 0;
        for (id, name, path) in known_projects {
            let is_favorite = legacy.favorite.contains(name);
            let is_recent = legacy.recent.contains(name);
            let is_last_opened = legacy.last_opened.as_deref() == Some(name.as_str());

            if is_favorite || is_recent || is_last_opened {
                let mut record = self.db.projects().register_local(*id, name, path, now)?;
                if is_favorite && !record.favorite {
                    record = self.db.projects().set_favorite(*id, true)?;
                }
                if is_last_opened || is_recent {
                    let opened_at = if is_last_opened { now } else { now - 1000 };
                    record = self.db.projects().touch_last_opened(*id, opened_at)?;
                }
                let _ = record;
                reconciled += 1;
            }
        }

        Ok(reconciled)
    }
}

// ── Tauri Commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn registry_list_projects(app: tauri::AppHandle) -> Result<Vec<StudioProjectDto>, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    service.list_projects().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_get_project(
    app: tauri::AppHandle,
    id: String,
) -> Result<Option<StudioProjectDto>, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    service.find_project(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_recent_projects(
    app: tauri::AppHandle,
    limit: Option<usize>,
) -> Result<Vec<StudioProjectDto>, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    service
        .recent_projects(limit.unwrap_or(8))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_favorite_projects(app: tauri::AppHandle) -> Result<Vec<StudioProjectDto>, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    service.favorite_projects().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_register_local_project(
    app: tauri::AppHandle,
    id: String,
    name: String,
    path: String,
) -> Result<StudioProjectDto, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let project_id = id
        .parse::<ProjectId>()
        .map_err(|_| format!("Invalid ProjectId: {id}"))?;
    let service = ProjectRegistryService::new(db.inner().clone());
    let now = chrono::Utc::now().timestamp_millis();
    service
        .register_local_project(project_id, &name, Path::new(&path), now)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_register_cloud_project(
    app: tauri::AppHandle,
    id: String,
    name: String,
    organization_id: Option<String>,
    endpoint: String,
    region: Option<String>,
) -> Result<StudioProjectDto, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let project_id = id
        .parse::<ProjectId>()
        .map_err(|_| format!("Invalid ProjectId: {id}"))?;
    let service = ProjectRegistryService::new(db.inner().clone());
    let now = chrono::Utc::now().timestamp_millis();
    service
        .register_cloud_project(project_id, &name, organization_id, &endpoint, region, now)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_rename_project(
    app: tauri::AppHandle,
    id: String,
    new_name: String,
) -> Result<StudioProjectDto, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    let project_id = if let Ok(parsed) = id.parse::<ProjectId>() {
        parsed
    } else if let Some(found) = service.find_project(&id).map_err(|e| e.to_string())? {
        found.id.parse::<ProjectId>().map_err(|e| e.to_string())?
    } else {
        return Err(format!("Project not found: {id}"));
    };
    service
        .rename_project(project_id, &new_name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_set_local_path(
    app: tauri::AppHandle,
    id: String,
    new_path: String,
) -> Result<StudioProjectDto, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    let project_id = if let Ok(parsed) = id.parse::<ProjectId>() {
        parsed
    } else if let Some(found) = service.find_project(&id).map_err(|e| e.to_string())? {
        found.id.parse::<ProjectId>().map_err(|e| e.to_string())?
    } else {
        return Err(format!("Project not found: {id}"));
    };
    service
        .set_local_path(project_id, Path::new(&new_path))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_set_favorite(
    app: tauri::AppHandle,
    id: String,
    favorite: bool,
) -> Result<StudioProjectDto, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    let project_id = if let Ok(parsed) = id.parse::<ProjectId>() {
        parsed
    } else if let Some(found) = service.find_project(&id).map_err(|e| e.to_string())? {
        found.id.parse::<ProjectId>().map_err(|e| e.to_string())?
    } else {
        return Err(format!("Project not found: {id}"));
    };
    service
        .set_favorite(project_id, favorite)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_touch_last_opened(
    app: tauri::AppHandle,
    id: String,
) -> Result<StudioProjectDto, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    let project_id = if let Ok(parsed) = id.parse::<ProjectId>() {
        parsed
    } else if let Some(found) = service.find_project(&id).map_err(|e| e.to_string())? {
        found.id.parse::<ProjectId>().map_err(|e| e.to_string())?
    } else {
        return Err(format!("Project not found: {id}"));
    };
    let now = chrono::Utc::now().timestamp_millis();
    service
        .touch_last_opened(project_id, now)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn registry_unregister_project(app: tauri::AppHandle, id: String) -> Result<bool, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    let project_id = if let Ok(parsed) = id.parse::<ProjectId>() {
        parsed
    } else if let Some(found) = service.find_project(&id).map_err(|e| e.to_string())? {
        found.id.parse::<ProjectId>().map_err(|e| e.to_string())?
    } else {
        return Ok(false);
    };
    service
        .unregister_project(project_id)
        .map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownProjectInput {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[tauri::command]
pub fn registry_reconcile_legacy_names(
    app: tauri::AppHandle,
    known_projects: Vec<KnownProjectInput>,
) -> Result<usize, String> {
    let db = app
        .try_state::<Arc<StudioDatabase>>()
        .ok_or_else(|| "StudioDatabase not initialized".to_string())?;
    let service = ProjectRegistryService::new(db.inner().clone());
    let mut mapped = Vec::new();
    for p in &known_projects {
        if let Ok(id) = p.id.parse::<ProjectId>() {
            mapped.push((id, p.name.clone(), PathBuf::from(&p.path)));
        }
    }
    let now = chrono::Utc::now().timestamp_millis();
    service
        .reconcile_legacy_project_names(&mapped, now)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use valori_studio_storage::db::LegacyStudioPaths;

    #[test]
    fn test_project_registry_service_crud_and_invariants() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("studio.redb");
        let db = Arc::new(StudioDatabase::open(&db_path).unwrap());
        let service = ProjectRegistryService::new(db.clone());

        let id1 = ProjectId::new();
        let project_dir = temp.path().join("projects").join("demo");
        std::fs::create_dir_all(&project_dir).unwrap();

        // 1. Register local project
        let reg = service
            .register_local_project(id1, "demo", &project_dir, 1000)
            .unwrap();
        assert_eq!(reg.id, id1.to_string());
        assert_eq!(reg.display_name, "demo");
        assert!(reg.available);
        assert!(!reg.favorite);

        // 2. Rename project (identity preserved)
        let renamed = service.rename_project(id1, "demo-renamed").unwrap();
        assert_eq!(renamed.id, id1.to_string());
        assert_eq!(renamed.display_name, "demo-renamed");

        // 3. Move path (identity preserved)
        let new_dir = temp.path().join("projects").join("demo-moved");
        std::fs::create_dir_all(&new_dir).unwrap();
        let moved = service.set_local_path(id1, &new_dir).unwrap();
        assert_eq!(moved.id, id1.to_string());
        assert!(moved.available);

        // 4. Missing path detection without deletion
        std::fs::remove_dir_all(&new_dir).unwrap();
        let missing = service.get_project(id1).unwrap().unwrap();
        assert_eq!(missing.id, id1.to_string());
        assert!(
            !missing.available,
            "missing directory marks available: false without deleting entry"
        );

        // 5. Favorites and recents ordering
        let id2 = ProjectId::new();
        service
            .register_local_project(id2, "finance", Path::new("/nonexistent"), 2000)
            .unwrap();
        service.set_favorite(id1, true).unwrap();
        service.touch_last_opened(id2, 5000).unwrap();
        service.touch_last_opened(id1, 6000).unwrap();

        let favs = service.favorite_projects().unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].id, id1.to_string());

        let recents = service.recent_projects(10).unwrap();
        assert_eq!(recents.len(), 2);
        assert_eq!(recents[0].id, id1.to_string()); // 6000 > 5000
        assert_eq!(recents[1].id, id2.to_string());

        // 6. Persistence across reopen
        drop(service);
        drop(db);

        let db2 = Arc::new(StudioDatabase::open(&db_path).unwrap());
        let service2 = ProjectRegistryService::new(db2);
        let list = service2.list_projects().unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|p| p.id == id1.to_string() && p.favorite));
    }

    #[test]
    fn test_legacy_reconciliation_resolves_known_and_leaves_unresolved_as_residue() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("studio.redb");
        let pref_file = temp.path().join("preferences.json");
        std::fs::write(
            &pref_file,
            br#"{
                "recentProjects": ["demo", "unknown-ghost"],
                "favoriteProjects": ["demo"]
            }"#,
        )
        .unwrap();

        let db = Arc::new(StudioDatabase::open(&db_path).unwrap());
        let legacy_paths = LegacyStudioPaths {
            preferences_json: Some(pref_file),
            events_jsonl: None,
        };
        db.run_legacy_migration(&legacy_paths, 1000);

        let service = ProjectRegistryService::new(db);
        let real_id = ProjectId::new();
        let demo_path = temp.path().join("projects").join("demo");
        std::fs::create_dir_all(&demo_path).unwrap();

        let known = vec![(real_id, "demo".to_string(), demo_path)];
        let count = service
            .reconcile_legacy_project_names(&known, 2000)
            .unwrap();
        assert_eq!(count, 1);

        let resolved = service.get_project(real_id).unwrap().unwrap();
        assert_eq!(resolved.display_name, "demo");
        assert!(resolved.favorite);

        // unknown-ghost was not assigned any fake ID
        assert_eq!(service.list_projects().unwrap().len(), 1);
    }
}
