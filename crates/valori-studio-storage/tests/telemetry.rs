// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
use valori_studio_storage::telemetry::{StudioTelemetryEvent, TelemetryCategory, MAX_QUEUE_LEN};
use valori_studio_storage::StudioDatabase;

fn open_tmp() -> (tempfile::TempDir, StudioDatabase) {
    let dir = tempfile::tempdir().unwrap();
    let db = StudioDatabase::open(&dir.path().join("studio.redb")).unwrap();
    (dir, db)
}

#[test]
fn enqueue_then_count() {
    let (_dir, db) = open_tmp();
    let event = StudioTelemetryEvent::new(
        "app_launched",
        None,
        serde_json::json!({}),
        1000,
        TelemetryCategory::Analytics,
    );
    db.telemetry().enqueue(&event).unwrap();
    assert_eq!(db.telemetry().count().unwrap(), 1);
}

#[test]
fn peek_batch_is_oldest_first() {
    let (_dir, db) = open_tmp();
    let queue = db.telemetry();
    let e1 = StudioTelemetryEvent::new(
        "first",
        None,
        serde_json::json!({}),
        100,
        TelemetryCategory::Analytics,
    );
    let e2 = StudioTelemetryEvent::new(
        "second",
        None,
        serde_json::json!({}),
        200,
        TelemetryCategory::Analytics,
    );
    let e3 = StudioTelemetryEvent::new(
        "third",
        None,
        serde_json::json!({}),
        50,
        TelemetryCategory::Analytics,
    );
    queue.enqueue(&e1).unwrap();
    queue.enqueue(&e2).unwrap();
    queue.enqueue(&e3).unwrap();

    let batch = queue.peek_batch(10).unwrap();
    assert_eq!(
        batch
            .iter()
            .map(|e| e.event_name.as_str())
            .collect::<Vec<_>>(),
        vec!["third", "first", "second"]
    );
}

#[test]
fn peek_batch_respects_limit() {
    let (_dir, db) = open_tmp();
    let queue = db.telemetry();
    for i in 0..5 {
        queue
            .enqueue(&StudioTelemetryEvent::new(
                "e",
                None,
                serde_json::json!({}),
                i,
                TelemetryCategory::Analytics,
            ))
            .unwrap();
    }
    assert_eq!(queue.peek_batch(2).unwrap().len(), 2);
}

#[test]
fn mark_delivered_deletes_the_event() {
    let (_dir, db) = open_tmp();
    let queue = db.telemetry();
    let event = StudioTelemetryEvent::new(
        "app_launched",
        None,
        serde_json::json!({}),
        1000,
        TelemetryCategory::Analytics,
    );
    queue.enqueue(&event).unwrap();

    assert!(queue.mark_delivered(&event.event_id).unwrap());
    assert_eq!(queue.count().unwrap(), 0);
    // No lingering "delivered" row — the whole record is gone.
    assert!(queue.peek_batch(10).unwrap().is_empty());
    // Marking again is not an error, just a no-op.
    assert!(!queue.mark_delivered(&event.event_id).unwrap());
}

#[test]
fn increment_retry_bumps_attempt_count_and_timestamp() {
    let (_dir, db) = open_tmp();
    let queue = db.telemetry();
    let event = StudioTelemetryEvent::new(
        "app_launched",
        None,
        serde_json::json!({}),
        1000,
        TelemetryCategory::Analytics,
    );
    queue.enqueue(&event).unwrap();

    let updated = queue.increment_retry(&event.event_id, 1100).unwrap();
    assert_eq!(updated.attempt_count, 1);
    assert_eq!(updated.last_attempt_at, Some(1100));

    let updated2 = queue.increment_retry(&event.event_id, 1200).unwrap();
    assert_eq!(updated2.attempt_count, 2);
    assert_eq!(updated2.last_attempt_at, Some(1200));
}

#[test]
fn increment_retry_on_unknown_event_is_not_found() {
    let (_dir, db) = open_tmp();
    let err = db
        .telemetry()
        .increment_retry("does-not-exist", 1000)
        .unwrap_err();
    assert!(matches!(
        err,
        valori_studio_storage::StudioStorageError::NotFound(_)
    ));
}

#[test]
fn prune_older_than_removes_only_stale_events() {
    let (_dir, db) = open_tmp();
    let queue = db.telemetry();
    let old = StudioTelemetryEvent::new(
        "old",
        None,
        serde_json::json!({}),
        100,
        TelemetryCategory::Analytics,
    );
    let recent = StudioTelemetryEvent::new(
        "recent",
        None,
        serde_json::json!({}),
        5000,
        TelemetryCategory::Analytics,
    );
    queue.enqueue(&old).unwrap();
    queue.enqueue(&recent).unwrap();

    let removed = queue.prune_older_than(1000).unwrap();
    assert_eq!(removed, 1);
    assert_eq!(queue.count().unwrap(), 1);
    assert_eq!(queue.peek_batch(10).unwrap()[0].event_name, "recent");
}

/// The queue must never grow past MAX_QUEUE_LEN — enqueue evicts the
/// oldest event(s) in the same transaction as the insert.
#[test]
fn queue_is_bounded_and_evicts_oldest_first() {
    let (_dir, db) = open_tmp();
    let queue = db.telemetry();

    for i in 0..(MAX_QUEUE_LEN + 10) {
        queue
            .enqueue(&StudioTelemetryEvent::new(
                format!("event-{i}"),
                None,
                serde_json::json!({}),
                i as i64,
                TelemetryCategory::Analytics,
            ))
            .unwrap();
    }

    assert_eq!(queue.count().unwrap(), MAX_QUEUE_LEN);

    // The 10 oldest (created_at 0..10) must have been evicted; the newest
    // MAX_QUEUE_LEN must remain.
    let batch = queue.peek_batch(MAX_QUEUE_LEN).unwrap();
    let oldest_remaining = batch.first().unwrap();
    assert_eq!(oldest_remaining.created_at, 10);
    let newest_remaining = batch.last().unwrap();
    assert_eq!(newest_remaining.created_at, (MAX_QUEUE_LEN + 9) as i64);
}

#[test]
fn reopen_preserves_queue_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    let event = StudioTelemetryEvent::new(
        "app_launched",
        None,
        serde_json::json!({"k": "v"}),
        1000,
        TelemetryCategory::Analytics,
    );
    {
        let db = StudioDatabase::open(&path).unwrap();
        db.telemetry().enqueue(&event).unwrap();
    }
    {
        let db = StudioDatabase::open(&path).unwrap();
        let batch = db.telemetry().peek_batch(10).unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].event_id, event.event_id);
        assert_eq!(batch[0].payload, serde_json::json!({"k": "v"}));
    }
}

// ── Category / discard_category (S2c) ────────────────────────────────────

#[test]
fn events_default_to_analytics_category_when_missing_from_older_json() {
    // Simulates a row written before `category` existed — no `category`
    // key in the JSON at all.
    let old_json = br#"{"event_id":"e1","created_at":100,"event_name":"old_event"}"#;
    let event: StudioTelemetryEvent = serde_json::from_slice(old_json).unwrap();
    assert_eq!(event.category, TelemetryCategory::Analytics);
}

#[test]
fn discard_category_removes_only_matching_category() {
    let (_dir, db) = open_tmp();
    let queue = db.telemetry();

    let a1 = StudioTelemetryEvent::new(
        "a1",
        None,
        serde_json::json!({}),
        100,
        TelemetryCategory::Analytics,
    );
    let a2 = StudioTelemetryEvent::new(
        "a2",
        None,
        serde_json::json!({}),
        200,
        TelemetryCategory::Analytics,
    );
    let c1 = StudioTelemetryEvent::new(
        "c1",
        None,
        serde_json::json!({}),
        300,
        TelemetryCategory::Crash,
    );
    queue.enqueue(&a1).unwrap();
    queue.enqueue(&a2).unwrap();
    queue.enqueue(&c1).unwrap();

    let removed = queue
        .discard_category(TelemetryCategory::Analytics)
        .unwrap();
    assert_eq!(removed, 2);
    assert_eq!(queue.count().unwrap(), 1);
    let remaining = queue.peek_batch(10).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].event_name, "c1");
    assert_eq!(remaining[0].category, TelemetryCategory::Crash);
}

#[test]
fn discard_category_is_idempotent_on_an_empty_or_already_clean_category() {
    let (_dir, db) = open_tmp();
    let queue = db.telemetry();
    assert_eq!(
        queue
            .discard_category(TelemetryCategory::Analytics)
            .unwrap(),
        0
    );

    let c1 = StudioTelemetryEvent::new(
        "c1",
        None,
        serde_json::json!({}),
        100,
        TelemetryCategory::Crash,
    );
    queue.enqueue(&c1).unwrap();
    // Discarding analytics again does nothing to the crash-category row.
    assert_eq!(
        queue
            .discard_category(TelemetryCategory::Analytics)
            .unwrap(),
        0
    );
    assert_eq!(queue.count().unwrap(), 1);
}

#[test]
fn discard_category_after_reopen_still_only_affects_the_named_category() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.redb");
    {
        let db = StudioDatabase::open(&path).unwrap();
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "a",
                None,
                serde_json::json!({}),
                100,
                TelemetryCategory::Analytics,
            ))
            .unwrap();
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "c",
                None,
                serde_json::json!({}),
                200,
                TelemetryCategory::Crash,
            ))
            .unwrap();
    }
    {
        let db = StudioDatabase::open(&path).unwrap();
        let removed = db
            .telemetry()
            .discard_category(TelemetryCategory::Analytics)
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(db.telemetry().count().unwrap(), 1);
        assert_eq!(db.telemetry().peek_batch(10).unwrap()[0].event_name, "c");
    }
}
