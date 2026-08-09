// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Studio-owned application preferences.
//!
//! # Not arbitrary JSON storage
//!
//! `StudioPreferences` is an explicit, named struct — every field this
//! store can hold is declared here, not accepted as an arbitrary
//! `(key, JSON value)` pair. This is deliberately narrower than
//! `tauri-plugin-store`'s `preferences.json`, which today accepts any key
//! any caller writes. Adding a new preference means adding a field here,
//! in a reviewed change — the same discipline `valori-metadata::Project`
//! already applies to control-plane state.
//!
//! # Not migrated from `preferences.json` in S1
//!
//! This store does not read or write `preferences.json`,
//! `tauri-plugin-store`, or `localStorage`. It is a new, independent store
//! that S2 may point the existing preference read/write call sites at —
//! see `docs/architecture/studio-storage.md` §"Backward compatibility".
//!
//! # Authoritative
//!
//! Every field here is a genuine user choice or Studio-local fact with no
//! other source of truth (see `crate` root docs' authoritative/cache
//! table). `delete()` clears the record back to defaults — it does not,
//! and must not, reach into any other table.

use redb::Database;
use serde::{Deserialize, Serialize};
use valori_domain::InstallationId;

use crate::error::StudioStorageResult;
use crate::schema::{self, PREFERENCES, SINGLETON_KEY};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TelemetryConsent {
    #[serde(default)]
    pub analytics: bool,
    #[serde(default)]
    pub crash: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub maximized: bool,
}

/// Studio's own preference record. Every field is `Option`/defaulted so
/// that a record written by an older build (missing a field this build
/// added) still deserializes — see `crate::schema` module docs on JSON
/// forward-compatibility.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct StudioPreferences {
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub accent_color: Option<String>,
    /// Mirrors `ui/src/lib/native.ts`'s `ONBOARDING_VERSION` gate — not
    /// migrated from it in S1, but the same "monotonic version, never
    /// silently rewound" semantics apply once it is.
    #[serde(default)]
    pub onboarding_version: Option<u32>,
    #[serde(default)]
    pub telemetry_consent: Option<TelemetryConsent>,
    #[serde(default)]
    pub window_state: Option<WindowState>,
    #[serde(default)]
    pub last_page: Option<String>,
    /// A random id generated once per install, persisted across restarts —
    /// mirrors `ui/src/lib/native.ts`'s `getInstallationId()`, which today
    /// lazily writes this into the same `preferences.json` key
    /// (`installationId`). A genuine singleton fact, unlike the
    /// name-only legacy project lists — see `crate::migration` module docs
    /// for why those live separately, not here.
    #[serde(default)]
    pub installation_id: Option<InstallationId>,
    /// The workspace folder chosen in onboarding/Settings, passed through
    /// as `VALORI_HOME` so it actually controls where projects/collections/
    /// snapshots live. Mirrors `ui/src/lib/native.ts`'s legacy
    /// `preferences.json` key `workspaceDir`.
    #[serde(default)]
    pub workspace_dir: Option<String>,
    /// The model folder chosen in onboarding/Settings. Mirrors the legacy
    /// `preferences.json` key `modelDir`.
    #[serde(default)]
    pub model_dir: Option<String>,
    /// Whether the app should show a persistent dock/taskbar icon. Mirrors
    /// the legacy `preferences.json` key `dockIcon`.
    #[serde(default)]
    pub dock_icon: Option<bool>,
    /// Whether the user has accepted the terms during onboarding. Mirrors
    /// the legacy `preferences.json` key `termsAccepted`.
    #[serde(default)]
    pub terms_accepted: Option<bool>,
    /// Per-notification-type on/off flags (`{"desktop": true, ...}`).
    /// `serde_json::Value`, not a typed struct — deliberately: this bag's
    /// keys are UI-defined notification types that can grow without a
    /// schema change, unlike `telemetry_consent`/`window_state`, which are
    /// stable, well-defined shapes. S7 (`docs/phases/phase-studio-S7-persistence-boundary.md`)
    /// — migrated off `localStorage["valori:notifs"]` on desktop; the web
    /// build keeps using `localStorage` (no `studio.redb` there).
    #[serde(default)]
    pub notification_prefs: Option<serde_json::Value>,
}

pub struct PreferencesStore<'a> {
    db: &'a Database,
}

impl<'a> PreferencesStore<'a> {
    pub(crate) fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// Never errors on a missing record — returns [`StudioPreferences::default`]
    /// if nothing has been written yet (fresh database, or after [`Self::delete`]).
    pub fn get(&self) -> StudioStorageResult<StudioPreferences> {
        Ok(schema::get_json(self.db, PREFERENCES, SINGLETON_KEY)?.unwrap_or_default())
    }

    /// Full replace.
    pub fn set(&self, prefs: &StudioPreferences) -> StudioStorageResult<()> {
        schema::put_json(self.db, PREFERENCES, SINGLETON_KEY, prefs)
    }

    /// Atomic read-modify-write, e.g. `prefs.update(|p| p.theme = Some("dark".into()))`.
    /// Starts from [`StudioPreferences::default`] if nothing is stored yet.
    pub fn update(
        &self,
        f: impl FnOnce(&mut StudioPreferences),
    ) -> StudioStorageResult<StudioPreferences> {
        schema::update_json(
            self.db,
            PREFERENCES,
            SINGLETON_KEY,
            StudioPreferences::default,
            f,
        )
    }

    /// Clears the stored record. A subsequent [`Self::get`] returns
    /// defaults, exactly as if nothing had ever been written. Returns
    /// `true` if a record existed to delete.
    pub fn delete(&self) -> StudioStorageResult<bool> {
        schema::delete_key(self.db, PREFERENCES, SINGLETON_KEY)
    }
}
