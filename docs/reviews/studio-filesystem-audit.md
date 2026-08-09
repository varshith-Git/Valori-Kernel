# Valori Studio — S6 Desktop Filesystem Audit

**Status:** Read-only investigation, performed before any S6 implementation.
**Scope:** `desktop/src-tauri/**`, `ui/src/**`, `crates/valori-daemon/**`,
`crates/valori-models/**`, `crates/valori-studio-storage/**`.

---

## 1. Every production filesystem operation, by owner

### Studio (`desktop/src-tauri` + `valori-studio-storage`)

| Path | Resolved by | Operation | Classification |
|---|---|---|---|
| `$VALORI_HOME/studio.redb` | `valori_studio_storage::path::default_db_path()` | open/create (redb) | **Canonical, Studio-owned** |
| `$VALORI_HOME/backups/studio.redb.*` | `path::default_backups_dir()` | create_dir_all, write, rename (rolling, `BACKUP_GENERATIONS = 3`) | **Canonical, Studio-owned** |
| `$VALORI_HOME/studio-recovery.jsonl` | `path::default_recovery_log_path()` | append-only write | **Canonical, Studio-owned** — deliberately a *sibling* of `studio.redb`, not inside a `recovery/` subdirectory (see §4) |
| Tauri `app_config_dir()/crashes/crash_marker.json` | `telemetry.rs::marker_path()` | write (panic hook), read+delete (next launch) | **Legacy location, Studio-owned** — not under `$VALORI_HOME` at all (see §4) |
| Tauri `app_config_dir()/preferences.json` | `studio_storage.rs::resolve_legacy_paths()` | **read-only**, S2a migration source | **Legacy, read-only, never written/deleted** |
| Tauri `app_config_dir()/events.jsonl` | same | **read-only**, S2a migration source | **Legacy, read-only, never written/deleted** |
| stdout/stderr | `tracing_subscriber::fmt()` | write | **Not a file at all** — confirmed no file sink exists anywhere in `desktop/src-tauri` |

### Daemon (`valori-daemon`, spawned as a child process by Studio)

| Path | Resolved by | Operation | Classification |
|---|---|---|---|
| `$VALORI_HOME/projects/<name>/project.json` | `JsonProjectStore` (`project.rs`) | read/write/rename (project registry + manifest) | **Canonical, Project-owned** |
| `$VALORI_HOME/projects/<name>/events.log`, `snapshot.val` | `ProjectManifest::event_log_path/snapshot_path` | owned by `valori-node`/`valori-kernel`/`valori-storage` at runtime, not the daemon itself | **Canonical, Project-owned** — the daemon only computes the path, never opens or writes these files |
| Cluster mode: `events-nN.log`, `current-nN.snap`, `raft-nN.redb`, `node-nN.log` | same | same | **Canonical, Project-owned / Node-owned** |
| `$VALORI_HOME/metadata.redb` | `valori_metadata::MetadataDb::open()`'s doc comment | **never actually called in production** | **Unknown/dormant** — confirmed in the S4 audit, reconfirmed here: zero production call sites |

### Models (`valori-models`, used by `valori-node`, not by `desktop/src-tauri` directly)

| Path | Resolved by | Operation | Classification |
|---|---|---|---|
| `<home>/models/<sanitized-id>/` | `ModelStore::new(home, ...)`, `home.as_ref().join("models")` | create_dir_all, stream-download, SHA-256 verify, install | **Canonical, Node-owned** (not Studio-owned — no `desktop/src-tauri` code references `valori-models` at all; wired into `valori-node`'s `server.rs`/`cluster_server.rs`/`ingest.rs`) |
| In-process `DownloadState` (partial-file progress) | `downloader/mod.rs` | in-memory only, no file beyond the partial artifact itself | **Temporary/ephemeral** |

### UI (`ui/src`)

Two genuinely different layers exist under `ui/src`, and they must not be
conflated (this distinction was under-stated on first pass and corrected
here):

- **Browser/React layer** (`"use client"` components, hooks). No
  filesystem access of any kind — confirmed by search. `pickFolder`/
  `revealPath` (`native.ts`) are thin Tauri command wrappers
  (`@tauri-apps/plugin-dialog`, `@tauri-apps/plugin-opener`); the UI never
  constructs a filesystem path string itself, it receives one back from
  the OS's native folder picker and hands it, opaque, to
  `setPreference("workspaceDir", ...)`/`startDaemon(workspaceDir)`.
- **`ui/src/lib/server/*.ts` — the Next.js server process** (what
  `docs/architecture/control-plane.md` calls the `ui-server`, a Node.js
  process, not the browser sandbox). This layer **does** legitimately
  touch the filesystem directly: `api-client.ts`, `connection.ts`, and
  `cluster-config.ts` each independently resolve `process.env.VALORI_HOME
  || path.join(os.homedir(), ".valori")` — a **third**, TypeScript-side
  duplicate of the same resolution rule `valori-daemon::default_home()`
  and `valori-studio-storage::path::default_home_dir()` already implement
  in Rust. `project-adapter.ts`'s `FALLBACK_PROJECTS_DIR` hardcodes
  `~/.valori/projects` without a `VALORI_HOME` check — **verified
  intentional, not a bug**: its own comment states the daemon's real
  `VALORI_HOME` (queried live via `resolveProjectsDir()`) is the source of
  truth, and this fallback is used *only* when the daemon is unreachable
  (e.g. still starting up) — a documented degraded-mode fallback, not an
  active parallel root. `projects.ts`'s own `VALORI_HOME` constant backs
  only its own `@deprecated` functions (per the file's own header
  comment) plus a handful of still-load-bearing pure path helpers used by
  the cluster (replication===3) lifecycle routes only.

This is a real, if low-risk, architectural fact: **the canonical root
resolution rule now exists independently in three places** (two Rust
crates, one TypeScript module cluster) instead of two. All three implement
the identical rule (`$VALORI_HOME` else `~/.valori`) and none was found to
have drifted from it. Consolidating the TypeScript side onto a single
shared resolver is a reasonable future cleanup, not undertaken in S6 —
see the phase doc's follow-ups (touching `ui/src/lib/server/*.ts` this
deeply, across three files with different call patterns, is new
TypeScript-side refactoring, not filesystem-*management* consolidation).

### Cloud (`ui/src/app/cloud/**`)

No local filesystem access at all — confirmed by search. Cloud is Supabase-backed exclusively (see the S4 audit's §8/§9).

---

## 2. Literal path/string search results

| Literal | Found in | Meaning |
|---|---|---|
| `~/.valori` | Doc comments only (`path.rs`, `lib.rs`, `CLAUDE.md`) — never a runtime string literal; every real resolution goes through `default_home_dir()`/`default_home()` | Documentation convention, not a code path |
| `.valori` | `path.rs`/`lib.rs`'s `.join(".valori")` | The literal directory name appended to `$HOME` |
| `studio.redb` | `path.rs::STUDIO_DB_FILENAME` | Canonical constant, single definition |
| `metadata.redb` | `valori-metadata`'s own doc comment | Never opened in production (§1) |
| `preferences.json` | `studio_storage.rs` (legacy, read-only) | S2a migration source |
| `events.jsonl` | `studio_storage.rs` (legacy, read-only) | S2a migration source |
| `logs` | **Not found as a real directory anywhere** — no code creates or writes to `$VALORI_HOME/logs/` | Aspirational only (matches the S4 audit's finding) |
| `cache` | **Not found as a real Studio directory** — `ui/`'s in-browser `localStorage` caches (`valori:projects-list`, `valori:tree:*`) are a different, browser-side concept, not `$VALORI_HOME/cache/` | Aspirational only |
| `downloads` | **Not found as a real directory** — model downloads land directly inside `models/<id>/`, no separate staging directory | Aspirational only |
| `models` | `valori-models::lib.rs` | Real, active, Node-owned |
| `projects` | `valori-daemon::project.rs` | Real, active, Project-owned |
| `temp` | **Not found as a Studio-owned directory** — the only production `std::env::temp_dir()` call in `desktop/src-tauri` is inside a test | Aspirational only |
| `crashes` | `telemetry.rs::CRASHES_DIR` constant, but rooted at Tauri's `app_config_dir()`, **not** `$VALORI_HOME` | Real, active, but at a **legacy, non-canonical root** (§4) |
| `backups` | `path.rs::default_backups_dir()` | Real, active, Studio-owned |

**Conclusion**: five of the eleven directories in the task's own target diagram (`logs/`, `cache/`, `downloads/`, `temp/`, `recovery/`) do not exist as real, distinct directories in the codebase today. `crashes/` exists but at the wrong root. This matches and extends the S4 audit's finding that "`~/.valori/cache/` and `~/.valori/downloads/` don't exist as real directories despite appearing in architecture diagrams."

---

## 3. Classification of every path

| Path | Classification |
|---|---|
| `$VALORI_HOME/studio.redb` | Canonical, Studio-owned |
| `$VALORI_HOME/backups/` | Canonical, Studio-owned |
| `$VALORI_HOME/studio-recovery.jsonl` | Canonical, Studio-owned (non-standard location by design — sibling of `studio.redb`, not inside a `recovery/` subdir) |
| `$VALORI_HOME/projects/<name>/**` | Canonical, Project-owned |
| `$VALORI_HOME/metadata.redb` | Unknown / dormant (schema-complete, never opened) |
| `<workspace-or-home>/models/<id>/` | Canonical, Node-owned (not Studio-owned) |
| Tauri `app_config_dir()/preferences.json` | Legacy, read-only |
| Tauri `app_config_dir()/events.jsonl` | Legacy, read-only |
| Tauri `app_config_dir()/crashes/crash_marker.json` | Legacy location, Studio-owned content |
| `$VALORI_HOME/logs/` | Does not exist (Unknown / aspirational) |
| `$VALORI_HOME/cache/` | Does not exist (Unknown / aspirational) |
| `$VALORI_HOME/downloads/` | Does not exist (Unknown / aspirational) |
| `$VALORI_HOME/temp/` | Does not exist (Unknown / aspirational) |
| `$VALORI_HOME/recovery/` | Does not exist (Unknown / aspirational — the real recovery log is a sibling file, not this directory) |
| Cloud (Supabase) | Cloud-owned, no local filesystem footprint |

---

## 4. Two deliberate exceptions to the target diagram, and why

1. **`studio-recovery.jsonl` is a sibling of `studio.redb`, not inside `recovery/`.** The crate's own doc comment is explicit: *"a corruption event that destroys `studio.redb` must not also destroy the record that corruption happened."* Nesting it inside a subdirectory changes nothing about that guarantee, but moving it is a real behavioral change to an already-shipped recovery mechanism with its own extensive DR-phase test suite. **Decision for S6: keep it where it is; expose it via `StudioPaths` at its real location, not the target diagram's location.** Documented here rather than silently diverging from the task's diagram.

2. **The crash marker lives under Tauri's `app_config_dir()`, not `$VALORI_HOME/crashes/`.** `install_panic_hook`'s own doc comment states this must be *"safe to run from within a panicking thread"* — minimizing what a panic handler does (no env var lookups, no `VALORI_HOME` resolution, just `AppHandle::path()`) is a deliberate robustness property, and changing it risks the one code path in the entire application that must never itself fail. It's also a single, tiny, one-shot file — not a growth or consolidation risk. **Decision for S6: `StudioPaths::crashes_dir()` returns the canonical `$VALORI_HOME/crashes/` path for any *new* Studio-owned crash-adjacent files, but the existing panic-hook marker path is left exactly as is, documented as a permanent, deliberate exception** — moving it is explicitly listed as a follow-up, not silently done.

---

## 5. `workspace_dir` / `model_dir` — what they actually are

Confirmed by tracing the real call path, not assumed:

- `workspaceDir` (a `studio.redb` preference, `StudioPreferences.workspace_dir`) is **not** a display-only value. `startDaemon(workspaceDir)` → `daemon_manager.rs::start_daemon_internal` → if `Some`, sets `VALORI_HOME` in the **spawned daemon child process's** environment (`daemon_manager.rs`'s own doc comment: *"the real effect of the workspace folder the user picked"*).
- This means **Studio's own root and the daemon's project-data root are only the same directory by default** — when `workspaceDir` is unset, the daemon's child process inherits no `VALORI_HOME` override and falls back to its own default (`$VALORI_HOME` from its inherited environment, or `~/.valori`) — identical resolution logic to Studio's own, so they coincide. When a user sets a custom `workspaceDir`, the daemon's `projects/`/`models/` root diverges from wherever Studio's own `studio.redb` lives.
- `modelDir` is stored identically (a `StudioPreferences` field) but **no production code path was found consuming it** — it is not passed to `start_daemon`, not passed to `ModelStore::new`, and no Tauri command reads it for anything other than round-tripping it back to the Settings UI. It is a **user-facing preference with no wired effect today** — worth flagging, but out of S6's scope to wire up (that would be new behavior, not filesystem-management consolidation).

**Precedence, as it actually exists today** (not proposed — this is what the code already does):
1. `workspaceDir` preference, if set → becomes the daemon's `VALORI_HOME`.
2. Otherwise, the daemon's own environment `$VALORI_HOME`, if the desktop app happened to set one (it doesn't, today) or the OS environment has one.
3. Otherwise, `~/.valori` (daemon's own default).

Studio's own root (`studio.redb`, backups, recovery log, crash marker parent) follows a **separate, simpler** precedence: `$VALORI_HOME` (the Tauri process's own OS environment) → `~/.valori`. **`workspaceDir` never affects where `studio.redb` itself lives** — only where the daemon's projects/models resolve.

---

## 6. Legacy path disposition table

| Path | Read? | Write? | Migrate? | Deprecated? | Delete? |
|---|---|---|---|---|---|
| Tauri `app_config_dir()/preferences.json` | Yes (S2a, once, idempotent) | Never | Already migrated (S2a→S2b-1) | Yes | **Never** — explicit invariant, unchanged by S6 |
| Tauri `app_config_dir()/events.jsonl` | Yes (S2a, once, idempotent) | Never | Already migrated | Yes | **Never** |
| Tauri `app_config_dir()/crashes/crash_marker.json` | Yes (one-shot, self-deletes after read) | Yes (panic hook) | Not migrated — deliberate exception (§4) | No — still the live, active location | Yes, but only its own marker, one-shot, by existing design |
| `tauri-plugin-store` | No | No | Superseded (S2b-2a), dependency never removed | Yes (flagged in the S4 audit as P2 cleanup) | N/A — no files it ever wrote were found |

No new legacy paths were found beyond what S1–S4 already catalogued.

---

## 7. What this audit changes about the S6 implementation plan

- `StudioPaths` will expose accessors for the directories that are real and load-bearing today (`studio_db`, `backups_dir`, `recovery_log_path` at its real sibling location, `projects_dir`, `models_dir` for the *default* layout) **and** lazily-creatable new canonical directories the task's diagram calls for but don't exist yet (`logs_dir`, `crashes_dir` at the new canonical location, `cache_dir`, `downloads_dir`, `temp_dir`) — created only when first used, per the task's explicit "do not create directories merely because they appear in the diagram" instruction.
- `crashes_dir()` is intentionally **not** wired to replace the existing panic-hook marker path — see §4.
- `models_dir()`/`projects_dir()` on `StudioPaths` describe the **default** (no-override) layout; they are not authoritative when a user has set `workspaceDir` — see §5. This is documented on the accessors themselves, not hidden.
