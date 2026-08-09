# Phase S2b-2d & S2b-2d.1 — Telemetry Queue & Consent Boundary Migration

## Goal

Redirect the desktop telemetry queue from file-based I/O (`events.jsonl`) to
`studio.redb`'s `telemetry_queue` table, and cleanly route all consent decisions
through the canonical `StudioPreferencesService` (S2b-2d.1), completing the
transition to the typed-service architecture established by S2b-1 through S2b-2c.

## Delivered

### `desktop/src-tauri/src/telemetry.rs` [MODIFIED]

Complete rewrite of the mutable queue infrastructure and canonical consent routing:

| Removed | Reason |
|---|---|
| `QUEUE_FILE` const | No longer written to |
| `MAX_QUEUE_LINES` const | Cap now enforced by `TelemetryQueue::enqueue` |
| `QUEUE_LOCK` static | No file to protect |
| `queue_path()` fn | No file path needed |
| `enqueue()` fn | Replaced by `enqueue_to_db()` |
| `build_envelope()` fn | Replaced by `build_wire_envelope()` |
| old `drain_queue()` fn | Replaced with DB-backed version |
| direct `db.preferences().get()` in `analytics_consent()` | Routed through `StudioPreferencesService` (S2b-2d.1) |

| Added | Purpose |
|---|---|
| `DRAIN_BATCH_SIZE = 50` | Caps the in-memory batch per drain tick |
| `PRUNE_OLDER_THAN_MS = 7 days` | Backstop for permanently failing events (file sender had none) |
| `enqueue_to_db(app, event)` | Writes `StudioTelemetryEvent` to `db.telemetry().enqueue()` |
| `build_wire_envelope(install_id, event)` | Builds `TelemetryEnvelope` (wire format) from a stored event at drain time |
| new `drain_queue()` | Reads `peek_batch(DRAIN_BATCH_SIZE)`, POSTs each, calls `mark_delivered` on success or `increment_retry` on failure |
| Canonical `analytics_consent()` | Reads consent solely via `app.try_state::<StudioPreferencesService>()` |

`enqueue_telemetry_event` (Tauri command) signature unchanged — `installation_id`
parameter kept for JS API compatibility but not stored; the installation id is
read from the preferences table at drain time. `enqueue_update_event` and
`spawn_sender` public signatures are unchanged.

New unit tests added (11 total in telemetry module):
- `drain_batch_size_is_positive` — invariant
- `prune_older_than_ms_is_positive_and_covers_at_least_a_day` — invariant
- `build_wire_envelope_produces_correct_wire_shape` — round-trip check for the re-hydration logic
- `build_wire_envelope_handles_missing_session_id` — empty string, not null/missing
- `enqueue_to_db_writes_to_telemetry_queue` — verifies DB write path using a temp DB
- `analytics_disabled_service_returns_false_and_queue_stays_empty` — S2b-2d.1: analytics=false prevents queuing
- `analytics_enabled_service_returns_true_and_event_can_be_queued` — S2b-2d.1: analytics=true permits queuing
- `analytics_and_crash_consent_are_independent_fields` — S2b-2d.1: crash consent independent of analytics consent
- `consent_defaults_to_false_when_no_record_exists` — S2b-2d.1: fail-closed default
- `consent_persists_across_database_reopen` — S2b-2d.1: persistence across restart
- `telemetry_storage_and_consent_are_independent_concerns` — S2b-2d.1: boundary isolation between tables

### `desktop/src-tauri/src/lib.rs` [MODIFIED]

- Registered `StudioPreferencesService` as managed Tauri state (`app.manage(StudioPreferencesService::new(studio_db.clone()))`) so telemetry consent check can reach it.
- Updated the comment above `spawn_sender(...)` to state it now drains `studio.redb`'s `telemetry_queue` and that `events.jsonl` is a read-only legacy artifact.

### `crates/valori-studio-storage/` [UNCHANGED]

No changes. `TelemetryQueue` already had full coverage (9 tests).

## Findings

### Consent boundary cleanup (S2b-2d.1)

Telemetry consent is now resolved strictly through `StudioPreferencesService`:
```text
Telemetry
    ↓
StudioPreferencesService
    ↓
StudioDatabase
    ↓
preferences table
```
`telemetry.rs` never accesses the `preferences` table or `db.preferences()` directly. Telemetry queue persistence remains owned by `TelemetryQueue` / `studio.redb`.

### `installation_id` not stored per-event

`TelemetryEnvelope` (wire format) carries an `installation_id` field, but
`StudioTelemetryEvent` (storage format) does not. Installation id is now read
once per drain tick from the preferences table and stamped on every envelope at
send time. This is simpler than storing it redundantly in every queue row, and
correct because installation id is stable and always accessible.

### Backstop for unbounded retry accumulation

`prune_older_than(cutoff)` is called on each drain tick with `cutoff = now - 7 days`.
The file-based sender had no such backstop. This closes the gap the storage audit
flagged in §6/§18.

## Validation

```
valori-studio-storage: 87 tests — 87 passed, 0 failed
desktop/src-tauri:     22 tests — 22 passed, 0 failed
Total:                109 passed, 0 failed
```

Commands:
```bash
cargo test -p valori-studio-storage     # 87 passed
cargo test                               # 22 passed (from desktop/src-tauri/)
```

## Follow-ups

| Item | Phase |
|---|---|
| Update state & sync state migration | S2b-2e |
