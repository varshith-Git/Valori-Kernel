// Thin bridge to native desktop capabilities (folder picking, reveal-in-Finder,
// persisted preferences) that only exist when `ui/` is running inside the
// Tauri desktop shell. Every export degrades gracefully when running in a
// plain browser tab (`npm run dev` in `ui/` directly) — callers never need
// their own `isTauri()` branch.

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Open a native "choose a folder" dialog. Returns `null` in a plain browser
 *  tab (no such capability exists there) or if the user cancels. */
export async function pickFolder(title?: string): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({ directory: true, multiple: false, title });
  return typeof result === "string" ? result : null;
}

/** Reveal a path in Finder/Explorer, or open it if it's a file. No-op (but
 *  doesn't throw) outside the desktop shell. */
export async function revealPath(path: string): Promise<void> {
  if (!isTauri()) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(path);
}

export function nativeAvailable(): boolean {
  return isTauri();
}

/** Opens Valori Cloud's login page in the system browser and starts the
 *  "sign in to sync" handoff — see src-tauri/src/lib.rs's open_cloud_login
 *  and the valori://auth-callback deep link it eventually receives back.
 *  No-op outside the desktop shell (the website is always cloud mode
 *  already, it has no "sync" concept to switch into). */
export async function openCloudLogin(provider?: "google" | "github"): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("open_cloud_login", { provider });
}

// ── Persisted preferences (desktop only) ────────────────────────────────────
// Typed preferences stored in studio.redb via StudioPreferencesService (S2b-2a).
// All reads and writes go to studio.redb's preferences table; legacy preferences.json
// is never written to. A fallback in-memory map is provided for browser dev mode.

const devMemoryPreferences: Record<string, unknown> = {};

export async function getPreference<T>(key: string): Promise<T | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    const val = await invoke<T | null>("get_preference", { key });
    return val ?? null;
  }
  return (devMemoryPreferences[key] as T) ?? null;
}

export async function setPreference<T>(key: string, value: T): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("set_preference", { key, value });
    return;
  }
  devMemoryPreferences[key] = value;
}

// ── Onboarding (versioned) ───────────────────────────────────────────────────
// Bump ONBOARDING_VERSION whenever the Welcome flow gains a step a returning
// user genuinely needs to see (e.g. a new required folder choice) — anyone
// on an older completed version, or with no record at all (including a
// stale/foreign preferences file from an earlier prototype), sees onboarding
// again instead of it silently getting skipped. Someone on a *newer* version
// than this build expects (downgrade case) is left alone — don't nag them
// backwards.

// Bump this whenever the onboarding flow gains a step that every existing
// user must see — v2 adds the mandatory sign-in step, v3 adds the
// telemetry consent step (Phase 1 desktop telemetry).
export const ONBOARDING_VERSION = 3;

export async function isOnboardingComplete(): Promise<boolean> {
  const completed = await getPreference<number>("onboardingVersion");
  return completed != null && completed >= ONBOARDING_VERSION;
}

export async function markOnboardingComplete(): Promise<void> {
  await setPreference("onboardingVersion", ONBOARDING_VERSION);
}

/** Developer/support escape hatch — see Settings → Developer. */
export async function resetOnboarding(): Promise<void> {
  await setPreference("onboardingVersion", 0);
}

// ── App memory (desktop only) ────────────────────────────────────────────────
// The small "remember where I was" state that makes reopening the app feel
// continuous instead of resetting to a blank slate every launch. Same
// studio.redb preferences table as everything else above — this isn't a
// separate store, just a documented slice of it.
//
// NOTE on `lastWorkspace`: there's no workspace-switcher in the UI today
// (workspaces exist on the daemon side — `crates/valori-daemon/src/workspace.rs`
// — but nothing in `ui/` lets a user pick one), so it's deliberately omitted
// here. Add it once that control exists; a field with nothing to write to it
// would just be dead state.

// ── Project Registry (S2b-2b: studio.redb projects table) ─────────────────────

export interface ProjectRegistryDto {
  id: string;
  displayName: string;
  kind:
    | { kind: "local"; path: string }
    | {
        kind: "cloud";
        organization_id?: string;
        cloud_endpoint: string;
        region?: string;
      };
  favorite: boolean;
  lastOpenedAt?: number;
  registeredAt: number;
  available: boolean;
}

export async function registryListProjects(): Promise<ProjectRegistryDto[]> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto[]>("registry_list_projects");
  }
  return [];
}

export async function registryGetProject(id: string): Promise<ProjectRegistryDto | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto | null>("registry_get_project", { id });
  }
  return null;
}

export async function registryRecentProjects(limit = 8): Promise<ProjectRegistryDto[]> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto[]>("registry_recent_projects", { limit });
  }
  return [];
}

export async function registryFavoriteProjects(): Promise<ProjectRegistryDto[]> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto[]>("registry_favorite_projects");
  }
  return [];
}

export async function registryRegisterLocalProject(
  id: string,
  name: string,
  path: string
): Promise<ProjectRegistryDto | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto>("registry_register_local_project", { id, name, path });
  }
  return null;
}

export async function registryRegisterCloudProject(
  id: string,
  name: string,
  organizationId?: string,
  endpoint = "https://api.valori.systems",
  region?: string
): Promise<ProjectRegistryDto | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto>("registry_register_cloud_project", {
      id,
      name,
      organizationId: organizationId ?? null,
      endpoint,
      region: region ?? null,
    });
  }
  return null;
}

export async function registryRenameProject(id: string, newName: string): Promise<ProjectRegistryDto | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto>("registry_rename_project", { id, newName });
  }
  return null;
}

export async function registrySetLocalPath(id: string, newPath: string): Promise<ProjectRegistryDto | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto>("registry_set_local_path", { id, newPath });
  }
  return null;
}

export async function registrySetFavorite(id: string, favorite: boolean): Promise<ProjectRegistryDto | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto>("registry_set_favorite", { id, favorite });
  }
  return null;
}

export async function registryTouchLastOpened(id: string): Promise<ProjectRegistryDto | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<ProjectRegistryDto>("registry_touch_last_opened", { id });
  }
  return null;
}

export async function registryUnregisterProject(id: string): Promise<boolean> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<boolean>("registry_unregister_project", { id });
  }
  return false;
}

// ── Legacy-compatible convenience helpers ────────────────────────────────────

export async function getRecentProjects(): Promise<string[]> {
  if (isTauri()) {
    const recents = await registryRecentProjects();
    if (recents.length > 0) {
      return recents.map((p) => p.displayName);
    }
  }
  return (await getPreference<string[]>("recentProjects")) ?? [];
}

/** Call when a project is opened — updates last_opened in studio.redb. */
export async function touchRecentProject(name: string): Promise<void> {
  if (isTauri()) {
    await registryTouchLastOpened(name).catch(() => {});
  }
  const current = await getRecentProjects();
  const next = [name, ...current.filter((n) => n !== name)].slice(0, 8);
  await setPreference("recentProjects", next);
  await setPreference("lastOpenedProject", name);
}

export async function getLastOpenedProject(): Promise<string | null> {
  if (isTauri()) {
    const recents = await registryRecentProjects(1);
    if (recents.length > 0) {
      return recents[0].displayName;
    }
  }
  return getPreference<string>("lastOpenedProject");
}

export async function getFavoriteProjects(): Promise<string[]> {
  if (isTauri()) {
    const favs = await registryFavoriteProjects();
    if (favs.length > 0) {
      return favs.map((p) => p.displayName);
    }
  }
  return (await getPreference<string[]>("favoriteProjects")) ?? [];
}

export async function toggleFavoriteProject(name: string): Promise<string[]> {
  if (isTauri()) {
    const currentFavs = await registryFavoriteProjects();
    const isFav = currentFavs.some((p) => p.displayName === name || p.id === name);
    await registrySetFavorite(name, !isFav).catch(() => {});
    const updated = await registryFavoriteProjects();
    return updated.map((p) => p.displayName);
  }
  const current = await getFavoriteProjects();
  const next = current.includes(name) ? current.filter((n) => n !== name) : [...current, name];
  await setPreference("favoriteProjects", next);
  return next;
}

/** A project was deleted — unregister from registry. */
export async function forgetProject(name: string): Promise<void> {
  if (isTauri()) {
    await registryUnregisterProject(name).catch(() => {});
  }
  const [recent, favorites, lastOpened] = await Promise.all([
    getRecentProjects(),
    getFavoriteProjects(),
    getLastOpenedProject(),
  ]);
  await setPreference("recentProjects", recent.filter((n) => n !== name));
  await setPreference("favoriteProjects", favorites.filter((n) => n !== name));
  if (lastOpened === name) await setPreference("lastOpenedProject", null);
}

export async function getLastPage(): Promise<string | null> {
  return getPreference<string>("lastPage");
}

export async function setLastPage(path: string): Promise<void> {
  await setPreference("lastPage", path);
}

// ── Daemon lifecycle (desktop only) ─────────────────────────────────────────
// The desktop app supervises `valori-daemon` directly (see
// `desktop/src-tauri/src/daemon_manager.rs`) rather than requiring it to be
// started by hand in a separate terminal. `home` is the workspace folder the
// user picked in onboarding/Settings — passed through as `VALORI_HOME` so the
// folder choice actually controls where projects/collections/snapshots live.

export interface DaemonStatus {
  running: boolean;
  healthy: boolean;
  bind: string | null;
}

/** No-op (returns not-running) outside the desktop shell.
 *  `modelDir`, if given, relocates model artifact storage independent of
 *  `home` — the real effect of the `modelDir` preference (S7). */
export async function startDaemon(home?: string | null, modelDir?: string | null): Promise<DaemonStatus> {
  if (!isTauri()) return { running: false, healthy: false, bind: null };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DaemonStatus>("start_daemon", { home: home ?? null, modelDir: modelDir ?? null });
}

/** No-op outside the desktop shell. */
export async function stopDaemon(): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("stop_daemon");
}

/** Returns not-running outside the desktop shell (there's nothing to supervise). */
export async function daemonStatus(): Promise<DaemonStatus> {
  if (!isTauri()) return { running: false, healthy: false, bind: null };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DaemonStatus>("daemon_status");
}

// ── Auto-updater (desktop only) ──────────────────────────────────────────────
// The Rust side emits `update-available` on startup if a new version is found.
// `installUpdate` downloads and applies it, then restarts the app.

export async function installUpdate(): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("install_update");
}

// ── Telemetry consent (desktop only) ────────────────────────────────────────
// Phase 1 desktop telemetry — see rfcs/desktop-telemetry (plan doc). Two
// toggles actually gate behavior; `diagnostics` (manual log upload only) is
// a future feature and stays a no-op for now. Both default to `false` —
// nothing is ever sent before a user has explicitly opted in, whether via
// the onboarding consent step or Settings → Privacy.
//
// Persisted through the same preferences.json store as everything else
// above, NOT localStorage — `SettingsModal.tsx`'s PrivacySection used to
// read/write a raw `localStorage.getItem("valori:privacy")` key that
// nothing else ever consumed; that's fixed to go through here instead.

export interface TelemetryConsent {
  analytics: boolean;
  crash: boolean;
}

const DEFAULT_TELEMETRY_CONSENT: TelemetryConsent = { analytics: false, crash: false };

export async function getTelemetryConsent(): Promise<TelemetryConsent> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<TelemetryConsent>("get_telemetry_consent_command");
  }
  const stored = await getPreference<TelemetryConsent>("telemetryConsent");
  return stored ?? DEFAULT_TELEMETRY_CONSENT;
}

export async function setTelemetryConsent(consent: TelemetryConsent): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("set_telemetry_consent_command", { consent });
    return;
  }
  await setPreference("telemetryConsent", consent);
}

/** A permanent id generated once per install, never tied to a Valori Cloud
 *  account or any other identity. Returns the exact same value forever
 *  across launches.
 *
 *  Desktop (Tauri): the canonical value lives in `studio.redb`'s
 *  `preferences.installation_id`, guaranteed to exist by an unconditional
 *  get-or-init call in `lib.rs`'s `setup()` — independent of telemetry
 *  consent (Studio Installation Identity phase). This branch is a thin
 *  read through `get_installation_id_command` →
 *  `StudioPreferencesService::get_or_init_installation_id`; it does not
 *  generate or persist anything itself, and `studio.redb` is the only
 *  desktop persistence location for this value.
 *
 *  Browser (Valori Cloud web build, not Tauri): there is no `studio.redb`
 *  to read from, so this falls back to a browser-local preference
 *  (`localStorage`-backed via `getPreference`/`setPreference`). This is a
 *  genuinely separate identity for the web build only — it must never be
 *  treated as, or fall back to, the desktop source of truth. */
export async function getInstallationId(): Promise<string> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("get_installation_id_command");
  }
  const existing = await getPreference<string>("installationId");
  if (existing) return existing;
  const fresh = crypto.randomUUID();
  await setPreference("installationId", fresh);
  return fresh;
}

// ── Provider credentials (Studio S3 — OS keychain, never localStorage) ────────
//
// Desktop (Tauri): the actual provider secret (OpenAI/Cohere/Groq/... API
// key) lives only in the OS credential store (macOS Keychain / Windows
// Credential Manager / Linux Secret Service), via `CredentialService` in
// `desktop/src-tauri/src/credential_service.rs`. What gets persisted
// anywhere JS can reach — `localStorage`, `studio.redb` if it were ever
// used for this — is only the opaque `credentialRef` string these
// functions return, never the secret. See
// `docs/reviews/studio-credentials-audit.md` and
// `docs/phases/phase-studio-S3-credentials.md`.
//
// Browser (Valori Cloud web, not Tauri): there is no OS keychain to call.
// These functions are desktop-only by construction (`credentialStore`/
// `credentialGet` throw outside Tauri) — the web build's provider config
// hooks keep storing the API key directly in `localStorage`, exactly as
// before this phase. That is a real, documented limitation of the web
// build, not something this phase claims to fix — see the phase doc's
// "Desktop vs Web" section.

/** Stores `secret` and returns the credential reference it's stored under.
 *  Desktop only — throws if called outside Tauri.
 *
 *  Pass `existingCredentialRef` to overwrite an already-minted reference
 *  instead of minting a new one — required for a password input's
 *  `onChange` (fires per keystroke): without reusing the ref, editing a
 *  key would mint one orphaned keychain entry per character typed. Omit
 *  it only for the very first save of a brand-new credential. */
export async function credentialStore(secret: string, existingCredentialRef?: string): Promise<string> {
  if (!isTauri()) throw new Error("credentialStore is only available in the desktop app");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string>("credential_store", { secret, existingCredentialRef: existingCredentialRef ?? null });
}

/** Resolves a credential reference to its secret. Call this only
 *  immediately before an actual provider HTTP request — never cache the
 *  result in persisted state (`localStorage`, React state that outlives
 *  the request). Returns `null` if the credential doesn't exist (deleted,
 *  never stored, or keychain entry removed externally). Desktop only. */
export async function credentialGet(credentialRef: string): Promise<string | null> {
  if (!isTauri()) throw new Error("credentialGet is only available in the desktop app");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string | null>("credential_get", { credentialRef });
}

/** Whether a credential currently exists, without resolving the secret.
 *  Returns `false` outside Tauri (no keychain to check). */
export async function credentialExists(credentialRef: string): Promise<boolean> {
  if (!isTauri()) return false;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<boolean>("credential_exists", { credentialRef });
}

/** Deletes a credential. Idempotent. No-op outside Tauri. */
export async function credentialDelete(credentialRef: string): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("credential_delete", { credentialRef });
}

/**
 * One-time, idempotent, verify-before-delete migration of a legacy
 * plaintext `apiKey` field (in a `localStorage` JSON blob at `storageKey`)
 * to a keychain-backed `credentialRef`. No-op outside Tauri — the web
 * build has no keychain to migrate to and keeps `apiKey` as-is.
 *
 * Ordering, and why (S3 phase — never delete before verify):
 *   1. If `credentialRef` is already present and `apiKey` is not — a
 *      previous run completed. No-op (idempotent).
 *   2. If `credentialRef` is already present AND `apiKey` is *still*
 *      present — a previous run stored the secret and recorded the ref,
 *      but was interrupted before verifying/cleaning up. Resume from
 *      verify, do not store again (avoids creating a duplicate credential).
 *   3. Otherwise, if a legacy `apiKey` is present with no ref yet: store
 *      it, and immediately persist the new `credentialRef` **alongside**
 *      the still-present `apiKey` (a deliberate, resumable intermediate
 *      state — see case 2).
 *   4. Verify: read the just-stored credential back and compare to the
 *      legacy value.
 *   5. Only on a verified byte-for-byte match, rewrite `localStorage`
 *      removing `apiKey`.
 *   6. On any failure (keychain unavailable, permission denied, verify
 *      mismatch, corrupt JSON, no legacy value): leave `localStorage`
 *      untouched beyond what step 3 already committed. Never delete the
 *      legacy secret on failure — retry on the next call (e.g. next
 *      launch), fail closed.
 *
 * Neither the legacy value nor the migrated secret is ever logged or
 * printed by this function.
 */
export async function migrateLegacyProviderCredential(storageKey: string): Promise<void> {
  if (!isTauri()) return;

  let raw: string | null;
  try {
    raw = localStorage.getItem(storageKey);
  } catch {
    return;
  }
  if (!raw) return;

  let parsed: Record<string, unknown>;
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== "object" || value === null || Array.isArray(value)) return;
    parsed = value as Record<string, unknown>;
  } catch {
    // Corrupt JSON — fail closed, leave untouched.
    return;
  }

  const legacyKey = typeof parsed.apiKey === "string" ? parsed.apiKey : "";
  const existingRef = typeof parsed.credentialRef === "string" ? parsed.credentialRef : "";

  if (existingRef) {
    if (!legacyKey) return; // case 1: already fully migrated
    await verifyAndCleanUpMigration(storageKey, parsed, existingRef, legacyKey); // case 2
    return;
  }

  if (!legacyKey) return; // nothing to migrate

  let ref: string;
  try {
    ref = await credentialStore(legacyKey);
  } catch {
    return; // keychain unavailable/permission denied/etc — retry next call
  }

  // Case 3: persist the ref immediately, apiKey still present as the
  // resumable safety net described above.
  const withRef = { ...parsed, credentialRef: ref };
  try {
    localStorage.setItem(storageKey, JSON.stringify(withRef));
  } catch {
    return;
  }

  await verifyAndCleanUpMigration(storageKey, withRef, ref, legacyKey);
}

async function verifyAndCleanUpMigration(
  storageKey: string,
  parsed: Record<string, unknown>,
  credentialRef: string,
  legacyKey: string,
): Promise<void> {
  let verified: string | null;
  try {
    verified = await credentialGet(credentialRef);
  } catch {
    return; // leave apiKey + credentialRef both present, retry next call
  }
  if (verified !== legacyKey) {
    // Verification failed — never delete the legacy secret.
    return;
  }
  const { apiKey: _drop, ...rest } = parsed;
  try {
    localStorage.setItem(storageKey, JSON.stringify(rest));
  } catch {
    // Leave as-is; harmless to retry (credentialRef already verifies).
  }
}

// ── Application Session Store (S2b-2c: studio.redb sessions table) ────────────

export interface StudioSessionDto {
  id: string;
  installationId?: string;
  appVersion: string;
  platform: string;
  startedAt: number;
  endedAt?: number;
  crashed: boolean;
  durationSecs?: number;
}

export async function getSessionId(): Promise<string | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<string>("get_session_id");
  }
  return null;
}

export async function getCurrentSession(): Promise<StudioSessionDto | null> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<StudioSessionDto | null>("session_get_current");
  }
  return null;
}

export async function getRecentSessions(limit = 10): Promise<StudioSessionDto[]> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<StudioSessionDto[]>("session_list_recent", { limit });
  }
  return [];
}

// ── Studio database recovery status ──────────────────────────────────────
// Reported once per launch by the Rust side after opening (and, if
// necessary, recovering) studio.redb — see
// docs/architecture/studio-storage.md §"Recovery UI". `studio.redb` is
// Studio's own local metadata database, never the store for actual
// project data (vectors/WAL/snapshots/indexes) — recovering it can
// restore a backup or start fresh, but it never touches project files.

export type StudioRecoveryStatus =
  | { kind: "healthy" }
  | { kind: "restored_from_backup"; message: string; backupGeneration: number; preservedOriginalPath: string }
  | { kind: "fresh_database_created"; message: string; preservedOriginalPath: string | null }
  | { kind: "unavailable" };

/** Queries this launch's recovery outcome directly — use this over the
 *  `studio-recovery` event when mounting after startup, since the event
 *  fires once, synchronously, before any window is guaranteed to be
 *  listening yet. Returns `null` if Studio storage never finished
 *  initializing (shouldn't normally happen — recovery is designed to
 *  always resolve to *some* status, even `"unavailable"`). */
export async function getStudioRecoveryStatus(): Promise<StudioRecoveryStatus | null> {
  if (!isTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<StudioRecoveryStatus | null>("get_studio_recovery_status");
}

