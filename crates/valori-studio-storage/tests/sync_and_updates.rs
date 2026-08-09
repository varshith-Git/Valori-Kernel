// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_domain::ProjectId;
use valori_studio_storage::sync::StudioSyncState;
use valori_studio_storage::update::StudioUpdateState;
use valori_studio_storage::StudioDatabase;

fn open_tmp() -> (tempfile::TempDir, StudioDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let db = StudioDatabase::open(&dir.path().join("studio.redb")).unwrap();
    (dir, db)
}

// ── Sync state ────────────────────────────────────────────────────────────

#[test]
fn sync_write_then_read() {
    let (_dir, db) = open_tmp();
    let project_id = ProjectId::new();
    let state = StudioSyncState {
        project_id,
        last_sync: Some(1000),
        remote_version: Some("etag-1".to_string()),
        dirty: false,
        conflict: false,
    };
    db.sync().set(&state).unwrap();
    assert_eq!(db.sync().get(project_id).unwrap(), Some(state));
}

#[test]
fn sync_get_on_unregistered_project_is_none() {
    let (_dir, db) = open_tmp();
    assert_eq!(db.sync().get(ProjectId::new()).unwrap(), None);
}

#[test]
fn sync_update_starts_from_fresh_defaults() {
    let (_dir, db) = open_tmp();
    let project_id = ProjectId::new();
    let updated = db.sync().update(project_id, |s| s.dirty = true).unwrap();
    assert_eq!(updated.project_id, project_id);
    assert!(updated.dirty);
    assert_eq!(updated.last_sync, None);
}

#[test]
fn sync_delete() {
    let (_dir, db) = open_tmp();
    let project_id = ProjectId::new();
    db.sync().set(&StudioSyncState::fresh(project_id)).unwrap();
    assert!(db.sync().delete(project_id).unwrap());
    assert!(db.sync().get(project_id).unwrap().is_none());
}

#[test]
fn sync_reopen_preserves_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    let project_id = ProjectId::new();
    {
        let db = StudioDatabase::open(&path).unwrap();
        db.sync().update(project_id, |s| s.conflict = true).unwrap();
    }
    {
        let db = StudioDatabase::open(&path).unwrap();
        assert!(db.sync().get(project_id).unwrap().unwrap().conflict);
    }
}

// ── Update state ─────────────────────────────────────────────────────────

#[test]
fn update_state_defaults_when_absent() {
    let (_dir, db) = open_tmp();
    assert_eq!(db.updates().get().unwrap(), StudioUpdateState::default());
}

#[test]
fn update_state_write_then_read() {
    let (_dir, db) = open_tmp();
    let state = StudioUpdateState {
        last_checked: Some(1000),
        available_version: Some("0.3.0".to_string()),
        downloaded: true,
        downloaded_at: Some(1100),
    };
    db.updates().set(&state).unwrap();
    assert_eq!(db.updates().get().unwrap(), state);
}

#[test]
fn update_state_reopen_preserves_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    {
        let db = StudioDatabase::open(&path).unwrap();
        db.updates()
            .update(|u| u.available_version = Some("0.3.0".to_string()))
            .unwrap();
    }
    {
        let db = StudioDatabase::open(&path).unwrap();
        assert_eq!(
            db.updates().get().unwrap().available_version,
            Some("0.3.0".to_string())
        );
    }
}
