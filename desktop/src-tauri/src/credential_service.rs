// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Provider-credential storage — the OS credential store, never `studio.redb`,
//! never `localStorage`, never a Valori-built vault.
//!
//! # Architecture (Studio S3 — Credential Security)
//!
//! ```text
//! ui/ (React config hooks)
//!        │  provider + model + secret (only at save time)
//!        ▼
//! Tauri commands (`credential_store`, `credential_exists`, `credential_delete`,
//! `credential_get` — the last one only for the immediate provider HTTP call,
//! see this module's "Why `credential_get` exists" section below)
//!        │
//!        ▼
//! `CredentialService`
//!        │
//!        ▼
//! `keyring::Entry` → OS credential store
//!   (macOS Keychain / Windows Credential Manager / Linux Secret Service)
//! ```
//!
//! Persisted provider configuration (`ui/`'s `localStorage`, in-memory
//! config objects) becomes `{ provider, model, credentialRef }` — never
//! `{ provider, model, apiKey }`, on the desktop path. See
//! `docs/reviews/studio-credentials-audit.md` and
//! `docs/phases/phase-studio-S3-credentials.md`.
//!
//! # Why `credential_get` exists
//!
//! The actual provider HTTP call (OpenAI/Cohere/Groq/... embedding, LLM,
//! rerank requests) happens in `ui/src/lib/server/{embed,llm,reranker}.ts`
//! — TypeScript, not Rust. Moving that whole call stack into Rust just to
//! avoid ever handing the secret back to JavaScript would be a large,
//! unrelated rewrite the audit explicitly said not to do (§11). The
//! smallest safe architecture keeps provider execution in JS and adds
//! exactly one narrow bridge: resolve a `CredentialRef` to its secret,
//! called only at the moment of an actual provider request, never cached
//! in persisted state. This is a deliberate, documented exception to
//! "avoid broad APIs" (§7) — it is one narrowly-scoped operation the UI
//! genuinely needs, not a `get_all_credentials()`-style broad API.
//!
//! # Keychain naming (invariant: `CredentialRef` → exactly one entry)
//!
//! - `service` = `"Valori"` (this module's `KEYCHAIN_SERVICE` constant) —
//!   stable across every credential, every provider, every project.
//! - `account` = the `CredentialRef`'s UUID string form — opaque, contains
//!   no provider name, no email, no organization id, no project name.
//!
//! One `CredentialRef` names exactly one `keyring::Entry` (`service`,
//! `account`) pair, and nothing else is ever encoded into that pair. This
//! keeps the keychain-visible metadata (e.g. what a user sees in macOS's
//! Keychain Access app) free of anything that could itself be sensitive —
//! only "Valori" and an opaque id are ever visible there.
//!
//! # Security rules this module follows
//!
//! - Never logs a secret. `tracing`/`debug!` calls in this module log only
//!   `CredentialRef`s and structural error info (see `CredentialError`'s
//!   `Display`, which never embeds a secret value — `keyring::Error`'s own
//!   `Display` impl doesn't either).
//! - Never serializes a secret — `CredentialService`'s methods return
//!   `String`/`Option<String>` in memory only, never a `Serialize` type
//!   that could accidentally round-trip through `studio.redb` or an
//!   `invoke()` payload logged elsewhere.
//! - Never writes a secret into `studio.redb` — this module has no
//!   `StudioDatabase` dependency at all, by construction.

use std::fmt;

use keyring::Entry;
use valori_domain::CredentialRef;

/// Keychain "service" name — stable across every credential this app ever
/// stores. See the module doc's "Keychain naming" section.
const KEYCHAIN_SERVICE: &str = "Valori";

/// Typed errors from `CredentialService`. Deliberately small and
/// UI-actionable — see `CredentialError::user_message()`, which is what the
/// Tauri commands surface to JavaScript instead of raw `keyring` internals
/// (task requirement: "Do not expose raw OS keychain internals as the
/// primary user message").
#[derive(Debug)]
pub enum CredentialError {
    /// The OS credential store itself could not be reached (locked,
    /// daemon not running, platform failure, etc).
    Unavailable(String),
    /// The OS denied access to the credential store (permission prompt
    /// declined, sandboxing, access-control policy).
    PermissionDenied(String),
    /// No credential exists for this `CredentialRef` (never set, or
    /// already deleted — including "deleted externally", e.g. the user
    /// removed it via the OS's own Keychain Access / Credential Manager
    /// UI outside of Valori).
    NotFound,
    /// The stored value could not be interpreted as a valid secret
    /// (e.g. not valid UTF-8) or the request was otherwise malformed.
    Invalid(String),
    /// Any other keychain failure, preserved as text for diagnostics —
    /// never contains a secret value (see the module doc's security rules).
    Other(String),
}

impl CredentialError {
    /// A short, non-technical message safe to show a user — never raw OS
    /// keychain internals. Mirrors the task's example message.
    pub fn user_message(&self) -> &'static str {
        match self {
            CredentialError::Unavailable(_) => {
                "The system credential store is unavailable. Please try again."
            }
            CredentialError::PermissionDenied(_) => {
                "Access to the system credential store was denied."
            }
            CredentialError::NotFound => {
                "Provider credential unavailable. Please reconnect your provider."
            }
            CredentialError::Invalid(_) => {
                "Provider credential is invalid. Please reconnect your provider."
            }
            CredentialError::Other(_) => {
                "Provider credential unavailable. Please reconnect your provider."
            }
        }
    }
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialError::Unavailable(detail) => {
                write!(f, "credential store unavailable: {detail}")
            }
            CredentialError::PermissionDenied(detail) => {
                write!(f, "credential store access denied: {detail}")
            }
            CredentialError::NotFound => write!(f, "credential not found"),
            CredentialError::Invalid(detail) => write!(f, "invalid credential: {detail}"),
            CredentialError::Other(detail) => write!(f, "credential store error: {detail}"),
        }
    }
}

impl std::error::Error for CredentialError {}

/// Maps `keyring::Error` (platform-specific, `#[non_exhaustive]`) to our
/// small, UI-actionable error set. Never includes the secret value in any
/// branch — `keyring::Error`'s own `Display` impl doesn't embed the
/// password/secret either (confirmed against `keyring` 3.6.3's source),
/// only structural/platform diagnostic text.
fn map_keyring_error(err: keyring::Error) -> CredentialError {
    match err {
        keyring::Error::NoEntry => CredentialError::NotFound,
        keyring::Error::NoStorageAccess(inner) => {
            CredentialError::PermissionDenied(inner.to_string())
        }
        keyring::Error::PlatformFailure(inner) => CredentialError::Unavailable(inner.to_string()),
        keyring::Error::BadEncoding(_) => {
            CredentialError::Invalid("stored value was not valid UTF-8".to_string())
        }
        other => CredentialError::Other(other.to_string()),
    }
}

/// Typed wrapper around the OS credential store (via `keyring`). The rest
/// of the application depends on this service, never on `keyring` directly
/// — see the module doc.
#[derive(Clone, Copy, Default)]
pub struct CredentialService;

impl CredentialService {
    pub fn new() -> Self {
        Self
    }

    fn entry(&self, cred_ref: &CredentialRef) -> Result<Entry, CredentialError> {
        Entry::new(KEYCHAIN_SERVICE, &cred_ref.to_string()).map_err(map_keyring_error)
    }

    /// Mints a fresh `CredentialRef` and stores `secret` under it. This is
    /// the entry point for "user just typed a new API key" — the caller
    /// has no ref yet, so one is minted here.
    pub fn store_new(&self, secret: &str) -> Result<CredentialRef, CredentialError> {
        let cred_ref = CredentialRef::new();
        self.store(&cred_ref, secret)?;
        Ok(cred_ref)
    }

    /// Stores `secret` under an existing `CredentialRef`. Used by
    /// migration (§15 of the S3 task — store under a ref it already knows
    /// it minted) and available for a future "rotate this credential"
    /// UX, though nothing currently calls it with a pre-existing ref other
    /// than migration's own retry path.
    pub fn store(&self, cred_ref: &CredentialRef, secret: &str) -> Result<(), CredentialError> {
        self.entry(cred_ref)?
            .set_password(secret)
            .map_err(map_keyring_error)
    }

    /// Retrieves the secret for `cred_ref`. `Ok(None)` means "no such
    /// credential" (handled gracefully, not an error) — matches
    /// `exists()`'s semantics and lets callers distinguish "not found"
    /// from a genuine store failure without matching on error variants.
    pub fn get(&self, cred_ref: &CredentialRef) -> Result<Option<String>, CredentialError> {
        match self.entry(cred_ref)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_error(e)),
        }
    }

    /// Whether a credential currently exists for `cred_ref`, without
    /// returning the secret.
    pub fn exists(&self, cred_ref: &CredentialRef) -> Result<bool, CredentialError> {
        Ok(self.get(cred_ref)?.is_some())
    }

    /// Deletes the credential for `cred_ref`. Idempotent: deleting an
    /// already-absent credential is treated as success, not an error — the
    /// end state ("no credential for this ref") is what the caller wants,
    /// and it's already true.
    pub fn delete(&self, cred_ref: &CredentialRef) -> Result<(), CredentialError> {
        match self.entry(cred_ref)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_error(e)),
        }
    }
}

// ── Tauri Commands ───────────────────────────────────────────────────────────
//
// Every command maps `CredentialError` to `CredentialError::user_message()`
// for the JS-visible error string — never the raw `Display` (which, while
// still secret-free, can contain platform-internal detail not meant for
// end users). Detailed diagnostics stay server-side (available via
// `err.to_string()` in a future logging hook if ever needed) — not
// currently logged anywhere, per "never log secrets" extending naturally
// to "don't need to log credential errors either to satisfy this phase."

use std::str::FromStr;

/// Stores `secret` and returns the `CredentialRef` string form it's stored
/// under. The UI persists the returned string as `credentialRef` — never
/// the secret itself.
///
/// `existing_credential_ref` lets a caller overwrite an already-minted
/// reference instead of always minting a new one — critical for a
/// password-input `onChange` handler that fires on every keystroke: without
/// this, editing a 40-character key would mint 40 keychain entries, 39 of
/// them immediately orphaned. Pass it once a ref exists for the field being
/// edited; omit it only for the very first save of a brand-new credential.
#[tauri::command]
pub fn credential_store(
    secret: String,
    existing_credential_ref: Option<String>,
) -> Result<String, String> {
    let service = CredentialService::new();
    match existing_credential_ref
        .as_deref()
        .map(CredentialRef::from_str)
    {
        Some(Ok(cred_ref)) => service
            .store(&cred_ref, &secret)
            .map(|_| cred_ref.to_string())
            .map_err(|e| e.user_message().to_string()),
        Some(Err(_)) => Err(
            CredentialError::Invalid("malformed credential reference".to_string())
                .user_message()
                .to_string(),
        ),
        None => service
            .store_new(&secret)
            .map(|r| r.to_string())
            .map_err(|e| e.user_message().to_string()),
    }
}

/// Resolves a `CredentialRef` to its secret. Called only immediately
/// before an actual provider HTTP request — see the module doc's "Why
/// `credential_get` exists". Returns `Ok(None)` (not an error) when the
/// credential doesn't exist, so the UI can show "reconnect your provider"
/// instead of a generic failure.
#[tauri::command]
pub fn credential_get(credential_ref: String) -> Result<Option<String>, String> {
    let cred_ref = CredentialRef::from_str(&credential_ref)
        .map_err(|_| CredentialError::Invalid("malformed credential reference".to_string()))
        .map_err(|e: CredentialError| e.user_message().to_string())?;
    CredentialService::new()
        .get(&cred_ref)
        .map_err(|e| e.user_message().to_string())
}

/// Whether a credential currently exists — used by the UI to show
/// connected/disconnected provider state without resolving the secret.
#[tauri::command]
pub fn credential_exists(credential_ref: String) -> Result<bool, String> {
    let cred_ref = CredentialRef::from_str(&credential_ref)
        .map_err(|_| CredentialError::Invalid("malformed credential reference".to_string()))
        .map_err(|e: CredentialError| e.user_message().to_string())?;
    CredentialService::new()
        .exists(&cred_ref)
        .map_err(|e| e.user_message().to_string())
}

/// Deletes a credential. Idempotent — see `CredentialService::delete`.
#[tauri::command]
pub fn credential_delete(credential_ref: String) -> Result<(), String> {
    let cred_ref = CredentialRef::from_str(&credential_ref)
        .map_err(|_| CredentialError::Invalid("malformed credential reference".to_string()))
        .map_err(|e: CredentialError| e.user_message().to_string())?;
    CredentialService::new()
        .delete(&cred_ref)
        .map_err(|e| e.user_message().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recognizable fake secret for tests — never a real credential
    /// shape, never printed unnecessarily (task §27's "safety" rule).
    const FAKE_SECRET: &str = "valori-test-secret-DO-NOT-USE";

    /// Every keychain test in this module talks to the *real* OS
    /// credential store (there is no in-memory `keyring` backend) — CI
    /// environments without a usable store (e.g. a headless Linux runner
    /// with no D-Bus Secret Service) will see these as `Unavailable`/
    /// `PermissionDenied`, not a panic. Each test cleans up after itself
    /// and tolerates a store that refuses access entirely, matching this
    /// module's own "handle keychain failures gracefully" requirement —
    /// these tests exercise that requirement, they don't assume it away.
    fn skip_if_keychain_unavailable<T>(result: Result<T, CredentialError>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(CredentialError::Unavailable(_)) | Err(CredentialError::PermissionDenied(_)) => {
                eprintln!("skipping: OS credential store unavailable in this environment");
                None
            }
            Err(e) => panic!("unexpected credential error: {e}"),
        }
    }

    #[test]
    fn store_then_get_round_trips_and_cleans_up() {
        let svc = CredentialService::new();
        let Some(cred_ref) = skip_if_keychain_unavailable(svc.store_new(FAKE_SECRET)) else {
            return;
        };
        let got = skip_if_keychain_unavailable(svc.get(&cred_ref));
        assert_eq!(got, Some(Some(FAKE_SECRET.to_string())));
        let _ = svc.delete(&cred_ref);
    }

    #[test]
    fn exists_reflects_store_and_delete() {
        let svc = CredentialService::new();
        let Some(cred_ref) = skip_if_keychain_unavailable(svc.store_new(FAKE_SECRET)) else {
            return;
        };
        assert_eq!(
            skip_if_keychain_unavailable(svc.exists(&cred_ref)),
            Some(true)
        );
        assert!(svc.delete(&cred_ref).is_ok());
        assert_eq!(
            skip_if_keychain_unavailable(svc.exists(&cred_ref)),
            Some(false)
        );
    }

    #[test]
    fn missing_credential_is_none_not_an_error() {
        let svc = CredentialService::new();
        let never_stored = CredentialRef::new();
        let got = skip_if_keychain_unavailable(svc.get(&never_stored));
        assert_eq!(got, Some(None));
    }

    #[test]
    fn delete_is_idempotent() {
        let svc = CredentialService::new();
        let Some(cred_ref) = skip_if_keychain_unavailable(svc.store_new(FAKE_SECRET)) else {
            return;
        };
        assert!(svc.delete(&cred_ref).is_ok());
        // Second delete of an already-gone credential must not error.
        assert!(svc.delete(&cred_ref).is_ok());
    }

    #[test]
    fn different_refs_never_collide() {
        let svc = CredentialService::new();
        let Some(a) = skip_if_keychain_unavailable(svc.store_new("secret-a-fake")) else {
            return;
        };
        let Some(b) = skip_if_keychain_unavailable(svc.store_new("secret-b-fake")) else {
            let _ = svc.delete(&a);
            return;
        };
        assert_ne!(a, b);
        assert_eq!(
            svc.get(&a).ok().flatten(),
            Some("secret-a-fake".to_string())
        );
        assert_eq!(
            svc.get(&b).ok().flatten(),
            Some("secret-b-fake".to_string())
        );
        let _ = svc.delete(&a);
        let _ = svc.delete(&b);
    }

    #[test]
    fn store_under_an_existing_ref_overwrites_rather_than_duplicating() {
        // Exercises the migration retry path: storing again under the same
        // CredentialRef (e.g. a resumed migration) must not create a second
        // entry — `keyring`/OS credential stores overwrite by (service,
        // account), which is exactly (KEYCHAIN_SERVICE, cred_ref).
        let svc = CredentialService::new();
        let cred_ref = CredentialRef::new();
        let Some(()) = skip_if_keychain_unavailable(svc.store(&cred_ref, "first-fake")) else {
            return;
        };
        assert!(svc.store(&cred_ref, "second-fake").is_ok());
        assert_eq!(
            svc.get(&cred_ref).ok().flatten(),
            Some("second-fake".to_string())
        );
        let _ = svc.delete(&cred_ref);
    }

    #[test]
    fn user_message_never_contains_the_raw_secret_or_os_internals_marker() {
        // A cheap structural guard: every user_message() is a fixed,
        // hardcoded string, so this is really pinning that fact — if a
        // future edit accidentally interpolates `self` into user_message,
        // this test's exact-match assertions start failing.
        assert_eq!(
            CredentialError::NotFound.user_message(),
            "Provider credential unavailable. Please reconnect your provider."
        );
        assert_eq!(
            CredentialError::Unavailable("platform detail".into()).user_message(),
            "The system credential store is unavailable. Please try again."
        );
        assert_eq!(
            CredentialError::PermissionDenied("platform detail".into()).user_message(),
            "Access to the system credential store was denied."
        );
    }

    // ── §19/§27: end-to-end security-boundary tests with a real credential ──

    /// A "provider connected" telemetry event carrying only the §19-safe
    /// fields (provider, model, credential_ref) must never contain the
    /// secret — even though `payload` is structurally freeform JSON (the
    /// audit's flagged risk), this proves the *recommended safe shape*
    /// genuinely excludes it.
    #[test]
    fn telemetry_event_with_safe_provider_metadata_never_contains_the_secret() {
        use valori_studio_storage::telemetry::{StudioTelemetryEvent, TelemetryCategory};

        let cred_ref = CredentialRef::new();
        let event = StudioTelemetryEvent::new(
            "provider_connected",
            None,
            serde_json::json!({
                "provider": "openai",
                "model": "gpt-4o-mini",
                "credential_ref": cred_ref.to_string(),
            }),
            0,
            TelemetryCategory::Analytics,
        );
        let serialized = serde_json::to_string(&event).unwrap();
        assert!(!serialized.contains(FAKE_SECRET));
    }

    /// Stores `FAKE_SECRET` through the real `CredentialService` (real OS
    /// keychain — skips gracefully if unavailable, same rationale as every
    /// other test in this module), then asserts the exact string never
    /// occurs in a serialized `StudioPreferences` record fetched through
    /// the sibling `preferences_service` module (same crate, so this
    /// integration is possible from an inline test even though the module
    /// itself is private to external crates).
    #[test]
    fn fake_secret_never_occurs_in_a_serialized_studio_preferences_record() {
        let svc = CredentialService::new();
        let Some(cred_ref) = skip_if_keychain_unavailable(svc.store_new(FAKE_SECRET)) else {
            return;
        };

        let temp = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(
            valori_studio_storage::StudioDatabase::open(&temp.path().join("studio.redb")).unwrap(),
        );
        let prefs_service = crate::preferences_service::StudioPreferencesService::new(db);
        let prefs = prefs_service.get_all().unwrap();
        assert!(!serde_json::to_string(&prefs).unwrap().contains(FAKE_SECRET));

        // The ref itself (safe, opaque) must not equal or embed the secret.
        assert!(!cred_ref.to_string().contains(FAKE_SECRET));

        let _ = svc.delete(&cred_ref);
    }
}
