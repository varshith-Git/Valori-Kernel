// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! S2a — one-time import of legacy Studio persistence into `studio.redb`.
//!
//! # Scope (S2a only)
//!
//! This module **reads** `preferences.json` (tauri-plugin-store's format)
//! and `events.jsonl` (`desktop/src-tauri/src/telemetry.rs`'s queue
//! format) and imports what it understands into this database. It never
//! writes to, renames, or deletes either legacy file — every function here
//! takes bytes or a path and only calls `std::fs::read`. Nothing in
//! `desktop/src-tauri` is wired to call this yet, and no existing runtime
//! consumer (the JS preference store, the telemetry sender, the updater)
//! is changed by this module's existence. That wiring — making the app
//! actually read/write through `StudioDatabase` going forward — is a
//! separate, later phase (S2b) by design; see
//! `docs/architecture/studio-storage.md` §"Migration strategy".
//!
//! # The five-step contract every migration function follows
//!
//! 1. **Detect** — read `meta`'s migration-completed flag. Already run?
//!    Return immediately (`MigrationReport::already_migrated`), touching
//!    nothing else. This is what makes calling these functions on every
//!    app startup safe and cheap.
//! 2. **Validate** — parse the legacy bytes fully *before* writing
//!    anything. For `events.jsonl` (one JSON value per line) this is
//!    per-line: a malformed line is recorded in
//!    [`MigrationReport::skipped`] and excluded, the rest of the file
//!    still imports. For `preferences.json` (one JSON object) a malformed
//!    file fails the whole call — there is no meaningful "half a JSON
//!    object" to partially accept.
//! 3. **Import transactionally** — every write this module makes (the
//!    imported data *and* the migration-completed flag) happens in **one**
//!    `redb` write transaction. If anything fails before `commit()`, the
//!    transaction is dropped and nothing is written — never "data
//!    imported but flag not set" (which would silently re-import next
//!    time) or the reverse (flag set but data missing).
//! 4. **Verify** — after commit, a fresh read transaction confirms the
//!    flag and a spot-check of the written data are actually there. Cheap
//!    insurance within one process, and literal compliance with "verify"
//!    as its own step rather than trusting the commit call's `Ok(())`
//!    alone.
//! 5. **Mark migration complete** — the flag written in step 3, read back
//!    in step 4. There is no separate "mark complete" call; it is part of
//!    the same atomic import.
//!
//! # Never migrates credentials
//!
//! `preferences.json`'s known shape has no credential-bearing field
//! today (`onboardingVersion`, `telemetryConsent`, `installationId`,
//! `lastPage`, `recentProjects`, `favoriteProjects`, `lastOpenedProject` —
//! see [`LegacyPreferences`]) and `LegacyPreferences` only deserializes
//! those named fields — an `apiKey` (or any other) key present in the file
//! is silently ignored by `serde`'s default "unknown fields are dropped"
//! behavior, not copied through. The actual `apiKey` exposure documented
//! in `docs/architecture/studio-storage-audit.md` §11 lives in `ui/`'s
//! `localStorage` (`useEmbeddingConfig`/`useLLMConfig`), a different store
//! entirely, and is explicitly **out of scope** for this migration — S2a
//! does not read `localStorage` in any form. The eventual home for
//! provider credentials is the OS keychain, referenced from a
//! `credential_ref`-shaped field, never a value in this database — see
//! `docs/architecture/studio-storage.md` §"Security".
//!
//! # Project identity: why `recentProjects`/`favoriteProjects` do NOT
//! land in the `projects` table
//!
//! `preferences.json` has only ever tracked projects **by name**
//! (`ui/src/lib/native.ts`'s `getRecentProjects()`/`getFavoriteProjects()`
//! return `string[]`) — it has no `ProjectId`. `crate::project::ProjectRegistry`
//! is keyed by `ProjectId` on purpose (see that module's docs on identity
//! discipline). Minting a fresh `ProjectId` for each legacy name here would
//! create an id the daemon's own `project.json` does not know about —
//! exactly the kind of duplicate identity `docs/architecture/ownership.md`
//! exists to prevent. Instead, the raw names are preserved losslessly in
//! [`LegacyProjectNames`] (`meta.legacy_project_names`) — inert, read-only
//! residue — for a later phase to resolve against the daemon's real
//! project list (by name) and register proper `ProjectId`-keyed entries.
//! [`legacy_project_names`] is how that later phase reads it back.

use std::path::Path;

use redb::{Database, ReadableTable};
use serde::{Deserialize, Serialize};
use valori_domain::{InstallationId, SessionId};

use crate::error::{StudioStorageError, StudioStorageResult};
use crate::preferences::{StudioPreferences, TelemetryConsent};
use crate::schema::{
    self, KEY_LEGACY_PREFERENCES_MIGRATED_AT, KEY_LEGACY_PROJECT_NAMES,
    KEY_LEGACY_TELEMETRY_MIGRATED_AT, META, PREFERENCES, SINGLETON_KEY, TELEMETRY_QUEUE,
};
use crate::telemetry::{StudioTelemetryEvent, MAX_QUEUE_LEN};

// ── Legacy source shapes ─────────────────────────────────────────────────
//
// Deliberately narrow mirrors of the real on-disk formats, not the real
// types (`ui/`'s TS types, `telemetry.rs`'s `TelemetryEnvelope`) — this
// crate cannot depend on either. Every field is `#[serde(default)]` so an
// unknown/absent field never fails the whole parse, and any field *not*
// listed here (an `apiKey`, or anything this crate doesn't know about) is
// silently dropped by serde rather than copied through — see module docs.

#[derive(Debug, Default, Deserialize)]
struct LegacyPreferences {
    #[serde(default, rename = "onboardingVersion")]
    onboarding_version: Option<u32>,
    #[serde(default, rename = "telemetryConsent")]
    telemetry_consent: Option<LegacyTelemetryConsent>,
    #[serde(default, rename = "lastPage")]
    last_page: Option<String>,
    #[serde(default, rename = "installationId")]
    installation_id: Option<String>,
    #[serde(default, rename = "recentProjects")]
    recent_projects: Vec<String>,
    #[serde(default, rename = "favoriteProjects")]
    favorite_projects: Vec<String>,
    #[serde(default, rename = "lastOpenedProject")]
    last_opened_project: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LegacyTelemetryConsent {
    #[serde(default)]
    analytics: bool,
    #[serde(default)]
    crash: bool,
}

/// Mirrors `desktop/src-tauri/src/telemetry.rs`'s `TelemetryEnvelope`.
/// `schema`/`source`/`version`/`platform`/`arch` are read (for validation —
/// a line that doesn't even parse as this shape is not a telemetry event)
/// but not persisted onto `StudioTelemetryEvent`, which does not model
/// them; only `event_id`, `timestamp`, `session_id`, `event`, `properties`
/// carry through.
#[derive(Debug, Deserialize)]
struct LegacyTelemetryEnvelope {
    event_id: String,
    timestamp: String,
    #[serde(default)]
    session_id: Option<String>,
    event: String,
    #[serde(default)]
    properties: serde_json::Value,
}

// ── Public result types ──────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct SkippedRecord {
    /// The legacy record's own identifier (an event id, a line number) —
    /// whatever is stable enough to look up in the original file.
    pub identifier: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MigrationReport {
    /// `true` if this exact migration had already run (`meta` flag
    /// present) — every other field is meaningless/zeroed in that case,
    /// since nothing was touched on this call.
    pub already_migrated: bool,
    /// `false` if the legacy source file did not exist at all — a normal,
    /// expected case (a fresh install, or one that never had this file),
    /// not an error. When `false`, nothing is imported and the
    /// migration-completed flag is **not** set — a legacy file that shows
    /// up later (unlikely, but not this crate's business to assume
    /// impossible) still gets picked up on a later call.
    pub source_found: bool,
    /// Records successfully written.
    pub imported: usize,
    /// Records present in the legacy source but not imported, with why.
    pub skipped: Vec<SkippedRecord>,
}

/// The name-only project bookkeeping carried over from `preferences.json`.
/// See module docs — never treated as `ProjectId`-keyed identity.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LegacyProjectNames {
    pub recent: Vec<String>,
    pub favorite: Vec<String>,
    pub last_opened: Option<String>,
}

// ── preferences.json → `preferences` table ───────────────────────────────

/// Reads `path` and calls [`migrate_legacy_preferences`]. A missing file
/// is reported via `MigrationReport::source_found == false`, not an error.
pub(crate) fn migrate_legacy_preferences_from_path(
    db: &Database,
    path: &Path,
    migrated_at: i64,
) -> StudioStorageResult<MigrationReport> {
    match std::fs::read(path) {
        Ok(bytes) => migrate_legacy_preferences(db, &bytes, migrated_at),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(MigrationReport {
            source_found: false,
            ..Default::default()
        }),
        Err(e) => Err(StudioStorageError::from(e)),
    }
}

/// Imports `preferences.json`'s bytes. Idempotent — a second call is a
/// no-op once the first has committed. See module docs for the five-step
/// contract and for why `recentProjects`/`favoriteProjects`/
/// `lastOpenedProject` land in `meta.legacy_project_names`, not `projects`.
///
/// Existing `preferences` rows (if any — e.g. from a build that already
/// writes through `StudioDatabase`) are **merged into**, not overwritten
/// by: only fields the legacy JSON actually had replace the stored value,
/// so this can never silently erase state this database already holds
/// that `preferences.json` doesn't know about.
pub(crate) fn migrate_legacy_preferences(
    db: &Database,
    json_bytes: &[u8],
    migrated_at: i64,
) -> StudioStorageResult<MigrationReport> {
    // ── 1. Detect ──
    if schema::get_json::<i64>(db, META, KEY_LEGACY_PREFERENCES_MIGRATED_AT)?.is_some() {
        return Ok(MigrationReport {
            already_migrated: true,
            source_found: true,
            ..Default::default()
        });
    }

    // ── 2. Validate ──
    let legacy: LegacyPreferences = serde_json::from_slice(json_bytes)?;

    let legacy_names = LegacyProjectNames {
        recent: legacy.recent_projects.clone(),
        favorite: legacy.favorite_projects.clone(),
        last_opened: legacy.last_opened_project.clone(),
    };
    let mut skipped = Vec::new();
    let installation_id = match legacy.installation_id.as_deref() {
        Some(raw) => match raw.parse::<InstallationId>() {
            Ok(id) => Some(id),
            Err(e) => {
                skipped.push(SkippedRecord {
                    identifier: "installationId".to_string(),
                    reason: format!("not a valid installation id: {e}"),
                });
                None
            }
        },
        None => None,
    };

    // ── 3. Import transactionally ──
    let tx = db.begin_write()?;
    {
        let mut prefs_table = tx.open_table(PREFERENCES)?;
        let mut prefs: StudioPreferences = match prefs_table.get(SINGLETON_KEY)? {
            Some(v) => serde_json::from_slice(v.value())?,
            None => StudioPreferences::default(),
        };
        if let Some(v) = legacy.onboarding_version {
            prefs.onboarding_version = Some(v);
        }
        if let Some(c) = &legacy.telemetry_consent {
            prefs.telemetry_consent = Some(TelemetryConsent {
                analytics: c.analytics,
                crash: c.crash,
            });
        }
        if let Some(p) = &legacy.last_page {
            prefs.last_page = Some(p.clone());
        }
        if let Some(id) = installation_id {
            prefs.installation_id = Some(id);
        }
        let bytes = serde_json::to_vec(&prefs)?;
        prefs_table.insert(SINGLETON_KEY, bytes.as_slice())?;
    }
    {
        let mut meta = tx.open_table(META)?;
        let names_bytes = serde_json::to_vec(&legacy_names)?;
        meta.insert(KEY_LEGACY_PROJECT_NAMES, names_bytes.as_slice())?;
        let flag_bytes = serde_json::to_vec(&migrated_at)?;
        meta.insert(KEY_LEGACY_PREFERENCES_MIGRATED_AT, flag_bytes.as_slice())?;
    }
    tx.commit()?;

    // ── 4. Verify ──
    let verify_tx = db.begin_read()?;
    let meta = verify_tx.open_table(META)?;
    if meta.get(KEY_LEGACY_PREFERENCES_MIGRATED_AT)?.is_none() {
        return Err(StudioStorageError::MigrationFailed {
            from: 0,
            to: 0,
            reason: "preferences migration committed but the completed-flag did not read back"
                .to_string(),
        });
    }
    drop(meta);
    drop(verify_tx);

    // ── 5. Mark migration complete — done as part of step 3's commit ──
    Ok(MigrationReport {
        already_migrated: false,
        source_found: true,
        imported: 1,
        skipped,
    })
}

// ── events.jsonl → `telemetry_queue` table ───────────────────────────────

pub(crate) fn migrate_legacy_telemetry_queue_from_path(
    db: &Database,
    path: &Path,
    migrated_at: i64,
) -> StudioStorageResult<MigrationReport> {
    match std::fs::read(path) {
        Ok(bytes) => migrate_legacy_telemetry_queue(db, &bytes, migrated_at),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(MigrationReport {
            source_found: false,
            ..Default::default()
        }),
        Err(e) => Err(StudioStorageError::from(e)),
    }
}

/// Imports `events.jsonl`'s lines. Idempotent — see
/// [`migrate_legacy_preferences`]'s doc for the shared contract.
///
/// Each line is validated independently: a malformed line, or one whose
/// `session_id` doesn't parse as a `SessionId`, is recorded in
/// [`MigrationReport::skipped`] with a reason and excluded — it does not
/// abort the rest of the file. If more than [`MAX_QUEUE_LEN`] valid events
/// are found, only the newest `MAX_QUEUE_LEN` (by `timestamp`) are
/// imported — the same oldest-evicted policy `TelemetryQueue::enqueue`
/// already enforces for live traffic; the rest are recorded as skipped
/// with reason `"queue capacity"`, not silently dropped.
pub(crate) fn migrate_legacy_telemetry_queue(
    db: &Database,
    jsonl_bytes: &[u8],
    migrated_at: i64,
) -> StudioStorageResult<MigrationReport> {
    // ── 1. Detect ──
    if schema::get_json::<i64>(db, META, KEY_LEGACY_TELEMETRY_MIGRATED_AT)?.is_some() {
        return Ok(MigrationReport {
            already_migrated: true,
            source_found: true,
            ..Default::default()
        });
    }

    // ── 2. Validate ──
    let text = String::from_utf8_lossy(jsonl_bytes);
    let mut valid: Vec<StudioTelemetryEvent> = Vec::new();
    let mut skipped = Vec::new();

    for (lineno, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let identifier = format!("line {}", lineno + 1);
        let envelope: LegacyTelemetryEnvelope = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(e) => {
                skipped.push(SkippedRecord {
                    identifier,
                    reason: format!("malformed JSON: {e}"),
                });
                continue;
            }
        };
        let created_at = match chrono::DateTime::parse_from_rfc3339(&envelope.timestamp) {
            Ok(dt) => dt.timestamp_millis(),
            Err(e) => {
                skipped.push(SkippedRecord {
                    identifier: envelope.event_id.clone(),
                    reason: format!("unparseable timestamp {:?}: {e}", envelope.timestamp),
                });
                continue;
            }
        };
        let session_id = match envelope.session_id.as_deref() {
            Some(raw) => match raw.parse::<SessionId>() {
                Ok(id) => Some(id),
                Err(e) => {
                    skipped.push(SkippedRecord {
                        identifier: envelope.event_id.clone(),
                        reason: format!("invalid session id {raw:?}: {e}"),
                    });
                    None
                }
            },
            None => None,
        };
        valid.push(StudioTelemetryEvent {
            event_id: envelope.event_id,
            created_at,
            event_name: envelope.event,
            session_id,
            payload: envelope.properties,
            attempt_count: 0,
            last_attempt_at: None,
            // Legacy `events.jsonl` rows were all gated by the old
            // analytics-only enqueue check regardless of event name — see
            // `TelemetryCategory::default()`'s doc comment for why
            // `Analytics` is the factually accurate category for them,
            // not just the safer one.
            category: crate::telemetry::TelemetryCategory::Analytics,
        });
    }

    // Bound to MAX_QUEUE_LEN, keeping the newest — same policy as live enqueue.
    valid.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then(a.event_id.cmp(&b.event_id))
    });
    if valid.len() > MAX_QUEUE_LEN {
        let drop_count = valid.len() - MAX_QUEUE_LEN;
        for dropped in valid.drain(0..drop_count) {
            skipped.push(SkippedRecord {
                identifier: dropped.event_id,
                reason: "queue capacity".to_string(),
            });
        }
    }

    // ── 3. Import transactionally ──
    let imported = valid.len();
    let tx = db.begin_write()?;
    {
        let mut table = tx.open_table(TELEMETRY_QUEUE)?;
        for event in &valid {
            let bytes = serde_json::to_vec(event)?;
            table.insert(event.event_id.as_str(), bytes.as_slice())?;
        }
    }
    {
        let mut meta = tx.open_table(META)?;
        let flag_bytes = serde_json::to_vec(&migrated_at)?;
        meta.insert(KEY_LEGACY_TELEMETRY_MIGRATED_AT, flag_bytes.as_slice())?;
    }
    tx.commit()?;

    // ── 4. Verify ──
    let verify_tx = db.begin_read()?;
    let meta = verify_tx.open_table(META)?;
    if meta.get(KEY_LEGACY_TELEMETRY_MIGRATED_AT)?.is_none() {
        return Err(StudioStorageError::MigrationFailed {
            from: 0,
            to: 0,
            reason: "telemetry migration committed but the completed-flag did not read back"
                .to_string(),
        });
    }
    drop(meta);
    drop(verify_tx);
    let count_tx = db.begin_read()?;
    let table = count_tx.open_table(TELEMETRY_QUEUE)?;
    use redb::ReadableTableMetadata;
    let actual_len = table.len()? as usize;
    if actual_len < imported {
        return Err(StudioStorageError::MigrationFailed {
            from: 0,
            to: 0,
            reason: format!(
                "telemetry migration committed {imported} events but only {actual_len} are readable back"
            ),
        });
    }

    // ── 5. Mark migration complete — done as part of step 3's commit ──
    Ok(MigrationReport {
        already_migrated: false,
        source_found: true,
        imported,
        skipped,
    })
}

// ── Reading back the legacy project-name residue ─────────────────────────

pub(crate) fn legacy_project_names(
    db: &Database,
) -> StudioStorageResult<Option<LegacyProjectNames>> {
    schema::get_json(db, META, KEY_LEGACY_PROJECT_NAMES)
}
