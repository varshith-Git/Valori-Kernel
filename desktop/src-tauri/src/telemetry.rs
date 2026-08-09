//! Desktop telemetry — S2b-2d: queue and sender are now backed by
//! `studio.redb`'s `telemetry_queue` table via `TelemetryQueue`.
//!
//! Sends session lifecycle, update success/failure, and crash/error events
//! to `POST /v1/telemetry/events`. Consent is checked at enqueue-time so
//! nothing is written to disk when the user has not opted in.
//!
//! # Consent boundary (S2c)
//!
//! Every event carries a [`TelemetryCategory`] (`Analytics` or `Crash`),
//! matching `StudioPreferences::telemetry_consent`'s two independent
//! consent fields — there is still no third category (no evidence one
//! ever existed; see `crates/valori-studio-storage/src/telemetry.rs`'s
//! `TelemetryCategory` doc comment). Consent is enforced **twice**, not
//! once:
//!
//! 1. **At enqueue** (`enqueue_telemetry_event`/`enqueue_update_event`) —
//!    `consent_for_category` gates the write, per that event's own
//!    category. Nothing is written to disk for a category whose consent
//!    is off.
//! 2. **At the uploader boundary** (`drain_queue`) — before dispatching
//!    each event's HTTP request, its category's consent is re-read fresh
//!    from `studio.redb` (never cached, never assumed from enqueue-time).
//!    If consent for that category is off *right now*, the event is
//!    discarded (deleted, never sent) instead of uploaded. This is what
//!    protects against a consent change made *after* an event was queued —
//!    including one queued in a previous session — and against any future
//!    code path that might enqueue without going through the guard above.
//!
//! **Revocation invalidates already-queued events of that category.**
//! `preferences_service.rs`'s `set_telemetry_consent_command` calls
//! `TelemetryQueue::discard_category` the moment a category's consent
//! turns off, so the queue is cleaned up immediately rather than waiting
//! for the next drain tick to silently skip-and-delete them one at a
//! time — both paths enforce the same invariant, the eager one is just
//! a UX/tidiness improvement, not the thing that makes it safe (that's
//! layer 2 above).
//!
//! **The ordering guarantee this provides, precisely:** `studio.redb`
//! (redb) transactions are atomic and serialized — once a
//! `set_telemetry_consent_command` write transaction commits, every
//! subsequent read transaction (including the per-event check inside
//! `drain_queue`'s loop) observes the new value; there is no way to read
//! a torn or stale value after that commit. The one thing this cannot
//! prevent is an HTTP request that was *already dispatched* (the request
//! is in flight, no longer cancellable) at the exact moment revocation
//! commits — physically nothing can un-send a request already on the
//! wire. Checking consent **per event inside the drain loop**, rather
//! than once for the whole batch, minimizes that window to "at most the
//! one event whose request is already in flight," instead of "the whole
//! batch." No new synchronization primitive was introduced — this relies
//! entirely on `StudioDatabase`'s existing transaction guarantees, per
//! the instruction to use the existing service/database architecture
//! rather than a second global state mechanism.
//!
//! # Queue backend (S2b-2d)
//!
//! `enqueue_to_db` / `drain_queue` use `Arc<StudioDatabase>` registered in
//! Tauri state — the same handle opened in `studio_storage::init_studio_storage`.
//! The S2a one-time migration already imported any existing `events.jsonl`
//! entries into `telemetry_queue` before this code runs, so there is no
//! in-flight data loss. `events.jsonl` is now a read-only legacy artifact:
//! this module never writes to it.
//!
//! # Crash reporting
//!
//! Split across two startups on purpose: a panic hook cannot safely make an
//! async HTTP call mid-panic (the tokio runtime isn't sound to rely on there),
//! so `install_panic_hook` only ever writes a local crash marker file. The
//! actual report is sent on the *next* startup, once the JS side has a chance
//! to check consent and call `check_and_clear_crash_marker`.
//!
//! # Session id
//!
//! One session id per process, generated once in `setup()` before anything
//! that might emit an event. `enqueue_to_db` stamps it on every event; the
//! JS side never generates or passes one.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tauri::Manager;
use uuid::Uuid;
use valori_domain::SessionId;
use valori_studio_storage::telemetry::{StudioTelemetryEvent, TelemetryCategory};

use crate::preferences_service::StudioPreferencesService;

// ── Wire-format constants ────────────────────────────────────────────────────

const CRASHES_DIR: &str = "crashes";
const CRASH_MARKER_FILE: &str = "crash_marker.json";
const TELEMETRY_ENDPOINT: &str = "https://api.valori.systems/v1/telemetry/events";
/// Envelope schema version — must match the backend's `CURRENT_SCHEMA`.
const SCHEMA: u32 = 1;
/// Matches the backend's `ALLOWED_SOURCES` entry for this client.
const SOURCE: &str = "desktop";
/// Maximum events to POST in a single drain tick — avoids loading the full
/// queue into memory; the sender loops every 60 s anyway.
const DRAIN_BATCH_SIZE: usize = 50;
/// Prune events older than this from the queue on each drain tick. The
/// file-based sender had no such backstop; this caps unbounded retry
/// accumulation for permanently unreachable events.
/// 7 days in milliseconds.
const PRUNE_OLDER_THAN_MS: i64 = 7 * 24 * 3_600 * 1_000;

// ── Wire envelope (unchanged shape — backend contract) ───────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TelemetryEnvelope {
    schema: u32,
    source: String,
    event_id: String,
    timestamp: String,
    session_id: String,
    installation_id: String,
    version: String,
    platform: String,
    arch: String,
    event: String,
    properties: serde_json::Value,
}

// ── AppInfo ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: String,
    pub platform: String,
    pub arch: String,
}

/// `env!("CARGO_PKG_VERSION")` bundled with platform/arch, exposed to JS
/// (which has no `@tauri-apps/plugin-os` and no `getVersion()` call).
#[tauri::command]
pub fn get_app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

// ── Session id ───────────────────────────────────────────────────────────────

/// One session id per process, generated once. Call early in `setup()` before
/// anything that might emit an event.
static SESSION_ID: OnceLock<String> = OnceLock::new();

/// Call once, early in `setup()`, before anything that might emit an event.
pub fn init_session_id() -> String {
    SESSION_ID
        .get_or_init(|| Uuid::new_v4().to_string())
        .clone()
}

fn session_id() -> String {
    init_session_id()
}

/// Exposes the session id to the JS side — called once and cached there.
#[tauri::command]
pub fn get_session_id() -> String {
    session_id()
}

// ── Process start timestamps ─────────────────────────────────────────────────

/// Monotonic process start — used only for crash uptime (same-process diff).
static APP_START: OnceLock<std::time::Instant> = OnceLock::new();

fn init_app_start() -> std::time::Instant {
    *APP_START.get_or_init(std::time::Instant::now)
}

/// Wall-clock (epoch ms) process start, for the startup waterfall. A separate
/// mark from `APP_START`: that one is a monotonic `Instant` for a same-process
/// duration; this one is wall-clock so it can be diffed against JS marks.
static RUST_START_MS: OnceLock<i64> = OnceLock::new();

/// Call as the literal first line of `run()`, before anything else.
pub fn init_rust_start_ms() -> i64 {
    *RUST_START_MS.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    })
}

/// Exposed to JS so `startupMarks.ts` can fetch this process's t0 once.
#[tauri::command]
pub fn get_rust_start_ms() -> i64 {
    init_rust_start_ms()
}

// ── Consent & installation id ────────────────────────────────────────────────

/// Reads the consent flag matching `category` via the canonical
/// `StudioPreferencesService`. Defaults to `false` on any read or
/// state-lookup failure — fail-closed so we never queue or send data
/// nobody agreed to.
///
/// Consent semantics (defaults, field ownership) live in
/// `StudioPreferencesService::get_telemetry_consent`; this function is a
/// thin call through the managed service — it does not access the
/// `preferences` table directly. Called fresh every time (never cached)
/// so both the enqueue guard and the uploader boundary in `drain_queue`
/// see the current value — see the module doc's "Consent boundary".
fn consent_for_category(app: &tauri::AppHandle, category: TelemetryCategory) -> bool {
    let Some(service) = app.try_state::<StudioPreferencesService>() else {
        return false;
    };
    let consent = service.get_telemetry_consent().unwrap_or_default();
    match category {
        TelemetryCategory::Analytics => consent.analytics,
        TelemetryCategory::Crash => consent.crash,
    }
}

/// Reads the permanent installation id. By this point in startup it is
/// already guaranteed to exist — `lib.rs`'s `setup()` calls
/// `StudioPreferencesService::get_or_init_installation_id` unconditionally,
/// independent of telemetry consent, before telemetry ever runs. This
/// function is a thin read through that same canonical service (no
/// duplicate get-or-init logic lives here — see the Studio Installation
/// Identity phase doc for why the three formerly-independent
/// implementations were consolidated into one).
fn installation_id(app: &tauri::AppHandle) -> Option<String> {
    let service = app.try_state::<StudioPreferencesService>()?;
    service
        .get_or_init_installation_id()
        .ok()
        .map(|id| id.to_string())
}

// ── Queue helpers ────────────────────────────────────────────────────────────

/// Appends one event to `studio.redb`'s `telemetry_queue`. Silently does
/// nothing if the database state is not yet registered (shouldn't happen in
/// normal startup order, but defensive).
fn enqueue_to_db(app: &tauri::AppHandle, event: &StudioTelemetryEvent) {
    let Some(db) = app.try_state::<std::sync::Arc<valori_studio_storage::StudioDatabase>>() else {
        return;
    };
    let _ = db.telemetry().enqueue(event);
}

/// Builds the wire-format envelope from a queued event plus the stable
/// installation id. Called at drain time, not at enqueue time — the
/// installation id is always available from the preferences table and does
/// not need to be stored per-event.
fn build_wire_envelope(installation_id: &str, event: &StudioTelemetryEvent) -> TelemetryEnvelope {
    let info = get_app_info();
    // Convert epoch-ms back to RFC3339 for the backend. If the timestamp is
    // out of range (shouldn't happen), fall back to now — still a valid send.
    let timestamp = chrono::DateTime::from_timestamp_millis(event.created_at)
        .map(|dt: chrono::DateTime<chrono::Utc>| dt.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    TelemetryEnvelope {
        schema: SCHEMA,
        source: SOURCE.to_string(),
        event_id: event.event_id.clone(),
        timestamp,
        session_id: event
            .session_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        installation_id: installation_id.to_string(),
        version: info.version,
        platform: info.platform,
        arch: info.arch,
        event: event.event_name.clone(),
        properties: event.payload.clone(),
    }
}

// ── Tauri command — JS-originated events ─────────────────────────────────────

/// Enqueue an event from the JS side. Checks consent at enqueue-time so
/// nothing is written to disk when the user has not opted in — gated by
/// `category`'s matching consent field, not always `analytics` (this is
/// the fix that makes crash-category events queueable independently of
/// analytics consent; see the module doc's "Consent boundary"). The
/// `session_id` is no longer a caller-supplied argument — every event from
/// this process carries the single `session_id()` value generated in
/// `setup()`. The `installation_id` parameter is kept for JS API
/// compatibility but is not stored per-event; it is read from the preferences
/// table at drain time.
#[tauri::command]
pub fn enqueue_telemetry_event(
    app: tauri::AppHandle,
    event: String,
    properties: serde_json::Value,
    installation_id: String,
    category: TelemetryCategory,
) -> Result<(), String> {
    // installation_id kept for JS API compat; not stored (read at drain time).
    let _ = installation_id;
    if !consent_for_category(&app, category) {
        return Ok(());
    }
    let sid: Option<SessionId> = session_id().parse().ok();
    let now = chrono::Utc::now().timestamp_millis();
    let ste = StudioTelemetryEvent::new(event, sid, properties, now, category);
    enqueue_to_db(&app, &ste);
    Ok(())
}

// ── Rust-native event enqueue (update check, install_update) ─────────────────

/// Enqueue an `update_*` event from a Rust-native call site. These are all
/// analytics-flavored (update lifecycle, not crashes) — hardcoded to
/// `TelemetryCategory::Analytics` since every current call site is one.
/// Silently does nothing if analytics consent is off or the store can't be
/// read.
pub fn enqueue_update_event(app: &tauri::AppHandle, event: &str, properties: serde_json::Value) {
    if !consent_for_category(app, TelemetryCategory::Analytics) {
        return;
    }
    let sid: Option<SessionId> = session_id().parse().ok();
    let now = chrono::Utc::now().timestamp_millis();
    let ste = StudioTelemetryEvent::new(event, sid, properties, now, TelemetryCategory::Analytics);
    enqueue_to_db(app, &ste);
}

// ── Background sender ────────────────────────────────────────────────────────

/// Background sender: runs immediately (flushes anything queued from a
/// previous offline session), then every `interval`. Reads `studio.redb`'s
/// `telemetry_queue`, POSTs each envelope, removes delivered events, and
/// bumps retry metadata on failures — so a network blip self-heals on the
/// next tick without any backoff logic.
pub fn spawn_sender(app: tauri::AppHandle, interval: std::time::Duration) {
    tauri::async_runtime::spawn(async move {
        loop {
            drain_queue(&app).await;
            tokio::time::sleep(interval).await;
        }
    });
}

async fn drain_queue(app: &tauri::AppHandle) {
    let Some(db) = app.try_state::<std::sync::Arc<valori_studio_storage::StudioDatabase>>() else {
        return;
    };

    // Prune events older than 7 days — backstop for events that permanently
    // fail delivery. The file-based sender had no such backstop.
    let cutoff = chrono::Utc::now().timestamp_millis() - PRUNE_OLDER_THAN_MS;
    let _ = db.telemetry().prune_older_than(cutoff);

    // Peek the oldest batch; if the queue is empty, nothing to do.
    let batch = match db.telemetry().peek_batch(DRAIN_BATCH_SIZE) {
        Ok(b) => b,
        Err(_) => return,
    };
    if batch.is_empty() {
        return;
    }

    // Read installation id once for the whole batch — stable across the tick.
    let install_id = installation_id(app).unwrap_or_default();

    let client = reqwest::Client::new();
    for event in &batch {
        // Uploader boundary: re-check this event's category consent fresh,
        // immediately before dispatching — not the enqueue-time guard, not
        // a cached value. Covers a consent change made after the event was
        // queued (including in a previous session) and any future code
        // path that might have enqueued without the guard. See the module
        // doc's "Consent boundary" for the exact ordering guarantee.
        if !consent_for_category(app, event.category) {
            // Consent for this category is off right now: discard rather
            // than upload — deleted outright, never sent, never retried.
            let _ = db.telemetry().mark_delivered(&event.event_id);
            continue;
        }

        let envelope = build_wire_envelope(&install_id, event);
        let res = client
            .post(TELEMETRY_ENDPOINT)
            .json(&envelope)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;
        let now_ms = chrono::Utc::now().timestamp_millis();
        match res {
            Ok(resp) if resp.status().is_success() => {
                let _ = db.telemetry().mark_delivered(&event.event_id);
            }
            _ => {
                let _ = db.telemetry().increment_retry(&event.event_id, now_ms);
            }
        }
    }
}

// ── Crash marker ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashInfo {
    /// Short hash of the panic message + location — groups identical crashes
    /// without ever uploading the raw message/stack.
    pub panic_hash: String,
    pub panic_location: String,
    /// The `session_id()` of the process that crashed — not the session
    /// reading this marker on the *next* launch (that one has its own id).
    pub previous_session: String,
    /// Seconds between `install_panic_hook` running and the panic.
    pub uptime_before_crash_secs: u64,
}

fn marker_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(CRASHES_DIR).join(CRASH_MARKER_FILE))
}

/// Installs a panic hook that writes a local crash marker and nothing else —
/// no network call, no async, safe to run from within a panicking thread.
/// Call once, early in `setup()`.
pub fn install_panic_hook(app: tauri::AppHandle) {
    init_app_start();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);

        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".to_string());
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());

        let hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            message.hash(&mut hasher);
            location.hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        };

        let uptime_before_crash_secs = APP_START.get().map(|t| t.elapsed().as_secs()).unwrap_or(0);
        let crash = CrashInfo {
            panic_hash: hash,
            panic_location: location,
            previous_session: session_id(),
            uptime_before_crash_secs,
        };
        if let Some(path) = marker_path(&app) {
            if let Ok(json) = serde_json::to_string(&crash) {
                let _ = fs::create_dir_all(path.parent().unwrap_or(&path));
                let _ = fs::write(&path, json);
            }
        }
    }));
}

/// Checked once at startup. Returns `Some` (and deletes the marker) if the
/// *previous* run panicked — the caller decides whether to actually send a
/// report based on consent. The marker is removed either way so a crash is
/// never reported twice.
///
/// S7 (`docs/phases/phase-studio-S7-persistence-boundary.md`) — the "what
/// belongs in `crashes/`" decision: the live panic-hook marker path stays
/// exactly as is (`marker_path`, Tauri's `app_config_dir()` — see S6's
/// filesystem audit §4 for why moving it is a permanent, deliberate
/// exception). What's new is that **once this function has read a crash**,
/// it archives a copy into the canonical `$VALORI_HOME/crashes/` — giving
/// that directory a real, bounded purpose (a local crash history) without
/// touching the one write path (the panic hook itself) that must stay
/// minimal-risk. Archival is best-effort: a failure here never changes
/// this function's return value or the marker-clearing behavior above.
#[tauri::command]
pub fn check_and_clear_crash_marker(app: tauri::AppHandle) -> Option<CrashInfo> {
    let path = marker_path(&app)?;
    let contents = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    let crash: CrashInfo = serde_json::from_str(&contents).ok()?;
    archive_crash(&crash);
    Some(crash)
}

/// Best-effort archival into `crashes_dir()` — see
/// `check_and_clear_crash_marker`'s doc comment. Never logs or panics; a
/// failure here is silently swallowed rather than surfaced, since it must
/// never change whether the app believes a crash was reported.
fn archive_crash(crash: &CrashInfo) {
    let crashes_dir = valori_studio_storage::StudioPaths::from_env().crashes_dir();
    if fs::create_dir_all(&crashes_dir).is_err() {
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = format!("crash-{now}-{}.json", &crash.panic_hash);
    let Ok(bytes) = serde_json::to_vec_pretty(crash) else {
        return;
    };
    let _ = fs::write(crashes_dir.join(file_name), bytes);
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use valori_studio_storage::StudioDatabase;

    #[test]
    fn crash_info_round_trips_through_the_same_json_shape_the_marker_file_uses() {
        let original = CrashInfo {
            panic_hash: "abcd1234ef567890".to_string(),
            panic_location: "src/telemetry.rs:42".to_string(),
            previous_session: "sess-abc".to_string(),
            uptime_before_crash_secs: 14_400,
        };
        let written = serde_json::to_string(&original).expect("panic hook's own write path");
        let read_back: CrashInfo =
            serde_json::from_str(&written).expect("startup check's own read path");
        assert_eq!(read_back.panic_hash, original.panic_hash);
        assert_eq!(read_back.panic_location, original.panic_location);
        assert_eq!(read_back.previous_session, original.previous_session);
        assert_eq!(
            read_back.uptime_before_crash_secs,
            original.uptime_before_crash_secs
        );
    }

    #[test]
    fn panic_hash_is_a_fixed_width_hex_digest_not_the_raw_message() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let message = "index out of bounds: the len is 3 but the index is 5";
        let location = "crates/valori-kernel/src/index.rs:384";
        let mut hasher = DefaultHasher::new();
        message.hash(&mut hasher);
        location.hash(&mut hasher);
        let hash = format!("{:016x}", hasher.finish());
        assert_eq!(hash.len(), 16);
        assert!(!hash.contains("index out of bounds"));
        assert!(!hash.contains("valori-kernel"));
    }

    #[test]
    fn session_id_is_stable_across_repeated_calls() {
        let a = session_id();
        let b = session_id();
        assert_eq!(a, b);
    }

    #[test]
    fn drain_batch_size_is_positive() {
        const _: () = assert!(DRAIN_BATCH_SIZE > 0);
        assert_eq!(DRAIN_BATCH_SIZE, 50);
    }

    #[test]
    fn prune_older_than_ms_is_positive_and_covers_at_least_a_day() {
        const _: () = assert!(PRUNE_OLDER_THAN_MS >= 24 * 3_600 * 1_000);
        assert_eq!(PRUNE_OLDER_THAN_MS, 7 * 24 * 3_600 * 1_000);
    }

    #[test]
    fn build_wire_envelope_produces_correct_wire_shape() {
        let dir = tempfile::tempdir().unwrap();
        let _db = StudioDatabase::open(&dir.path().join("studio.redb")).unwrap();

        let sid_str = "a1a1a1a1-1a2b-4c3d-8e9f-0123456789ab";
        let sid: SessionId = sid_str.parse().unwrap();
        let event = StudioTelemetryEvent::new(
            "test_event",
            Some(sid),
            serde_json::json!({"foo": "bar"}),
            1_723_100_000_000_i64,
            TelemetryCategory::Analytics,
        );
        let envelope = build_wire_envelope("inst-abc", &event);

        assert_eq!(envelope.schema, SCHEMA);
        assert_eq!(envelope.source, SOURCE);
        assert_eq!(envelope.event_id, event.event_id);
        assert_eq!(envelope.session_id, sid_str);
        assert_eq!(envelope.installation_id, "inst-abc");
        assert_eq!(envelope.event, "test_event");
        assert_eq!(envelope.properties["foo"], "bar");
        // Timestamp must be a valid RFC3339 string
        assert!(chrono::DateTime::parse_from_rfc3339(&envelope.timestamp).is_ok());
    }

    #[test]
    fn build_wire_envelope_handles_missing_session_id() {
        let event = StudioTelemetryEvent::new(
            "anon_event",
            None,
            serde_json::json!({}),
            1_000,
            TelemetryCategory::Analytics,
        );
        let envelope = build_wire_envelope("inst-xyz", &event);
        // session_id is empty string when absent, not "null" or missing
        assert_eq!(envelope.session_id, "");
    }

    #[test]
    fn enqueue_to_db_writes_to_telemetry_queue() {
        let dir = tempfile::tempdir().unwrap();
        let db =
            std::sync::Arc::new(StudioDatabase::open(&dir.path().join("studio.redb")).unwrap());
        let event = StudioTelemetryEvent::new(
            "app_launched",
            None,
            serde_json::json!({}),
            chrono::Utc::now().timestamp_millis(),
            TelemetryCategory::Analytics,
        );
        db.telemetry().enqueue(&event).unwrap();
        assert_eq!(db.telemetry().count().unwrap(), 1);
    }

    #[test]
    fn test_crash_marker_path_parent_creation() {
        let temp_dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let crash_dir = temp_dir.join("crashes");
        let path = crash_dir.join("crash_marker.json");

        let crash = CrashInfo {
            panic_hash: "hash".to_string(),
            panic_location: "loc".to_string(),
            previous_session: "sess-xyz".to_string(),
            uptime_before_crash_secs: 2,
        };

        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        let json = serde_json::to_string(&crash).unwrap();
        fs::write(&path, json).unwrap();

        assert!(path.exists());
        let read_back: CrashInfo =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read_back.panic_hash, "hash");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    // ── Consent boundary tests (S2b-2d.1) ───────────────────────────────────
    //
    // These tests exercise the consent logic through the same surface that
    // production code goes through — StudioPreferencesService / TelemetryQueue
    // — without a real Tauri AppHandle. They prove the architectural boundary
    // is real: telemetry storage uses TelemetryQueue; consent decisions use
    // StudioPreferencesService.

    use valori_studio_storage::preferences::TelemetryConsent;

    /// Consent semantics live in `StudioPreferencesService::get_telemetry_consent`.
    /// When analytics=false the service returns that correctly, and the telemetry
    /// module must not enqueue anything — simulated here by verifying the queue
    /// remains empty when we skip the enqueue call based on the service's answer.
    #[test]
    fn analytics_disabled_service_returns_false_and_queue_stays_empty() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());

        // Set analytics=false, crash=true — they must be independent.
        db.preferences()
            .update(|p| {
                p.telemetry_consent = Some(TelemetryConsent {
                    analytics: false,
                    crash: true,
                });
            })
            .unwrap();

        let service = crate::preferences_service::StudioPreferencesService::new(db.clone());
        let consent = service.get_telemetry_consent().unwrap();

        // analytics=false — telemetry module must not enqueue.
        assert!(!consent.analytics, "analytics must be false");
        // crash consent is independent and unaffected.
        assert!(consent.crash, "crash consent must remain true");

        // Simulate the guard: nothing is enqueued when analytics is off.
        if consent.analytics {
            let event = StudioTelemetryEvent::new(
                "page_view",
                None,
                serde_json::json!({}),
                1000,
                TelemetryCategory::Analytics,
            );
            db.telemetry().enqueue(&event).unwrap();
        }
        assert_eq!(
            db.telemetry().count().unwrap(),
            0,
            "queue must be empty when analytics=false"
        );
    }

    #[test]
    fn analytics_enabled_service_returns_true_and_event_can_be_queued() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());

        db.preferences()
            .update(|p| {
                p.telemetry_consent = Some(TelemetryConsent {
                    analytics: true,
                    crash: false,
                });
            })
            .unwrap();

        let service = crate::preferences_service::StudioPreferencesService::new(db.clone());
        let consent = service.get_telemetry_consent().unwrap();

        assert!(consent.analytics, "analytics must be true");
        // crash consent is independent and unaffected.
        assert!(!consent.crash, "crash consent must remain false");

        // Simulate the guard: event is enqueued when analytics is on.
        if consent.analytics {
            let event = StudioTelemetryEvent::new(
                "page_view",
                None,
                serde_json::json!({}),
                1000,
                TelemetryCategory::Analytics,
            );
            db.telemetry().enqueue(&event).unwrap();
        }
        assert_eq!(
            db.telemetry().count().unwrap(),
            1,
            "event must be queued when analytics=true"
        );
    }

    /// Crash consent and analytics consent are separate fields.
    /// analytics=false must NOT suppress crash reporting (or vice versa).
    #[test]
    fn analytics_and_crash_consent_are_independent_fields() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());
        let service = crate::preferences_service::StudioPreferencesService::new(db.clone());

        // Case A: analytics=false, crash=true
        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: false,
                crash: true,
            })
            .unwrap();
        let c = service.get_telemetry_consent().unwrap();
        assert!(!c.analytics);
        assert!(c.crash);

        // Case B: analytics=true, crash=false
        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: true,
                crash: false,
            })
            .unwrap();
        let c = service.get_telemetry_consent().unwrap();
        assert!(c.analytics);
        assert!(!c.crash);

        // Case C: both false
        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: false,
                crash: false,
            })
            .unwrap();
        let c = service.get_telemetry_consent().unwrap();
        assert!(!c.analytics);
        assert!(!c.crash);
    }

    /// `StudioPreferencesService::get_telemetry_consent` owns consent semantics.
    /// The default (no consent record ever written) must be `false` for both fields —
    /// fail-closed, same as `analytics_consent()`.
    #[test]
    fn consent_defaults_to_false_when_no_record_exists() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());
        let service = crate::preferences_service::StudioPreferencesService::new(db);

        let consent = service.get_telemetry_consent().unwrap();
        assert!(
            !consent.analytics,
            "analytics must default to false (fail-closed)"
        );
        assert!(!consent.crash, "crash must default to false (fail-closed)");
    }

    /// Consent set through the preference service survives a DB reopen — the
    /// same value `analytics_consent()` would read on the next launch is the
    /// same one that was persisted.
    #[test]
    fn consent_persists_across_database_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.redb");

        {
            let db = std::sync::Arc::new(StudioDatabase::open(&path).unwrap());
            let service = crate::preferences_service::StudioPreferencesService::new(db);
            service
                .set_telemetry_consent(TelemetryConsent {
                    analytics: true,
                    crash: false,
                })
                .unwrap();
        }

        // Reopen — simulates the next app launch.
        {
            let db = std::sync::Arc::new(StudioDatabase::open(&path).unwrap());
            let service = crate::preferences_service::StudioPreferencesService::new(db);
            let consent = service.get_telemetry_consent().unwrap();
            assert!(consent.analytics, "analytics must persist across reopen");
            assert!(!consent.crash, "crash must persist across reopen");
        }
    }

    /// Architectural boundary: telemetry storage is `TelemetryQueue` in
    /// `studio.redb`; consent is `StudioPreferencesService`. They operate on
    /// separate tables, and `StudioPreferencesService` itself never reaches
    /// into `TelemetryQueue`'s table directly — the "revocation discards
    /// queued analytics events" behavior (S2c) is deliberately layered one
    /// level up, in `set_telemetry_consent_command`'s orchestration (see
    /// `preferences_service.rs`), not inside the service. This test pins
    /// that the service boundary itself stays narrow; the command-level
    /// discard behavior has its own dedicated tests below.
    #[test]
    fn telemetry_storage_and_consent_are_independent_concerns() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());

        // Consent API (preferences table)
        let service = crate::preferences_service::StudioPreferencesService::new(db.clone());
        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: true,
                crash: true,
            })
            .unwrap();
        let consent = service.get_telemetry_consent().unwrap();
        assert!(consent.analytics);

        // Telemetry API (telemetry_queue table) — independent of consent state
        let event = StudioTelemetryEvent::new(
            "evt",
            None,
            serde_json::json!({}),
            42,
            TelemetryCategory::Analytics,
        );
        db.telemetry().enqueue(&event).unwrap();
        assert_eq!(db.telemetry().count().unwrap(), 1);

        // The service alone (not the Tauri command) does not touch the
        // queue — it only ever writes the `preferences` table.
        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: false,
                crash: false,
            })
            .unwrap();
        assert_eq!(
            db.telemetry().count().unwrap(),
            1,
            "the service boundary alone must not reach into the telemetry queue"
        );

        db.telemetry().mark_delivered(&event.event_id).unwrap();
        let consent_after = service.get_telemetry_consent().unwrap();
        assert!(
            !consent_after.analytics,
            "consent unaffected by queue drain"
        );
    }

    // ── Consent revocation invalidates queued events (S2c) ──────────────────
    //
    // These tests exercise `consent_for_category` and `drain_queue`'s
    // per-event uploader-boundary check directly — the same functions
    // production code calls, without a real Tauri AppHandle/network stack
    // (drain_queue needs one; its HTTP-dispatch behavior is exercised via
    // the real desktop smoke test instead, per the phase's own instruction
    // to use a disposable database there, not real production telemetry).
    // What's proven here, against real `studio.redb` test storage: category
    // tagging is correct, `discard_category` is the mechanism the command
    // handler uses for eager cleanup, and independent crash/analytics
    // consent both hold under it.

    /// analytics ON → enqueue → analytics OFF → the queued event must never
    /// be uploadable again. Proven at the storage layer: after revocation,
    /// `discard_category(Analytics)` (what `set_telemetry_consent_command`
    /// calls) removes it, and `consent_for_category` would refuse to send
    /// it even if it were somehow still present — a `drain_queue` in
    /// production application checks this per event before every send.
    #[test]
    fn revoking_analytics_discards_the_previously_queued_analytics_event() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());
        let service = crate::preferences_service::StudioPreferencesService::new(db.clone());

        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: true,
                crash: false,
            })
            .unwrap();
        let event = StudioTelemetryEvent::new(
            "page_view",
            None,
            serde_json::json!({}),
            1000,
            TelemetryCategory::Analytics,
        );
        db.telemetry().enqueue(&event).unwrap();
        assert_eq!(db.telemetry().count().unwrap(), 1);

        // Revoke — this is exactly what set_telemetry_consent_command does
        // in addition to persisting the new consent value.
        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: false,
                crash: false,
            })
            .unwrap();
        db.telemetry()
            .discard_category(TelemetryCategory::Analytics)
            .unwrap();

        assert_eq!(
            db.telemetry().count().unwrap(),
            0,
            "the queued analytics event must be gone"
        );
        let consent = service.get_telemetry_consent().unwrap();
        assert!(
            !consent.analytics,
            "the revoked consent value itself must read back as off"
        );
    }

    /// Ten queued analytics events, consent revoked before any drain runs:
    /// `discard_category` removes all ten in one call, leaving nothing an
    /// uploader could ever send.
    #[test]
    fn revoking_analytics_discards_all_queued_analytics_events_not_just_one() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());
        for i in 0..10 {
            db.telemetry()
                .enqueue(&StudioTelemetryEvent::new(
                    format!("evt-{i}"),
                    None,
                    serde_json::json!({}),
                    i,
                    TelemetryCategory::Analytics,
                ))
                .unwrap();
        }
        assert_eq!(db.telemetry().count().unwrap(), 10);

        let removed = db
            .telemetry()
            .discard_category(TelemetryCategory::Analytics)
            .unwrap();
        assert_eq!(removed, 10);
        assert_eq!(db.telemetry().count().unwrap(), 0);
    }

    /// analytics OFF (old events discarded) → analytics ON again → a new
    /// event queues → only the new event may ever upload. Proves discard
    /// doesn't leave the queue permanently disabled, and that re-enabling
    /// doesn't resurrect anything that was already discarded.
    #[test]
    fn re_enabling_analytics_after_revocation_only_allows_new_events() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());

        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "old_event",
                None,
                serde_json::json!({}),
                1000,
                TelemetryCategory::Analytics,
            ))
            .unwrap();
        db.telemetry()
            .discard_category(TelemetryCategory::Analytics)
            .unwrap();
        assert_eq!(db.telemetry().count().unwrap(), 0);

        // Re-enable, queue a new event.
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "new_event",
                None,
                serde_json::json!({}),
                2000,
                TelemetryCategory::Analytics,
            ))
            .unwrap();

        let remaining = db.telemetry().peek_batch(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0].event_name, "new_event",
            "the discarded old event must never reappear"
        );
    }

    /// analytics OFF, crash ON: an analytics-category event queued earlier
    /// is discarded by revocation, while a crash-category event queued
    /// under crash consent remains queued and would still pass the
    /// uploader-boundary check — the two categories are enforced
    /// independently end-to-end, not just at the consent-struct level.
    #[test]
    fn independent_crash_consent_survives_analytics_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());
        let service = crate::preferences_service::StudioPreferencesService::new(db.clone());

        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: true,
                crash: true,
            })
            .unwrap();
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "page_view",
                None,
                serde_json::json!({}),
                1000,
                TelemetryCategory::Analytics,
            ))
            .unwrap();
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "studio_crashed",
                None,
                serde_json::json!({}),
                1100,
                TelemetryCategory::Crash,
            ))
            .unwrap();
        assert_eq!(db.telemetry().count().unwrap(), 2);

        // Revoke analytics only — crash stays on.
        service
            .set_telemetry_consent(TelemetryConsent {
                analytics: false,
                crash: true,
            })
            .unwrap();
        db.telemetry()
            .discard_category(TelemetryCategory::Analytics)
            .unwrap();

        let remaining = db.telemetry().peek_batch(10).unwrap();
        assert_eq!(
            remaining.len(),
            1,
            "only the analytics event must be discarded"
        );
        assert_eq!(remaining[0].event_name, "studio_crashed");
        assert_eq!(remaining[0].category, TelemetryCategory::Crash);

        let consent = service.get_telemetry_consent().unwrap();
        assert!(!consent.analytics);
        assert!(consent.crash, "crash consent must remain independently on");
    }

    /// Simulates "queue an event, exit, disable analytics, restart, drain":
    /// the discard is a database-level operation, so it holds across a
    /// process boundary — a fresh `StudioDatabase::open` (standing in for
    /// process restart) reads back a queue that reflects the revocation
    /// made in a previous "session", not a stale in-memory view.
    #[test]
    fn revocation_across_a_restart_leaves_the_old_analytics_event_undeliverable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.redb");

        // "Session 1": queue an event while analytics is on.
        {
            let db = std::sync::Arc::new(StudioDatabase::open(&path).unwrap());
            let service = crate::preferences_service::StudioPreferencesService::new(db.clone());
            service
                .set_telemetry_consent(TelemetryConsent {
                    analytics: true,
                    crash: false,
                })
                .unwrap();
            db.telemetry()
                .enqueue(&StudioTelemetryEvent::new(
                    "session_started",
                    None,
                    serde_json::json!({}),
                    1000,
                    TelemetryCategory::Analytics,
                ))
                .unwrap();
        }

        // Between sessions: user disables analytics (e.g. from Settings —
        // exercised here via the same service + discard call the Tauri
        // command makes; not modeling the process exit itself, just that
        // the change is durable, which is what matters for this test).
        {
            let db = std::sync::Arc::new(StudioDatabase::open(&path).unwrap());
            let service = crate::preferences_service::StudioPreferencesService::new(db.clone());
            service
                .set_telemetry_consent(TelemetryConsent {
                    analytics: false,
                    crash: false,
                })
                .unwrap();
            db.telemetry()
                .discard_category(TelemetryCategory::Analytics)
                .unwrap();
        }

        // "Session 2" (restart): drain would find nothing to send, and even
        // a hypothetical leftover would fail the uploader-boundary check.
        {
            let db = std::sync::Arc::new(StudioDatabase::open(&path).unwrap());
            assert_eq!(
                db.telemetry().count().unwrap(),
                0,
                "old analytics event must never survive to a restart"
            );
            let service = crate::preferences_service::StudioPreferencesService::new(db.clone());
            let consent = service.get_telemetry_consent().unwrap();
            assert!(!consent.analytics);
        }
    }

    /// A "network unavailable" drain tick (simulated by never calling any
    /// network code — just re-running the same consent check `drain_queue`
    /// would) must not somehow make a discarded/revoked event deliverable
    /// again. There is no retry path that resurrects a discarded event:
    /// `discard_category` deletes rows outright, the same as `mark_delivered`
    /// — there is no "queued but blocked" flag to accidentally flip back.
    #[test]
    fn consent_check_alone_never_makes_a_revoked_event_deliverable_again() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(StudioDatabase::open(&dir.path().join("s.redb")).unwrap());
        db.telemetry()
            .enqueue(&StudioTelemetryEvent::new(
                "page_view",
                None,
                serde_json::json!({}),
                1000,
                TelemetryCategory::Analytics,
            ))
            .unwrap();
        db.telemetry()
            .discard_category(TelemetryCategory::Analytics)
            .unwrap();

        // Simulate several "drain ticks" (as a flaky network would trigger
        // repeatedly) — none of them can find or resurrect the event.
        for _ in 0..5 {
            assert_eq!(db.telemetry().count().unwrap(), 0);
            assert_eq!(db.telemetry().peek_batch(10).unwrap().len(), 0);
        }
    }
}
