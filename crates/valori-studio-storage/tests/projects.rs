// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use std::path::Path;

use valori_domain::ProjectId;
use valori_studio_storage::project::ProjectKind;
use valori_studio_storage::StudioDatabase;

fn open_tmp() -> (tempfile::TempDir, StudioDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let db = StudioDatabase::open(&dir.path().join("studio.redb")).unwrap();
    (dir, db)
}

#[test]
fn register_then_lookup() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    db.projects()
        .register_local(id, "My Project", Path::new("/home/me/projects/demo"), 1000)
        .unwrap();

    let got = db.projects().get(id).unwrap().unwrap();
    assert_eq!(got.id, id);
    assert_eq!(got.display_name, "My Project");
    assert_eq!(got.registered_at, 1000);
    assert!(!got.favorite);
    assert_eq!(got.last_opened_at, None);
    match got.kind {
        ProjectKind::Local { path } => assert_eq!(path, Path::new("/home/me/projects/demo")),
        ProjectKind::Cloud { .. } => panic!("expected Local"),
    }
}

#[test]
fn rename_preserves_id_and_other_fields() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    let registry = db.projects();
    registry
        .register_local(id, "Old Name", Path::new("/p"), 1000)
        .unwrap();
    registry.set_favorite(id, true).unwrap();

    let renamed = registry.rename(id, "New Name").unwrap();
    assert_eq!(renamed.id, id, "identity must survive a rename");
    assert_eq!(renamed.display_name, "New Name");
    assert!(renamed.favorite, "unrelated fields must survive a rename");
    assert_eq!(renamed.registered_at, 1000);
}

#[test]
fn path_change_preserves_id() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    let registry = db.projects();
    registry
        .register_local(id, "Demo", Path::new("/old/path"), 1000)
        .unwrap();

    let moved = registry.set_local_path(id, Path::new("/new/path")).unwrap();
    assert_eq!(moved.id, id, "identity must survive a path change");
    match moved.kind {
        ProjectKind::Local { path } => assert_eq!(path, Path::new("/new/path")),
        _ => panic!("expected Local"),
    }
}

/// Re-registering an already-registered project (same id) must merge into
/// the existing record, not create a duplicate or reset favorite/
/// registered_at/last_opened_at. This is the identity-preservation
/// contract CLAUDE.md item 8 requires tests for.
#[test]
fn re_registration_is_idempotent_and_preserves_identity() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    let registry = db.projects();

    registry
        .register_local(id, "Demo", Path::new("/p"), 1000)
        .unwrap();
    registry.set_favorite(id, true).unwrap();
    registry.touch_last_opened(id, 2000).unwrap();

    // Re-register (e.g. daemon restarted and re-announced the same project).
    let re_registered = registry
        .register_local(id, "Demo", Path::new("/p"), 9999)
        .unwrap();

    assert_eq!(re_registered.id, id);
    assert_eq!(
        re_registered.registered_at, 1000,
        "registered_at must not move on re-registration"
    );
    assert!(
        re_registered.favorite,
        "favorite must survive re-registration"
    );
    assert_eq!(
        re_registered.last_opened_at,
        Some(2000),
        "last_opened_at must survive re-registration"
    );

    assert_eq!(
        registry.list().unwrap().len(),
        1,
        "re-registration must not create a duplicate row"
    );
}

#[test]
fn restart_preserves_identity_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    let id = ProjectId::new();

    {
        let db = StudioDatabase::open(&path).unwrap();
        db.projects()
            .register_local(id, "Demo", Path::new("/p"), 1000)
            .unwrap();
        db.projects().set_favorite(id, true).unwrap();
    }
    {
        let db = StudioDatabase::open(&path).unwrap();
        let got = db.projects().get(id).unwrap().unwrap();
        assert_eq!(got.id, id);
        assert!(got.favorite);
    }
}

#[test]
fn favorite_toggle() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    let registry = db.projects();
    registry
        .register_local(id, "Demo", Path::new("/p"), 1000)
        .unwrap();

    registry.set_favorite(id, true).unwrap();
    assert_eq!(registry.favorites().unwrap().len(), 1);

    registry.set_favorite(id, false).unwrap();
    assert!(registry.favorites().unwrap().is_empty());
}

#[test]
fn recent_is_sorted_by_last_opened_descending() {
    let (_dir, db) = open_tmp();
    let registry = db.projects();

    let a = ProjectId::new();
    let b = ProjectId::new();
    let c = ProjectId::new();
    registry
        .register_local(a, "A", Path::new("/a"), 1000)
        .unwrap();
    registry
        .register_local(b, "B", Path::new("/b"), 1000)
        .unwrap();
    registry
        .register_local(c, "C", Path::new("/c"), 1000)
        .unwrap();

    registry.touch_last_opened(a, 100).unwrap();
    registry.touch_last_opened(b, 300).unwrap();
    registry.touch_last_opened(c, 200).unwrap();

    let recent = registry.recent(2).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id, b);
    assert_eq!(recent[1].id, c);
}

#[test]
fn cloud_reference_never_carries_credentials_and_uses_string_org_ref() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    db.projects()
        .register_cloud_ref(
            id,
            "Cloud Demo",
            Some("org_abc123".to_string()),
            "https://api.valori.systems",
            Some("us-east-1".to_string()),
            1000,
        )
        .unwrap();

    let got = db.projects().get(id).unwrap().unwrap();
    match &got.kind {
        ProjectKind::Cloud {
            organization_id,
            cloud_endpoint,
            region,
        } => {
            assert_eq!(organization_id.as_deref(), Some("org_abc123"));
            assert_eq!(cloud_endpoint, "https://api.valori.systems");
            assert_eq!(region.as_deref(), Some("us-east-1"));
        }
        ProjectKind::Local { .. } => panic!("expected Cloud"),
    }

    // Structural guard, not exhaustive: the serialized record must not
    // contain any of the secret-shaped field names this store must never
    // accept.
    let raw = serde_json::to_string(&got).unwrap();
    for forbidden in [
        "api_key",
        "apiKey",
        "access_token",
        "refresh_token",
        "password",
    ] {
        assert!(
            !raw.contains(forbidden),
            "serialized project record must never contain {forbidden}"
        );
    }
}

#[test]
fn unregister_removes_the_record() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    let registry = db.projects();
    registry
        .register_local(id, "Demo", Path::new("/p"), 1000)
        .unwrap();

    assert!(registry.unregister(id).unwrap());
    assert!(registry.get(id).unwrap().is_none());
    assert!(
        !registry.unregister(id).unwrap(),
        "unregistering twice is not an error"
    );
}

#[test]
fn rename_of_unregistered_project_is_not_found() {
    let (_dir, db) = open_tmp();
    let id = ProjectId::new();
    let err = db.projects().rename(id, "Ghost").unwrap_err();
    assert!(matches!(
        err,
        valori_studio_storage::StudioStorageError::NotFound(_)
    ));
}
