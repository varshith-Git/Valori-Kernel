// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use std::path::Path;

use valori_domain::ProjectId;
use valori_studio_storage::project_cache::StudioProjectCacheEntry;
use valori_studio_storage::StudioDatabase;

fn open_tmp() -> (tempfile::TempDir, StudioDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let db = StudioDatabase::open(&dir.path().join("studio.redb")).unwrap();
    (dir, db)
}

#[test]
fn put_then_get() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    let entry = StudioProjectCacheEntry {
        id,
        display_name: Some("Demo".to_string()),
        status: Some("running".to_string()),
        record_count: Some(42),
        refreshed_at: 1000,
    };
    db.project_cache().put(&entry).unwrap();
    assert_eq!(db.project_cache().get(id).unwrap(), Some(entry));
}

/// The load-bearing property: clearing (or never populating) the cache
/// must have zero effect on the authoritative project registry.
#[test]
fn clearing_the_cache_does_not_affect_the_project_registry() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    db.projects()
        .register_local(id, "Demo", Path::new("/p"), 1000)
        .unwrap();
    db.project_cache()
        .put(&StudioProjectCacheEntry {
            id,
            display_name: Some("Demo".to_string()),
            status: Some("running".to_string()),
            record_count: Some(10),
            refreshed_at: 1000,
        })
        .unwrap();

    let removed = db.project_cache().clear().unwrap();
    assert_eq!(removed, 1);
    assert!(db.project_cache().get(id).unwrap().is_none());

    // The registry entry must be completely unaffected.
    let project = db.projects().get(id).unwrap().unwrap();
    assert_eq!(project.id, id);
    assert_eq!(project.display_name, "Demo");
}

#[test]
fn delete_single_entry() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    db.project_cache()
        .put(&StudioProjectCacheEntry {
            id,
            display_name: None,
            status: None,
            record_count: None,
            refreshed_at: 1,
        })
        .unwrap();
    assert!(db.project_cache().delete(id).unwrap());
    assert!(db.project_cache().get(id).unwrap().is_none());
}

#[test]
fn reopen_preserves_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    let id = ProjectId::new();
    {
        let db = StudioDatabase::open(&path).unwrap();
        db.project_cache()
            .put(&StudioProjectCacheEntry {
                id,
                display_name: Some("X".into()),
                status: None,
                record_count: None,
                refreshed_at: 5,
            })
            .unwrap();
    }
    {
        let db = StudioDatabase::open(&path).unwrap();
        assert_eq!(db.project_cache().get(id).unwrap().unwrap().refreshed_at, 5);
    }
}
