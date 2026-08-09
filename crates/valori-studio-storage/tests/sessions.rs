// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_domain::{InstallationId, SessionId};
use valori_studio_storage::session::SessionRetentionPolicy;
use valori_studio_storage::StudioDatabase;

fn open_tmp() -> (tempfile::TempDir, StudioDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let db = StudioDatabase::open(&dir.path().join("studio.redb")).unwrap();
    (dir, db)
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Starts and cleanly ends a session at `started_at`, `started_at + 1000`.
fn mk_completed(db: &StudioDatabase, started_at: i64) -> SessionId {
    let id = SessionId::new();
    db.sessions()
        .start(id, None, "0.2.4", "macos", started_at)
        .unwrap();
    db.sessions().end(id, started_at + 1000, false).unwrap();
    id
}

/// Starts and ends a session marked crashed, at `started_at`.
fn mk_crashed(db: &StudioDatabase, started_at: i64) -> SessionId {
    let id = SessionId::new();
    db.sessions()
        .start(id, None, "0.2.4", "macos", started_at)
        .unwrap();
    db.sessions().end(id, started_at + 1000, true).unwrap();
    id
}

#[test]
fn start_then_get() {
    let (_dir, db) = open_tmp();
    let id = SessionId::new();
    let install_id = InstallationId::new();
    db.sessions()
        .start(id, Some(install_id), "0.2.4", "macos", 1000)
        .unwrap();

    let got = db.sessions().get(id).unwrap().unwrap();
    assert_eq!(got.id, id);
    assert_eq!(got.installation_id, Some(install_id));
    assert_eq!(got.app_version, "0.2.4");
    assert_eq!(got.platform, "macos");
    assert_eq!(got.started_at, 1000);
    assert_eq!(got.ended_at, None);
    assert!(!got.crashed);
    assert!(got.is_open());
}

#[test]
fn end_marks_ended_and_not_crashed() {
    let (_dir, db) = open_tmp();
    let id = SessionId::new();
    db.sessions()
        .start(id, None, "0.2.4", "linux", 1000)
        .unwrap();
    let ended = db.sessions().end(id, 2000, false).unwrap();

    assert_eq!(ended.ended_at, Some(2000));
    assert!(!ended.crashed);
    assert!(!ended.is_open());
}

#[test]
fn crash_state_is_recorded() {
    let (_dir, db) = open_tmp();
    let id = SessionId::new();
    db.sessions()
        .start(id, None, "0.2.4", "windows", 1000)
        .unwrap();
    let ended = db.sessions().end(id, 1050, true).unwrap();

    assert!(ended.crashed);
    assert_eq!(ended.ended_at, Some(1050));
}

#[test]
fn ending_an_unknown_session_is_not_found() {
    let (_dir, db) = open_tmp();
    let err = db
        .sessions()
        .end(SessionId::new(), 1000, false)
        .unwrap_err();
    assert!(matches!(
        err,
        valori_studio_storage::StudioStorageError::NotFound(_)
    ));
}

#[test]
fn open_sessions_excludes_ended_ones() {
    let (_dir, db) = open_tmp();
    let a = SessionId::new();
    let b = SessionId::new();
    db.sessions()
        .start(a, None, "0.2.4", "macos", 1000)
        .unwrap();
    db.sessions()
        .start(b, None, "0.2.4", "macos", 1000)
        .unwrap();
    db.sessions().end(a, 2000, false).unwrap();

    let open = db.sessions().open_sessions().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, b);
}

#[test]
fn recent_sorted_by_started_at_descending() {
    let (_dir, db) = open_tmp();
    let a = SessionId::new();
    let b = SessionId::new();
    let c = SessionId::new();
    db.sessions().start(a, None, "0.2.4", "macos", 100).unwrap();
    db.sessions().start(b, None, "0.2.4", "macos", 300).unwrap();
    db.sessions().start(c, None, "0.2.4", "macos", 200).unwrap();

    let recent = db.sessions().recent(2).unwrap();
    assert_eq!(recent[0].id, b);
    assert_eq!(recent[1].id, c);
}

#[test]
fn reopen_preserves_session_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    let id = SessionId::new();
    {
        let db = StudioDatabase::open(&path).unwrap();
        db.sessions()
            .start(id, None, "0.2.4", "macos", 1000)
            .unwrap();
        db.sessions().end(id, 1500, false).unwrap();
    }
    {
        let db = StudioDatabase::open(&path).unwrap();
        let got = db.sessions().get(id).unwrap().unwrap();
        assert_eq!(got.started_at, 1000);
        assert_eq!(got.ended_at, Some(1500));
    }
}

#[test]
fn start_and_end_are_idempotent() {
    let (_dir, db) = open_tmp();
    let id = SessionId::new();

    // Start twice
    let s1 = db
        .sessions()
        .start(id, None, "0.2.4", "macos", 1000)
        .unwrap();
    let s2 = db
        .sessions()
        .start(id, None, "0.2.4", "macos", 2000)
        .unwrap();
    assert_eq!(s1.started_at, 1000);
    assert_eq!(
        s2.started_at, 1000,
        "second start must be idempotent and preserve original started_at"
    );

    // End twice
    let e1 = db.sessions().end(id, 3000, false).unwrap();
    let e2 = db.sessions().end(id, 3000, false).unwrap();
    assert_eq!(e1.ended_at, Some(3000));
    assert_eq!(e2.ended_at, Some(3000), "second end must be idempotent");
}

#[test]
fn reconcile_crashed_marks_prior_open_sessions() {
    let (_dir, db) = open_tmp();
    let prior_crashed = SessionId::new();
    let prior_clean = SessionId::new();
    let current_session = SessionId::new();

    // Prior session 1: crashed (never called end)
    db.sessions()
        .start(prior_crashed, None, "0.2.4", "macos", 1000)
        .unwrap();

    // Prior session 2: ended cleanly
    db.sessions()
        .start(prior_clean, None, "0.2.4", "macos", 2000)
        .unwrap();
    db.sessions().end(prior_clean, 2500, false).unwrap();

    // Current session started
    db.sessions()
        .start(current_session, None, "0.2.4", "macos", 3000)
        .unwrap();

    // Reconcile crashed
    let count = db
        .sessions()
        .reconcile_crashed(current_session, 3000)
        .unwrap();
    assert_eq!(count, 1);

    let crashed_rec = db.sessions().get(prior_crashed).unwrap().unwrap();
    assert!(crashed_rec.crashed);
    assert_eq!(crashed_rec.ended_at, Some(3000));

    let clean_rec = db.sessions().get(prior_clean).unwrap().unwrap();
    assert!(!clean_rec.crashed);
    assert_eq!(clean_rec.ended_at, Some(2500));

    let curr_rec = db.sessions().get(current_session).unwrap().unwrap();
    assert!(!curr_rec.crashed);
    assert_eq!(curr_rec.ended_at, None);
    assert!(curr_rec.is_open());
}

// ── S5: session retention / prune ──────────────────────────────────────────

#[test]
fn prune_on_empty_database_is_a_safe_no_op() {
    let (_dir, db) = open_tmp();
    let now = 1_000_000_000;
    let stats = db
        .sessions()
        .prune(SessionId::new(), &SessionRetentionPolicy::default(), now)
        .unwrap();
    assert_eq!(stats.scanned, 0);
    assert_eq!(stats.deleted, 0);
    assert_eq!(stats.retained, 0);
}

#[test]
fn active_session_is_never_deleted() {
    let (_dir, db) = open_tmp();
    let now = 1_000 * DAY_MS;
    let active = SessionId::new();
    // Started long ago but never ended — still open.
    db.sessions()
        .start(active, None, "0.2.4", "macos", 1)
        .unwrap();

    let policy = SessionRetentionPolicy {
        max_completed_sessions: 0,
        completed_retention_days: 0,
        crashed_retention_days: 0,
    };
    let stats = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(stats.deleted, 0);
    assert_eq!(stats.protected_active, 1);
    assert!(db.sessions().get(active).unwrap().is_some());
}

#[test]
fn current_session_is_never_deleted_even_if_it_would_otherwise_be_eligible() {
    let (_dir, db) = open_tmp();
    let now = 1_000 * DAY_MS;
    // Ended and crashed, ancient — would be eligible under an aggressive
    // policy, except it's passed as the current session.
    let current = mk_crashed(&db, 1);

    let policy = SessionRetentionPolicy {
        max_completed_sessions: 0,
        completed_retention_days: 0,
        crashed_retention_days: 0,
    };
    let stats = db.sessions().prune(current, &policy, now).unwrap();
    assert_eq!(stats.deleted, 0);
    assert_eq!(stats.protected_current, 1);
    assert!(db.sessions().get(current).unwrap().is_some());
}

#[test]
fn recent_completed_sessions_are_never_deleted() {
    let (_dir, db) = open_tmp();
    let now = 100 * DAY_MS;
    let recent = mk_completed(&db, now - 5 * DAY_MS); // 5 days old

    // Force everything past the count floor so only the age check protects it.
    let policy = SessionRetentionPolicy {
        max_completed_sessions: 0,
        completed_retention_days: 90,
        crashed_retention_days: 180,
    };
    let stats = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(stats.deleted, 0);
    assert_eq!(stats.protected_within_retention, 1);
    assert!(db.sessions().get(recent).unwrap().is_some());
}

#[test]
fn old_completed_sessions_beyond_the_cap_are_deleted() {
    let (_dir, db) = open_tmp();
    let now = 1000 * DAY_MS;
    let old = mk_completed(&db, now - 200 * DAY_MS); // 200 days old

    let policy = SessionRetentionPolicy {
        max_completed_sessions: 0, // nothing protected by the newest-N floor
        completed_retention_days: 90,
        crashed_retention_days: 180,
    };
    let stats = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(stats.deleted, 1);
    assert!(db.sessions().get(old).unwrap().is_none());
}

#[test]
fn recent_crashed_sessions_are_never_deleted() {
    let (_dir, db) = open_tmp();
    let now = 200 * DAY_MS;
    let recent_crash = mk_crashed(&db, now - 10 * DAY_MS); // 10 days old, well under 180

    let policy = SessionRetentionPolicy::default();
    let stats = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(stats.deleted, 0);
    assert_eq!(stats.protected_within_retention, 1);
    assert!(db.sessions().get(recent_crash).unwrap().is_some());
}

#[test]
fn old_crashed_sessions_are_deleted() {
    let (_dir, db) = open_tmp();
    let now = 1000 * DAY_MS;
    let old_crash = mk_crashed(&db, now - 200 * DAY_MS); // 200 days old, past 180

    let policy = SessionRetentionPolicy::default();
    let stats = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(stats.deleted, 1);
    assert!(db.sessions().get(old_crash).unwrap().is_none());
}

#[test]
fn completed_count_below_the_cap_deletes_nothing_regardless_of_age() {
    let (_dir, db) = open_tmp();
    let now = 1000 * DAY_MS;
    // 5 completed sessions, all ancient (200 days), cap is 100 — well
    // under the cap, so the count-exceeded trigger never fires.
    let mut ids = Vec::new();
    for i in 0..5 {
        ids.push(mk_completed(&db, now - 200 * DAY_MS - i * 1000));
    }

    let policy = SessionRetentionPolicy::default(); // cap 100, 90-day age
    let stats = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(
        stats.deleted, 0,
        "row count under the cap must never trigger deletion, even for old sessions \
         beyond the age cutoff — the cap is the gate, per policy"
    );
    for id in ids {
        assert!(db.sessions().get(id).unwrap().is_some());
    }
}

#[test]
fn completed_count_above_the_cap_prunes_only_the_old_excess() {
    let (_dir, db) = open_tmp();
    let now = 1000 * DAY_MS;
    let policy = SessionRetentionPolicy {
        max_completed_sessions: 3,
        completed_retention_days: 90,
        crashed_retention_days: 180,
    };

    // 5 completed sessions: newest 3 protected by the cap; of the oldest 2,
    // one is past 90 days (eligible) and one is recent (protected by age).
    let newest_a = mk_completed(&db, now - DAY_MS);
    let newest_b = mk_completed(&db, now - 2 * DAY_MS);
    let newest_c = mk_completed(&db, now - 3 * DAY_MS);
    let excess_recent = mk_completed(&db, now - 10 * DAY_MS); // rank 4, but only 10 days old
    let excess_old = mk_completed(&db, now - 200 * DAY_MS); // rank 5, 200 days old

    let stats = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(stats.deleted, 1);
    assert!(db.sessions().get(excess_old).unwrap().is_none());
    // Everything else, including the recent excess row, survives.
    for id in [newest_a, newest_b, newest_c, excess_recent] {
        assert!(
            db.sessions().get(id).unwrap().is_some(),
            "session {id} must survive — protected by rank or by age"
        );
    }
}

#[test]
fn deletion_among_excess_completed_sessions_is_oldest_first_and_deterministic() {
    let (_dir, db) = open_tmp();
    let now = 1000 * DAY_MS;
    let policy = SessionRetentionPolicy {
        max_completed_sessions: 2,
        completed_retention_days: 90,
        crashed_retention_days: 180,
    };

    // 5 sessions, all past the 90-day cutoff; newest 2 protected by cap;
    // the 3 oldest are all eligible — verifies every eligible one is
    // removed (not just the single oldest), deterministically.
    let newest_a = mk_completed(&db, now - 100 * DAY_MS);
    let newest_b = mk_completed(&db, now - 110 * DAY_MS);
    let old_c = mk_completed(&db, now - 120 * DAY_MS);
    let old_d = mk_completed(&db, now - 130 * DAY_MS);
    let oldest_e = mk_completed(&db, now - 140 * DAY_MS);

    let stats = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(stats.deleted, 3);
    for id in [old_c, old_d, oldest_e] {
        assert!(db.sessions().get(id).unwrap().is_none());
    }
    for id in [newest_a, newest_b] {
        assert!(db.sessions().get(id).unwrap().is_some());
    }

    // Re-run with a fixed clock and identical fixture shape (fresh DB) to
    // confirm the same decision is reached every time — determinism, not
    // just "some subset got deleted."
    let (_dir2, db2) = open_tmp();
    let a2 = mk_completed(&db2, now - 100 * DAY_MS);
    let b2 = mk_completed(&db2, now - 110 * DAY_MS);
    let c2 = mk_completed(&db2, now - 120 * DAY_MS);
    let d2 = mk_completed(&db2, now - 130 * DAY_MS);
    let e2 = mk_completed(&db2, now - 140 * DAY_MS);
    let stats2 = db2
        .sessions()
        .prune(SessionId::new(), &policy, now)
        .unwrap();
    assert_eq!(stats2.deleted, 3);
    assert!(db2.sessions().get(c2).unwrap().is_none());
    assert!(db2.sessions().get(d2).unwrap().is_none());
    assert!(db2.sessions().get(e2).unwrap().is_none());
    assert!(db2.sessions().get(a2).unwrap().is_some());
    assert!(db2.sessions().get(b2).unwrap().is_some());
}

#[test]
fn prune_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    let now = 1000 * DAY_MS;
    let policy = SessionRetentionPolicy {
        max_completed_sessions: 0,
        completed_retention_days: 90,
        crashed_retention_days: 180,
    };
    let (kept, removed) = {
        let db = StudioDatabase::open(&path).unwrap();
        let kept = mk_completed(&db, now - 5 * DAY_MS);
        let removed = mk_completed(&db, now - 200 * DAY_MS);
        let stats = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
        assert_eq!(stats.deleted, 1);
        (kept, removed)
    };
    {
        let db = StudioDatabase::open(&path).unwrap();
        assert!(db.sessions().get(kept).unwrap().is_some());
        assert!(db.sessions().get(removed).unwrap().is_none());
    }
}

#[test]
fn pruning_twice_is_idempotent() {
    let (_dir, db) = open_tmp();
    let now = 1000 * DAY_MS;
    let policy = SessionRetentionPolicy {
        max_completed_sessions: 0,
        completed_retention_days: 90,
        crashed_retention_days: 180,
    };
    let old = mk_completed(&db, now - 200 * DAY_MS);

    let first = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(first.deleted, 1);
    assert!(db.sessions().get(old).unwrap().is_none());

    let second = db.sessions().prune(SessionId::new(), &policy, now).unwrap();
    assert_eq!(second.deleted, 0, "nothing left to delete on a second pass");
    assert_eq!(second.scanned, 0);
}

#[test]
fn realistic_mixed_fixture_prunes_exactly_the_expected_set() {
    let (_dir, db) = open_tmp();
    let now = 1000 * DAY_MS;
    let policy = SessionRetentionPolicy::default(); // 100 / 90d / 180d

    let current = SessionId::new();
    db.sessions()
        .start(current, None, "0.2.4", "macos", now)
        .unwrap();

    let active_other = SessionId::new(); // e.g. a race with reconcile — still protected
    db.sessions()
        .start(active_other, None, "0.2.4", "macos", now - 1000)
        .unwrap();

    let recent_completed = mk_completed(&db, now - 10 * DAY_MS);
    let old_completed = mk_completed(&db, now - 95 * DAY_MS);
    let recent_crashed = mk_crashed(&db, now - 20 * DAY_MS);
    let old_crashed = mk_crashed(&db, now - 200 * DAY_MS);

    let stats = db.sessions().prune(current, &policy, now).unwrap();

    assert_eq!(stats.scanned, 6);
    assert_eq!(
        stats.deleted, 1,
        "only old_crashed is past its 180-day window"
    );
    assert_eq!(stats.protected_current, 1);
    assert_eq!(stats.protected_active, 1);
    assert_eq!(stats.retained, 5);

    assert!(db.sessions().get(current).unwrap().is_some());
    assert!(db.sessions().get(active_other).unwrap().is_some());
    assert!(db.sessions().get(recent_completed).unwrap().is_some());
    // old_completed is only 95 days old (>90) but well under the 100-count
    // cap, so it survives — the count gate never fired.
    assert!(db.sessions().get(old_completed).unwrap().is_some());
    assert!(db.sessions().get(recent_crashed).unwrap().is_some());
    assert!(db.sessions().get(old_crashed).unwrap().is_none());
}
