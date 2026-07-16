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

// ── Persisted preferences (desktop only) ────────────────────────────────────
// A single JSON file under the app's config dir, via tauri-plugin-store.
// Nothing here is used outside the desktop shell — the browser dev path
// (`npm run dev` in ui/ standalone) has no equivalent persistence and doesn't
// need one; it's a development convenience, not a shipped surface.

let storePromise: Promise<import("@tauri-apps/plugin-store").LazyStore> | null = null;

async function getStore() {
  if (!isTauri()) return null;
  if (!storePromise) {
    storePromise = import("@tauri-apps/plugin-store").then(
      ({ LazyStore }) => new LazyStore("preferences.json"),
    );
  }
  return storePromise;
}

export async function getPreference<T>(key: string): Promise<T | null> {
  const store = await getStore();
  if (!store) return null;
  const value = await store.get<T>(key);
  return value ?? null;
}

export async function setPreference<T>(key: string, value: T): Promise<void> {
  const store = await getStore();
  if (!store) return;
  await store.set(key, value);
  await store.save();
}

// ── Onboarding (versioned) ───────────────────────────────────────────────────
// Bump ONBOARDING_VERSION whenever the Welcome flow gains a step a returning
// user genuinely needs to see (e.g. a new required folder choice) — anyone
// on an older completed version, or with no record at all (including a
// stale/foreign preferences file from an earlier prototype), sees onboarding
// again instead of it silently getting skipped. Someone on a *newer* version
// than this build expects (downgrade case) is left alone — don't nag them
// backwards.

export const ONBOARDING_VERSION = 1;

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
// preferences.json store as everything else above — this isn't a separate
// file, just a documented slice of it.
//
// NOTE on `lastWorkspace`: there's no workspace-switcher in the UI today
// (workspaces exist on the daemon side — `crates/valori-daemon/src/workspace.rs`
// — but nothing in `ui/` lets a user pick one), so it's deliberately omitted
// here. Add it once that control exists; a field with nothing to write to it
// would just be dead state.

const MAX_RECENT_PROJECTS = 8;

export async function getRecentProjects(): Promise<string[]> {
  return (await getPreference<string[]>("recentProjects")) ?? [];
}

/** Call when a project is opened — moves it to the front, dedupes, caps at
 *  MAX_RECENT_PROJECTS. Also records it as `lastOpenedProject`. */
export async function touchRecentProject(name: string): Promise<void> {
  const current = await getRecentProjects();
  const next = [name, ...current.filter((n) => n !== name)].slice(0, MAX_RECENT_PROJECTS);
  await setPreference("recentProjects", next);
  await setPreference("lastOpenedProject", name);
}

export async function getLastOpenedProject(): Promise<string | null> {
  return getPreference<string>("lastOpenedProject");
}

export async function getFavoriteProjects(): Promise<string[]> {
  return (await getPreference<string[]>("favoriteProjects")) ?? [];
}

export async function toggleFavoriteProject(name: string): Promise<string[]> {
  const current = await getFavoriteProjects();
  const next = current.includes(name) ? current.filter((n) => n !== name) : [...current, name];
  await setPreference("favoriteProjects", next);
  return next;
}

/** A project was deleted — drop it from both lists so they don't accumulate
 *  references to things that no longer exist. */
export async function forgetProject(name: string): Promise<void> {
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

/** No-op (returns not-running) outside the desktop shell. */
export async function startDaemon(home?: string | null): Promise<DaemonStatus> {
  if (!isTauri()) return { running: false, healthy: false, bind: null };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DaemonStatus>("start_daemon", { home: home ?? null });
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
