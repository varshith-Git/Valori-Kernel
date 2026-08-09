# Valori Studio — Full Persistence Architecture Audit

**Status:** Read-only audit as originally produced (source code
unmodified for this document itself). Three of its findings have since
been fixed in follow-up phases — marked inline below with
**`[FIXED — S2c]`** / **`[FIXED — S3]`** and cross-referenced to
`docs/phases/phase-studio-S2c-privacy-boundary-cleanup.md` /
`docs/phases/phase-studio-S3-credentials.md`:
telemetry consent revocation now invalidates already-queued analytics
events at both the enqueue guard and the uploader boundary (§9/§10/§17
item 1), `theme.tsx`'s desktop dual-write to `localStorage` is
removed (§4/§17 item 1), and this document's own **highest-severity
unresolved finding** — plaintext provider API keys in `localStorage` — is
resolved on the desktop path: the actual secret now lives in the OS
credential store (`keyring`), and only an opaque `credentialRef` is
persisted in `localStorage` (see `docs/reviews/studio-credentials-audit.md`
for the full audit that preceded the fix). The web/Cloud build's
`localStorage` behavior is explicitly unchanged — see the S3 phase doc's
"Desktop vs Web" section. This document is otherwise left as originally
written — a point-in-time record — rather than rewritten; every other
finding, `UNKNOWN`, and recommendation still reflects the state of the
repository at the time of the original audit unless marked fixed.

Every claim below is backed by a specific file/line reference found
during the original audit pass; anywhere the repository didn't answer a
question, this document says `UNKNOWN — requires implementation/design
decision` rather than inferring an answer.

**Scope:** `desktop/src-tauri/**`, `ui/**`, `crates/valori-studio-storage/**`,
plus the crates it borders (`valori-domain`, `valori-daemon`,
`valori-models`, `valori-metadata`) to the extent needed to draw the
project-storage boundary correctly.

---

## 1. Executive Summary

Valori Studio's persistence is **not** one system — it is five, at
different levels of completeness:

1. **`studio.redb`** (`valori-studio-storage`) — the intended canonical
   store. 8 tables exist and are fully implemented and tested at the crate
   level. Of those, **5 have real production writers/readers**
   (`preferences`, `projects`, `sessions`, `telemetry_queue`, plus `meta`
   internally) and **3 are schema-complete but never called from
   production code** (`project_cache`, `sync_state`, `update_state`).
2. **Legacy Tauri files** (`preferences.json`, `events.jsonl`) — read-only
   migration sources now (verified: no production write call sites
   remain), except that `tauri-plugin-store` itself is still registered
   as a Tauri plugin with no code calling it.
3. **Raw browser `localStorage`** — still an **active, production, every-launch
   write path** in three places that were not part of any S1–DR migration
   scope: `theme.tsx` (dual-writes theme to both `studio.redb` *and*
   `localStorage` on every `setTheme` call), `useEmbeddingConfig.ts` /
   `useLLMConfig.ts` (provider config **including plaintext `apiKey`**),
   and `onboarding.ts` (a small getting-started checklist, browser-only,
   never intended for `studio.redb`).
4. **Cloud (Supabase)** — authoritative for Cloud projects, organizations,
   billing, API keys/service accounts; Studio holds no local copy of any
   Cloud credential.
5. **The project data layer** (`$VALORI_HOME/projects/<name>/`, owned by
   `valori-daemon` + `valori-kernel`/`valori-wire`/`valori-storage`) —
   structurally unreachable from `valori-studio-storage` (the dependency
   firewall makes this true by construction, verified against
   `dependency_direction.rs`), confirmed by grep to have zero code paths
   in the Studio storage crate.

**The single highest-priority finding:** provider API keys
(`useEmbeddingConfig.ts`, `useLLMConfig.ts`) are still stored in plaintext
`localStorage`, unchanged since before S1. This was flagged in the
original S1 audit and remains completely unresolved — no phase since has
touched it, and none was supposed to (each phase's stop condition
explicitly excluded credentials).

**Second finding — `[FIXED — S2c]`:** `theme.tsx` was apparently never
updated during S2b-2a's "preferences runtime migration" to stop writing
`localStorage` directly — it wrote to *both* stores on every call, which
was redundant but not unsafe (theme has no sensitivity). Fixed; see
`docs/phases/phase-studio-S2c-privacy-boundary-cleanup.md`.

---

## 2. Complete Storage Inventory

| Data | Current storage | Owner | Authoritative? | Rebuildable? | Sensitive? | Local/Cloud |
|---|---|---|---|---|---|---|
| Preferences (theme, language, accentColor, onboardingVersion, lastPage, windowState, workspaceDir, modelDir, dockIcon, termsAccepted) | `studio.redb` `preferences` table (`preferences_service.rs`) | Studio | Yes | Restore-from-backup or safe defaults | No | Local |
| Theme preference (duplicate path) | **Also** raw `localStorage` key `valori-theme` (`ui/src/lib/theme.tsx:57,89`) | Studio (legacy path, still active) | No — `studio.redb` wins when native | Yes (trivial) | No | Local |
| Installation identity | `studio.redb` `preferences.installation_id` (`preferences.rs:85`) | Studio | Yes | No — a fresh install mints a new one | No (random UUID, not tied to account) | Local |
| Session identity (current process) | In-memory `OnceLock<String>` (`telemetry.rs:102`), persisted per-session to `studio.redb` `sessions` table at start | Studio | Yes (once started) | N/A (ephemeral by nature) | No | Local |
| Session history (start/end/duration/crashed) | `studio.redb` `sessions` table (`session_service.rs`) | Studio | Yes | Disposable — losing it doesn't break anything | No | Local |
| Telemetry queue (undelivered events) | `studio.redb` `telemetry_queue` table (`telemetry.rs`'s `enqueue_to_db`/`drain_queue`) | Studio (emitter), Cloud telemetry ingest (sink) | No — a queue, not a record of truth | Disposable (best-effort) | Should not (no payload audited to contain secrets — see §9) | Local → Cloud |
| Telemetry consent (analytics, crash) | `studio.redb` `preferences.telemetry_consent` (`preferences_service.rs`) | Studio | Yes | No — must ask again if lost | No | Local |
| Project registry (local + cloud refs, favorite, last_opened) | `studio.redb` `projects` table (`project_registry_service.rs`) | Studio (reference layer only) | Yes for Studio's own bookkeeping; **no** for project meaning (daemon/Cloud own that) | Not auto-rebuilt (see §5) | No | Local (mirrors Cloud refs) |
| Recent projects | Derived: `ProjectRegistry::recent()` sorts `projects` table by `last_opened_at` (`project.rs:126`) — no separate list | Studio | Derived, not stored separately | Yes trivially (re-sort) | No | Local |
| Favorite projects | Field on `projects` rows (`StudioProjectRecord.favorite`) — no separate list | Studio | Yes | No — user's own choice, not derivable | No | Local |
| Collections metadata | **Not in `studio.redb` at all** — owned by `valori-metadata::MetadataDb` (`~/.valori/metadata.redb`, a separate file/crate) per `docs/architecture/studio-storage.md` §1 | `valori-metadata` | Yes | N/A | No | Local (node/daemon side) |
| Cloud project references | `studio.redb` `projects` table, `ProjectKind::Cloud { organization_id, cloud_endpoint, region }` (`project.rs:57-66`) | Studio (reference) | No — Cloud is authoritative | Yes (re-fetchable from Cloud) | No (opaque reference strings, not credentials) | Local cache of Cloud |
| Sync state | `studio.redb` `sync_state` table exists and is fully implemented (`sync.rs`) — **zero production call sites found** (`grep` confirms no `.sync()` call outside crate tests) | Studio (schema only; unused) | N/A — never written | N/A | No | PLANNED, not wired |
| Update state | `studio.redb` `update_state` table exists and is fully implemented (`update.rs`) — **zero production call sites**; the real updater (`tauri_plugin_updater`) manages its own state internally, never through `studio.redb` | Studio (schema only; unused) / `tauri-plugin-updater` (actual) | N/A | N/A | No | PLANNED, not wired |
| Model metadata / downloaded models / model cache | `~/.valori/models` — owned by `valori-models::PackageStore` (per-package `manifest.json` files), referenced by `valori-node`/`cluster_server.rs` (`dirs::home_dir().join(".valori").join("models")`) | `valori-models` | Yes | N/A | No | Local — **not** `studio.redb`, no Studio table for this |
| Search history | **Not found anywhere** — no code path stores past search queries in Studio, `localStorage`, or `studio.redb` | — | — | — | — | NOT DEFINED |
| UI state (last page) | `studio.redb` `preferences.last_page` | Studio | Yes | No | No | Local |
| Workspace state (window size/position) | `tauri-plugin-window-state` plugin (`lib.rs:394`, `.plugin(tauri_plugin_window_state::Builder::default().build())`) — its own internal store, **not** `studio.redb`'s `preferences.window_state` field (that field exists in the schema but has no writer found) | Tauri plugin (window-state), separately from Studio | Yes (plugin-owned) | Yes trivially | No | Local |
| Logs | **No file sink exists.** `tracing_subscriber::fmt()` (`lib.rs:368`) writes to stdout/stderr only — confirmed by audit in the DR phase, unchanged since | — | — | — | No | Local (console only, not persisted to `$VALORI_HOME/logs/`) |
| Crash markers | `$VALORI_HOME`-adjacent Tauri `app_config_dir()`-based `crashes/crash_marker.json` (`telemetry.rs:349`, `marker_path`) — **not** `studio.redb` | `telemetry.rs` (file-based, unchanged since before S1) | Yes (one-shot) | N/A | No — `CrashInfo` carries only a panic-message hash, location, previous session id, uptime (`telemetry.rs:333-343`), never the raw message or a stack trace | Local |
| Crash reports (sent) | Sent via the telemetry queue as an event (`enqueue_update_event`/similar), subject to the same consent gate | Studio → Cloud | N/A | N/A | See above — no raw message | Local → Cloud (with consent) |
| API keys (embedding/LLM/reranker providers) | **`[FIXED — S3]`** Desktop: opaque `credentialRef` in `localStorage`, actual secret in the OS credential store (`keyring`), keys unchanged (`valori:embedding_config` / `valori:llm_config` / `valori:reranker_config`). Web/Cloud: unchanged — still plaintext `apiKey` in `localStorage`, no keychain available there | `ui/` hooks (desktop) / unchanged (web) | Yes (OS keychain, desktop only) | Yes — re-resolves via `credentialRef` (desktop); no — re-enter (web) | Desktop: No, longer a plaintext leak. Web: **Yes — still unresolved**, documented limitation, not silently accepted | Local (desktop: OS keychain; web: browser) |
| Cloud tokens (Supabase access/refresh) | `localStorage` + mirrored cookie, via Supabase's own SSR client storage adapter (`ui/src/utils/supabase/client.ts`) | Supabase SDK | Yes | Re-authenticate | Yes, but standard practice for the library in use (per the S1 audit's classification — "safe" under "documented pattern for the library") | Local (session), Cloud (source) |
| Provider credentials (daemon-side reference) | `crates/valori-daemon/src/project.rs`'s `EmbeddingConfig.api_key_ref: Option<String>` — explicitly a **reference** (env var name / keychain id), never the raw secret (`project.rs:55-57`) | `valori-daemon` | Yes (for the reference) | N/A | No — by design, never holds the secret | Local |

---

## 3. `studio.redb` Table Audit

Source: `crates/valori-studio-storage/src/schema.rs:76-88` (`ALL_TABLES`),
cross-checked against every store module and every desktop call site.

| Table | Purpose | Writer(s) | Reader(s) | Data shape | Authoritative vs. cache | Rebuild strategy | Backup importance | Growth | Retention | Sensitive risk |
|---|---|---|---|---|---|---|---|---|---|---|
| `meta` | Schema version + S2a migration flags + legacy project-name residue | `db::open` (version), `migration.rs` (flags/residue) | `db::open`, `recovery.rs` (`validate_database_file`) | JSON scalars/small structs, keyed by fixed string constants (`schema_version`, `legacy_preferences_migrated_at`, `legacy_telemetry_migrated_at`, `legacy_project_names`) | Authoritative (internal bookkeeping) | Recreated fresh on new DB; version stamped | Critical — a missing/wrong version blocks or mis-migrates everything else | Fixed, ~4 keys, never grows | Forever (small) | Low — no user data |
| `preferences` | Studio's own preference record (singleton) | `preferences_service.rs` (all Tauri commands), `theme.tsx` (theme only), `migration.rs` (S2a import) | Same + `AppShellGate.tsx` (indirectly via commands) | One JSON blob (`StudioPreferences`) under key `"singleton"` | Authoritative | Restore-from-backup or safe defaults (§10) | High | Fixed (one row) | Forever | Low — no credentials modeled in the struct (verified: no `apiKey`-shaped field exists in `StudioPreferences`, confirmed by reading `preferences.rs` in full) |
| `projects` | Studio's local project registry (local + cloud refs, favorite, last-opened) | `project_registry_service.rs`'s `registry_*` commands, `migration.rs`'s legacy-name reconciliation (`reconcile_legacy_project_names`) | `project_registry_service.rs`, `AppShellGate.tsx`/`Sidebar.tsx` (via commands, per `native.ts`'s `ProjectRegistryDto`) | One JSON row per `ProjectId` (`StudioProjectRecord`) | Authoritative for Studio's own bookkeeping only — **not** for project meaning (see §5) | **Not auto-rebuilt** on a fresh DB (see §5) | High for user favorites/recents; low for anything re-fetchable from daemon | Bounded by number of projects a user has (small, realistically <1000) | Forever (until `unregister`) | Low — no secrets; `ProjectKind::Cloud.organization_id` is a plain opaque string, not a typed credential |
| `project_cache` | Disposable display cache for project list rendering | **None found** — `ProjectCacheStore` exists and is tested, but no Tauri command or call site in `desktop/src-tauri` uses it | Same — unused | `StudioProjectCacheEntry` per `ProjectId` | Explicitly documented as never-authoritative | Trivial — clearing it is designed to be safe | None — nothing depends on it yet | N/A (empty in practice today) | N/A | Low |
| `sessions` | Application process run history (start/end/duration/crashed) | `session_service.rs` (`session_get_current`/`session_end_current` commands, and `setup()`/`shutdown_and_exit()` directly in `lib.rs`) | `session_service.rs`, `AppShellGate.tsx`-adjacent UI (via `getCurrentSession`/`getRecentSessions` in `native.ts`) | One JSON row per `SessionId` (`StudioSessionRecord`) | Authoritative | Disposable — a lost history doesn't break the app | Low | **Unbounded** — no cap, no pruning found in `session.rs` or anywhere in desktop (see §12) | Forever, no retention policy found | Low — `app_version`/`platform`/timestamps only |
| `telemetry_queue` | Durable, bounded queue for undelivered telemetry events | `telemetry.rs`'s `enqueue_to_db` (from `enqueue_telemetry_event` command and `enqueue_update_event`) | `telemetry.rs`'s `drain_queue` (background sender, `spawn_sender`) | One JSON row per event id (`StudioTelemetryEvent`) | Never authoritative — a queue | Fully disposable | None — best-effort by design | **Bounded** — `MAX_QUEUE_LEN = 500` enforced on every `enqueue` (`telemetry.rs` crate-side, verified in `crates/valori-studio-storage/src/telemetry.rs`) | Until delivered or pruned (`prune_older_than`, called with a 7-day cutoff on every drain tick per `desktop/src-tauri/src/telemetry.rs`'s `PRUNE_OLDER_THAN_MS`) | Depends entirely on what callers put in `properties` — see §9/§10 |
| `sync_state` | Studio-side sync bookkeeping per project | **None found** — `SyncStateStore` exists and is tested, no production call site | Same — unused | `StudioSyncState` per `ProjectId` (`project_id`, `last_sync`, `remote_version`, `dirty`, `conflict`) | N/A — never populated | N/A | None today | N/A | N/A | Low |
| `update_state` | Studio auto-updater bookkeeping | **None found** — `UpdateStateStore` exists and is tested; the real `tauri-plugin-updater` manages its own persistence independently (plugin-internal, not audited here as it's outside this crate) | Same — unused | `StudioUpdateState` (singleton: `last_checked`, `available_version`, `downloaded`, `downloaded_at`) | N/A — never populated | N/A | None today | N/A | N/A | Low |

**Verification note on `sessions`' unbounded growth**: confirmed by reading
`crates/valori-studio-storage/src/session.rs` in full — `start()`,
`end()`, `reconcile_crashed()`, `list()`, `recent()`, `open_sessions()` —
none of them delete or cap rows. Every process launch since install adds
exactly one permanent row. This is a real, previously undocumented growth
concern (see §12).

---

## 4. Legacy Persistence Audit

**Read-only migration compatibility (confirmed, not disputed):**

- `preferences.json` — read by `migration.rs`'s `migrate_legacy_preferences[_from_path]`, called once at startup by `studio_storage.rs`. No write call site found anywhere in `desktop/src-tauri` or `crates/valori-studio-storage`.
- `events.jsonl` — same pattern, `migrate_legacy_telemetry_queue[_from_path]`. No write call site found.

**Active production writes still bypassing `StudioDatabase` (confirmed by
this audit, not previously documented):**

- **`tauri-plugin-store` is still registered** as a Tauri plugin
  (`desktop/src-tauri/src/lib.rs:395`,
  `.plugin(tauri_plugin_store::Builder::default().build())`), even though
  no code anywhere calls `app.store(...)` or the JS `LazyStore` API
  anymore (both were confirmed removed from `preferences_service.rs` and
  `ui/src/lib/native.ts` in the S2b-2a phase). This is **dead plugin
  registration**, not an active write path, but it means the
  `preferences.json` file could still theoretically be written to by any
  future code that (re-)imports `@tauri-apps/plugin-store` client-side —
  nothing currently prevents that.
- **`ui/src/lib/theme.tsx`** `[FIXED — S2c]` — at the time of this audit,
  wrote theme to raw `localStorage` (`localStorage.setItem("valori-theme", p)`,
  line 89) **unconditionally, every time `setTheme` is called**, in
  addition to (not instead of) `setPreference("theme", p)` when running
  natively. This meant, inside the actual desktop app, theme was written
  to two places on every toggle — not just as a browser-dev-mode
  fallback. `loadTheme()` did prefer the native `studio.redb` value when
  available (line 51), so `studio.redb` won on read; the `localStorage`
  write was redundant, not actively harmful (no sensitivity), but was a
  real gap in what S2b-2a's own phase doc claimed ("preferences.json is
  preserved byte-for-byte unmodified" — true — but didn't mention the
  `theme.tsx` localStorage path was left unconverted). Fixed in
  `docs/phases/phase-studio-S2c-privacy-boundary-cleanup.md`:
  `theme.tsx` now branches on `nativeAvailable()` — desktop writes
  `studio.redb` only, browser/web writes `localStorage` only, with a
  one-time non-destructive backfill for installations that only had a
  legacy `localStorage` value.
- **`ui/src/lib/hooks/useEmbeddingConfig.ts`** and
  **`useLLMConfig.ts`** — both persist their full config, including a
  plaintext `apiKey` field, to `localStorage` (`valori:embedding_config`,
  `valori:llm_config`). This was flagged in the original S1 audit
  (`docs/architecture/studio-storage-audit.md` §11) as unresolved and
  explicitly out of scope for every phase since (S1 through DR all
  excluded credential storage from their stop conditions). Still
  completely unresolved as of this audit.
- **`ui/src/lib/onboarding.ts`** — a small "getting started checklist"
  (`markSearched`/`markProofViewed`/`dismissOnboarding`), stored under
  `valori-onboarding:*` keys in raw `localStorage`. This was never in
  scope for any Studio persistence phase — it's browser-only UI
  affordance state for a dashboard widget, not part of the
  `preferences`/`onboardingVersion` gate that *was* migrated
  (`ONBOARDING_VERSION` in `native.ts`, migrated to `studio.redb` in
  S2b-2a). Worth naming explicitly so it isn't confused with the
  migrated onboarding-completion flag.
- **`ui/src/lib/hooks/useProjectManifest.ts`**'s `localStorage` cache
  (`valori:projects-list`) is unchanged from before S1 — explicitly
  documented in that file as a disposable SWR-fallback cache, not
  authoritative. Not part of any migration's scope; still functions as
  designed.

**Target-architecture claim, verified:**

```text
Studio runtime
      ↓
typed Rust services
      ↓
StudioDatabase
      ↓
studio.redb
```

**True for**: preferences, project registry, sessions, telemetry queue —
each confirmed to route through a typed service (`preferences_service.rs`,
`project_registry_service.rs`, `session_service.rs`, `telemetry.rs`) with
zero direct `redb::` access found in `desktop/src-tauri` (`grep -rn
"redb::" desktop/src-tauri/src` returns nothing).

**Not true for**: theme (dual-write, see above), embedding/LLM provider
config (never migrated, not in scope), the onboarding checklist widget
(never intended to migrate), window size/position (owned by
`tauri-plugin-window-state`, a different plugin entirely, never routed
through `studio.redb` despite a `preferences.window_state` field existing
in the schema with no writer).

---

## 5. Project Storage Boundary

Verified locations, from `crates/valori-daemon/src/project.rs`:

- **`project.json`** — `crates/valori-daemon/src/project.rs`'s
  `ProjectManifest`, one file per project at
  `~/.valori/projects/<name>/project.json` (implied by `JsonProjectStore`,
  the sole `ProjectStore` implementor).
- **`events.log`** (WAL) — `ProjectManifest::event_log_path()` →
  `self.dir.join("events.log")` (`project.rs:166`).
- **`snapshot.val`** — `ProjectManifest::snapshot_path()` (name inferred) →
  `self.dir.join("snapshot.val")` (`project.rs:170`).
- **Indexes, vectors, collections** — held inside the kernel's snapshot
  format and in-memory `KernelState`, written to `snapshot.val`; not
  separate files (per `CLAUDE.md`'s architecture description, consistent
  with what this audit found — no separate index/vector files referenced
  anywhere in `valori-daemon` or `valori-studio-storage`).

**Ownership boundary, verified structurally, not just by convention:**

`crates/valori-studio-storage`'s `Cargo.toml` depends on `valori-domain`
only (`valori-domain = { workspace = true }`, verified by reading the
file), and this is mechanically enforced by
`crates/valori-node/tests/dependency_direction.rs`'s `SEALED_CRATES`
(`("valori-studio-storage", &["valori-domain"])`, line 77). `valori-daemon`
is not on that allowlist. A `grep -rn "redb::\|StudioDatabase" crates/valori-daemon/src`
found zero hits (not run in full here, but the dependency graph already
makes it impossible for `valori-daemon` to import
`valori-studio-storage` in the first place, since the firewall constrains
`valori-studio-storage`'s dependencies, and no edge points the other
way either — `valori-daemon`'s own `Cargo.toml`, per earlier phase audits,
depends on `valori-models` only among the crates relevant here).

**Confirmed: Studio recovery cannot modify project storage.**
`crates/valori-studio-storage/src/recovery.rs`'s every filesystem
operation (`preserve_corrupt`, `restore_backup`, `create_backup`) takes a
`db_path`/`backups_dir` argument scoped to `studio.redb` and its own
`backups/` subdirectory — none of them ever construct or touch a
`projects/` path. This was proven by test in the DR phase
(`project_data_is_never_touched_by_studio_database_recovery`, hash-verified)
and by a live desktop application run (see the DR phase doc).

**Does any Studio code accidentally treat the project registry as the
authoritative project itself?**
No accidental case found. `crate::project` module's own doc comment
explicitly disclaims this ("`StudioProjectRecord` is a thin Studio-local
persistence record... never a replacement for it"), and
`project_registry_service.rs`'s `StudioProjectDto` includes an
`available: bool` field computed by checking whether the cached local
path still exists on disk (`ProjectKind::Local { path } => path.exists()`,
line 61) rather than assuming the registry entry's existence implies the
project exists — the one place this distinction could have been blurred,
it wasn't.

---

## 6. Local vs. Cloud Project Storage

**Actual fields found** (`crates/valori-studio-storage/src/project.rs`,
`StudioProjectRecord` + `ProjectKind`):

```rust
StudioProjectRecord {
    id: ProjectId,
    display_name: String,
    kind: ProjectKind,        // Local | Cloud
    favorite: bool,
    last_opened_at: Option<i64>,
    registered_at: i64,
}

ProjectKind::Local { path: PathBuf }

ProjectKind::Cloud {
    organization_id: Option<String>,   // opaque string, not a typed OrganizationId
    cloud_endpoint: String,
    region: Option<String>,
}
```

**Not present, despite the prompt's example list** — `deployment_id` does
not exist anywhere in this struct. Not inventing it; not present in the
code.

**Local authoritative data**: the project's actual directory contents
(`project.json`/`events.log`/`snapshot.val`), owned by `valori-daemon` +
kernel — never `studio.redb`.

**Cloud authoritative data**: whatever Valori Cloud's own database holds
for a project — confirmed separately in `ui/src/app/cloud/CloudProjectsClient.tsx`'s
`CloudProject` type (`id, name, region, dim, index_type, status, node_url,
replication`) — note this is a **different, larger** shape than
`ProjectKind::Cloud`'s three fields, because the Cloud dashboard page
fetches live from Supabase and has no need for a thin reference — it's
already talking to the source of truth directly. `ProjectKind::Cloud` is
Studio's own, deliberately thinner, offline-capable reference.

**Studio cache**: `favorite`, `last_opened_at`, `registered_at` are
Studio-local facts with no other source of truth (genuinely authoritative
*for Studio's own bookkeeping*, per `crate::project` module docs);
`display_name` and the `Cloud` variant's fields are **not** authoritative —
they mirror what Studio was told at registration time and can drift from
the real Cloud state until re-synced (no mechanism currently re-syncs
them — see §7).

---

## 7. Sync Readiness

Answered strictly from what exists in `crates/valori-studio-storage/src/sync.rs`
and its call sites (none found in production code — see §3).

- **What is the synchronization unit?** `UNKNOWN — requires implementation/design
  decision`. `StudioSyncState` is keyed by `ProjectId` (`sync.rs:32`), which
  suggests "project" is the closest thing to an intended unit, but no code
  anywhere references collections, documents, operations, or snapshots as
  sync units. This is an inference from the key type, not a confirmed
  design — flagged as such.
- **What is the source of truth?** For a Cloud project, Cloud (per
  `ProjectKind::Cloud`'s module docs — "Cloud remains authoritative").
  `UNKNOWN` for anything more granular than "the whole project."
- **How are conflicts represented?** A single `bool` field, `conflict`
  (`sync.rs:40`). No conflict *detail* (which fields, which side, when)
  is modeled. `UNKNOWN — requires implementation/design decision` for
  anything beyond "yes/no."
- **Is there an existing version/epoch mechanism?** `remote_version:
  Option<String>` (`sync.rs:36`) exists but is untyped and unstructured —
  no `ClusterEpoch`, no vector clock, no monotonic counter tied to it.
  `valori-core::ClusterEpoch` exists in the kernel layer (per `CLAUDE.md`'s
  crate table) but nothing in `valori-studio-storage` references it — the
  dependency firewall would forbid it anyway (`valori-studio-storage` may
  only depend on `valori-domain`). `UNKNOWN` whether `remote_version` is
  meant to be an ETag, a timestamp, or something else — the field has no
  usage to infer intent from.
- **Are local and cloud project IDs compatible?** **Yes, structurally** —
  both `ProjectKind::Local` and `ProjectKind::Cloud` variants live under
  the same `StudioProjectRecord.id: ProjectId`
  (`valori_domain::ProjectId`), and `docs/architecture/ownership.md`
  (referenced throughout this crate's docs) establishes `ProjectId` as
  the one shared identity across local/Cloud. This part of the
  architecture genuinely is sync-ready.
- **Is there an existing change/event log sync could consume?** Yes, at
  the *project data* layer — `events.log` (the kernel's WAL, BLAKE3-chained,
  per `CLAUDE.md`) is a real, ordered event log. But there is **no
  connection between it and `studio.redb`'s `sync_state` table** — no code
  reads `events.log` to update `sync_state`, and no code reads
  `sync_state` to decide what to replay. `UNKNOWN` whether the intended
  design is "sync consumes the kernel's WAL" or something else — not
  determinable from current code, since nothing connects them yet.
- **Can sync be implemented without creating a second state-management
  system?** Structurally, yes — the `ProjectId` compatibility (above)
  and the existing WAL both look like they were designed with this in
  mind (per `docs/architecture/ownership.md`'s stated identity rule), but
  this is an architectural *possibility*, not a proven fact, since no
  sync code exists to validate it against. Flagged as an assessment, not
  a guarantee.

**Bottom line: `sync_state`'s schema exists; the sync engine, the
conflict model, and the connection to the WAL do not. Not currently
defined**, per the instruction to say so explicitly rather than fill
gaps.

---

## 8. Update State Readiness

**What the existing updater already stores, verified from
`desktop/src-tauri/src/lib.rs`:**

- `install_update` (line ~129) calls `app.updater()` →
  `updater.check().await` → `update.download_and_install(...)` — this is
  entirely `tauri-plugin-updater`'s own internal logic. No code in
  `desktop/src-tauri` persists a version, a release, a rollback marker, or
  a "failed update" record anywhere — not in `studio.redb`, not in a file.
- The only persistence *this codebase* adds around updates is
  **telemetry events**: `enqueue_update_event(&app, "update_check_started"/
  "update_download_started"/"update_download_completed"/
  "update_install_started"/"update_install_success"/"update_install_failed", ...)`
  (`lib.rs`, `install_update` and the background check task) — these are
  fire-and-forget telemetry events in `telemetry_queue`, not durable
  "what version am I on, did the last update fail" state. Once delivered
  (or pruned after 7 days), that history is gone.
- `studio.redb`'s `update_state` table (`last_checked`,
  `available_version`, `downloaded`, `downloaded_at`) exists and is fully
  implemented (`update.rs`) but — confirmed by `grep` — has **zero**
  production call sites.

**Is moving it into `studio.redb` actually necessary?**
`UNKNOWN — requires a product decision`, but the audit surfaces a concrete
gap it would close: today, "did the last update attempt fail" is only
ever visible transiently (as a telemetry event, if consent was on, until
delivered/pruned) — there's no durable, locally-queryable answer to "is
there an update available" or "did my last update succeed" after the
process restarts. Whether that gap is worth closing with
`update_state`, or left to `tauri-plugin-updater`'s own mechanism (not
audited here — out of this crate's scope), is a product call this audit
does not make.

**Fields the existing schema already has**: `last_checked: Option<i64>`,
`available_version: Option<String>`, `downloaded: bool`,
`downloaded_at: Option<i64>` (`update.rs`). No rollback or per-attempt
failure-reason field exists in the schema today; if "did the last update
fail, and why" needs to be answerable, that would be a schema addition —
not implemented here, per the stop condition.

---

## 9. Secrets Audit

| Location | What | Classification |
|---|---|---|
| `ui/src/lib/hooks/useEmbeddingConfig.ts` → `localStorage` (`valori:embedding_config`) | **`[FIXED — S3, desktop only]`** Desktop: `credentialRef` only, actual secret in OS keychain. Web: unchanged, still plaintext `apiKey` | Desktop: **Safe**. Web: **Unsafe** — documented, deferred limitation, not this phase's scope |
| `ui/src/lib/hooks/useLLMConfig.ts` → `localStorage` (`valori:llm_config`) | **`[FIXED — S3, desktop only]`** Same split as above (also applies to `SettingsModal.tsx`'s reranker config, `valori:reranker_config`, not in a dedicated hook) | Desktop: **Safe**. Web: **Unsafe** — same, deferred |
| `crates/valori-studio-storage/src/preferences.rs`'s `StudioPreferences` | No `apiKey`/token/secret field exists in the struct (verified by reading the full file) | **Safe** — cannot leak what it never models |
| `crates/valori-studio-storage/src/migration.rs`'s `LegacyPreferences` | Typed-field deserialization recognizes only 7 named fields; an `apiKey` key in `preferences.json` is silently dropped by serde, never copied through (verified by the DR-adjacent test `preferences_migration_tolerates_unknown_fields_and_never_copies_secrets`) | **Safe by construction** |
| `crates/valori-studio-storage/src/project.rs`'s `ProjectKind::Cloud.organization_id` | Plain opaque `String` reference, not a credential, not a typed `OrganizationId` (which is a Cloud-only concept per `dependency_direction.rs`) | **Safe** |
| `crates/valori-daemon/src/project.rs`'s `EmbeddingConfig.api_key_ref` | A *reference* (env var name / keychain id), explicitly never the raw secret, by design (doc comment, `project.rs:55-57`) | **Safe by design** |
| `ui/src/utils/supabase/client.ts` | Supabase access/refresh tokens in `localStorage` + mirrored cookie | **Server-only / standard practice** — this is the documented pattern for the Supabase SSR client library in use; classified safe under "library convention," not audited further here |
| `desktop/src-tauri/src/lib.rs`'s `auth-callback` deep-link handler | Explicitly does not read or store tokens on the Rust side — passes the query string straight through to the webview (comment confirms, unchanged since the original audit) | **Safe** |
| `crates/valori-studio-storage/src/telemetry.rs`'s `StudioTelemetryEvent.payload: serde_json::Value` | Arbitrary JSON from call sites — the type itself does not prevent a secret from being passed in `properties` | **Needs convention, not found to currently contain one** — no call site audited here passes anything secret-shaped; this is a structural risk (nothing stops it), not a confirmed leak |
| `desktop/src-tauri/src/telemetry.rs`'s `CrashInfo` | `panic_hash` (a hash, not the raw message), `panic_location`, `previous_session` (a `SessionId`, not personal data), `uptime_before_crash_secs` — no message text, no stack trace | **Safe** |
| `crates/valori-studio-storage/src/recovery.rs`'s `RecoveryLogEntry` | `recovery_timestamp`, `state`, `reason` (a short error `Display` string), paths, booleans — verified by test (`recovery_log_records_the_event_without_sensitive_payloads`) to never contain preference values | **Safe** |

**Migration residue**: none found — `preferences.json`'s real shape (per
the S2a migration's own hand-verified fixtures) has never contained a
credential field, so there is no plaintext secret sitting in
`studio.redb` today as a byproduct of migration.

**`[FIXED — S3]`** ~~Needs keychain: the two `localStorage` `apiKey`
fields~~ — done, on the desktop path. `desktop/src-tauri/src/credential_service.rs`
now wraps the OS credential store (`keyring`) and
`useEmbeddingConfig.ts`/`useLLMConfig.ts`/`SettingsModal.tsx`'s reranker
config persist `credentialRef` instead. The web/Cloud build's `localStorage`
`apiKey` storage remains — there is no OS keychain to move it to in a
browser tab — and is explicitly out of this phase's scope (documented, not
silently accepted). See `docs/phases/phase-studio-S3-credentials.md`.

---

## 10. Telemetry/Privacy Audit

- **What is stored locally?** Queued events in `telemetry_queue`
  (`StudioTelemetryEvent`: `event_id`, `created_at`, `event_name`,
  `session_id: Option<SessionId>`, `payload: serde_json::Value`,
  `attempt_count`, `last_attempt_at`) until delivered or pruned.
- **What is uploaded?** The same data, re-hydrated into a
  `TelemetryEnvelope` at drain time (`build_wire_envelope`,
  `desktop/src-tauri/src/telemetry.rs`) — adds `schema`, `source`,
  `installation_id` (read fresh from `preferences` at drain time, not
  stored per-event), `version`, `platform`, `arch` — POSTed to
  `https://api.valori.systems/v1/telemetry/events`.
- **What requires consent?** `analytics_consent(&app)` — reads
  `StudioPreferencesService::get_telemetry_consent().analytics`
  (`telemetry.rs:161-170`), checked in `enqueue_telemetry_event` and
  `enqueue_update_event` **before** anything is written to
  `telemetry_queue` at all — consent gates the write, not just the send.
- **What happens when consent is disabled?** Nothing is queued in the
  first place (`if !analytics_consent(&app) { return Ok(()); }`,
  `telemetry.rs:247-249`). Verified by the crate-level consent-boundary
  tests (`analytics_disabled_service_returns_false_and_queue_stays_empty`).
- **Can queued events remain after consent is revoked?** **`[FIXED — S2c]`**
  At the time of this audit: **Yes** — if events were queued while
  consent was on, then consent is turned off, `drain_queue` has no
  consent check of its own (it only checks
  `app.try_state::<Arc<StudioDatabase>>()`); it would still attempt to
  deliver whatever is already in the queue. Revoking consent stopped
  **new** events from being queued, but did not purge or block delivery
  of **already-queued** ones. Fixed in
  `docs/phases/phase-studio-S2c-privacy-boundary-cleanup.md`: every
  queued event now carries a `TelemetryCategory`; revocation eagerly
  discards already-queued events of that category
  (`discard_revoked_telemetry_categories`), and `drain_queue` independently
  re-checks consent per event, per category, immediately before every
  send — so even a hypothetical leftover can never be uploaded.
- **Are credentials or project contents ever included?** Not by any
  audited call site (see §9) — but the `payload: serde_json::Value` type
  is unconstrained, so this is enforced by convention/discipline at call
  sites, not by the type system.
- **Is telemetry bounded?** Yes — `MAX_QUEUE_LEN = 500` enforced on every
  `enqueue` (evicts oldest), plus a 7-day `prune_older_than` sweep on
  every drain tick (`PRUNE_OLDER_THAN_MS`, `desktop/src-tauri/src/telemetry.rs`).
- **What happens when the network is unavailable?** `drain_queue`'s POST
  fails or times out (10s timeout, `telemetry.rs`) → `increment_retry` is
  called, bumping `attempt_count`/`last_attempt_at`; the event stays
  queued for the next tick (every 60s, `spawn_sender`). No backoff — the
  same interval is used regardless of how many consecutive failures have
  occurred. Confirmed: no exponential-backoff or circuit-breaker logic
  exists in `drain_queue`.

---

## 11. Failure/Recovery Audit

Verified against `crates/valori-studio-storage/src/recovery.rs` and its
13-test suite (`tests/recovery.rs`), all passing as of the DR phase.

| Case | Status | Evidence |
|---|---|---|
| `studio.redb` missing | **Handled** | `open_with_recovery` creates fresh, `RecoveryOutcome::Healthy` — no recovery log entry (silent, correct for first install) |
| `studio.redb` corrupt | **Handled** | Preserved aside, backups tried, fresh fallback — `RecoveryOutcome::{RestoredFromBackup,FreshDatabaseCreated}` |
| Backup corrupt | **Handled** | `validate_database_file` skips it, tries the next generation |
| No backup | **Handled** | Falls to fresh database, app stays launchable |
| Database locked (another handle/process) | **Handled, deliberately not as "corruption"** | `DatabaseAlreadyOpen` returns a plain `Err`, not a recovery attempt (see the DR phase's own finding) |
| Permission denied | **Partially handled** | `preserve_corrupt` catches a rename failure and logs it, continuing to the backup/fresh path rather than aborting (`recovery.rs`'s `preserve_corrupt` returns `None` on error, doesn't propagate). But a permission error on the *database file itself* (can't even attempt a read) would surface as a generic `Err` from `StudioDatabase::open`, routed through the same corruption path — not specially distinguished from corruption. No dedicated test exercises a real OS-level permission-denied scenario (only simulated via corrupt bytes) |
| Disk full | **Partially handled** | If fresh-database creation itself fails (e.g. disk full), `open_with_recovery` returns `Err`, and `desktop/src-tauri`'s `init_studio_storage` treats that as "Studio storage unavailable, continue without it" (non-fatal to the app) — but this specific scenario (disk genuinely full) has no dedicated test; only the general "even fresh creation fails" contract is exercised indirectly through the error-propagation path |
| Read-only filesystem | **Not specifically tested** | Would manifest as an IO error on `create_dir_all`/`fs::rename`/`fs::copy`, all of which are `?`-propagated as generic errors — same graceful-degradation fallback applies, but no dedicated test simulates a read-only mount |
| Migration failure | **Handled** | Pre-migration backup trigger + fallback to that backup (or fresh) on failure — tested (`takes_a_backup_before_a_database_that_needs_migration_is_opened`) |
| Schema too new | **Handled** | `StudioStorageError::UnsupportedSchemaVersion`, file untouched (pre-dates DR, from S1) — this case is treated as a normal `open` failure and *does* enter the recovery path in `open_with_recovery` today (it doesn't special-case "too new" the way it special-cases `DatabaseAlreadyOpen`). Worth flagging: a schema-too-new database (e.g. after a downgrade) would get preserved-aside and replaced with an older-schema backup or a fresh database, which is arguably correct (can't use a future-schema DB anyway) but was not explicitly called out as a designed scenario in the DR phase's own test suite — no test exists named for this specific case. |

---

## 12. Durability/Growth Audit

| Table | Bounded? | Mechanism | Flag |
|---|---|---|---|
| `sessions` | **No** | None found — `start`/`end`/`reconcile_crashed` never delete rows | 🔴 Every app launch adds one permanent row, forever. At, say, 3 launches/day for a year, that's ~1,000 small JSON rows — not immediately dangerous, but the only table in this schema with genuinely unbounded growth and no pruning code anywhere. |
| `telemetry_queue` | **Yes** | `MAX_QUEUE_LEN = 500` (enqueue-time eviction) + 7-day `prune_older_than` sweep | None |
| `sync_state` | N/A (unused) | — | — |
| `preferences` | Fixed (1 row) | Singleton key | None |
| `update_state` | Fixed (1 row) | Singleton key | None |
| `projects` | Bounded by real project count | User-driven (register/unregister) | None — realistically small |
| `project_cache` | N/A (unused) | — | — |
| `meta` | Fixed (~4 keys) | Constant set of keys | None |

**"recent_queries" / "operations"** (named in the prompt's checklist) —
**not found anywhere** in `valori-studio-storage` or `desktop/src-tauri`.
No such table, field, or concept exists. `NOT DEFINED`.

**What could eventually affect startup/backup/recovery time**:
`sessions`' unbounded growth is the only concrete finding here. Backup
creation (`create_backup`) does a full-file `std::fs::copy` of
`studio.redb` — an ever-growing `sessions` table means an ever-growing
file, which means an ever-growing backup copy time and ever-growing
`validate_database_file` cost (which opens every table). At realistic
usage this is very unlikely to matter for years, but it is the one table
in the schema without any bound at all.

---

## 13. Concurrency Audit

**Mapped access paths** (`grep`-verified against `desktop/src-tauri/src`):

- **Tauri commands** (`preferences_service.rs`, `project_registry_service.rs`,
  `session_service.rs`, `studio_storage.rs`'s `get_studio_recovery_status`) —
  all go through `app.try_state::<Arc<StudioDatabase>>()`, never a direct
  `redb::Database` handle.
- **Background telemetry worker** (`spawn_sender`/`drain_queue`) — same,
  via `app.try_state::<Arc<StudioDatabase>>()`.
- **Session lifecycle** (`setup()`'s session-start block, `shutdown_and_exit()`'s
  session-end block, both in `lib.rs`) — same.
- **Updater** — does not touch `StudioDatabase` at all (see §8);
  `tauri-plugin-updater` manages its own state independently.
- **Sync** — no code exists (see §7).
- **Model downloads** — owned by `valori-models`/`valori-node`, entirely
  outside `valori-studio-storage`'s reach (different crate, different
  file, `~/.valori/models`) — confirmed no code path connects them.
- **Project operations** (register/rename/favorite/etc.) — all through
  `project_registry_service.rs`'s typed methods.

**Direct `redb::Database` access outside the crate**: none found — `grep
-rn "redb::" desktop/src-tauri/src` returns zero results.

**Multiple database instances**: exactly one production
`app.manage(Arc<StudioDatabase>)` call site
(`lib.rs:468`); every other `StudioDatabase::open` call found is inside a
`#[cfg(test)]` module (verified — 13 test-only call sites tallied across
`project_registry_service.rs`, `preferences_service.rs`, `studio_storage.rs`,
`session_service.rs`, `telemetry.rs`, none in production paths).

**Long-lived write transactions**: none found — every store method in
`valori-studio-storage` opens and commits its own transaction per call
(the pattern established in S1 and unchanged since, verified by spot-checking
`preferences.rs`, `session.rs`, `telemetry.rs`).

**Blocking operations**: `recovery.rs`'s filesystem operations
(`fs::copy`, `fs::rename`) are synchronous and run inside `setup()`,
which per Tauri's own execution model runs before the async runtime
event loop is fully driving the UI — this matches the existing,
already-reviewed design (recovery must complete before any service that
depends on the database starts, per `docs/architecture/studio-storage.md`
§17). Not flagged as a new concern — this was already the intended,
audited design of the DR phase.

**Conclusion: all production access goes through the intended typed
storage layer.** No violations found.

---

## 14. Architecture Boundaries Audit

**Intended direction, verified:**

```text
valori-domain
      ↓
valori-studio-storage
      ↓
desktop/src-tauri
      ↓
UI
```

- `crates/valori-studio-storage/Cargo.toml`: depends on `valori-domain`
  only (plus external crates `redb`, `serde`, `serde_json`, `thiserror`,
  `uuid`, `chrono`, `tracing` — none of them workspace crates).
- `desktop/src-tauri/Cargo.toml`: depends on `valori-studio-storage` (path
  dependency, confirmed present since S2b-1) and `valori-domain` directly
  (added in S2b-2a per that phase's own doc, for `InstallationId`/`SessionId`
  used in service signatures).
- UI reaches Studio storage only through Tauri commands (IPC), never a
  direct dependency — TypeScript cannot depend on a Rust crate.

**Does `kernel`/`node`/`consensus`/`storage`/`wire` depend on
`valori-studio-storage`?** No — verified two ways:
1. `crates/valori-node/tests/dependency_direction.rs`'s `SEALED_CRATES`
   entry for `valori-studio-storage` constrains *its* outgoing edges
   (`&["valori-domain"]`), which mechanically prevents it from ever
   depending on `valori-kernel`/`valori-node`/etc. (a cycle would be
   required for the reverse to happen, and `shipped_dependency_graph_is_acyclic`
   guards against that).
2. None of `valori-kernel`, `valori-node`, `valori-consensus`,
   `valori-storage`, `valori-wire`'s `Cargo.toml` files list
   `valori-studio-storage` as a dependency (spot-checked; these crates
   predate `valori-studio-storage`'s existence and nothing in this audit
   found a new edge added to any of them).

**Do the dependency-direction tests still protect this boundary?**
Yes — confirmed by running `cargo test -p valori-node --test
dependency_direction --test architecture` during this audit: **7/7
passing**, including `sealed_crates_depend_only_on_their_allowlist` (which
covers `valori-studio-storage`'s entry) and
`cloud_only_concepts_are_not_defined_in_oss_platform_core` (which also
covers it, per `OSS_PLATFORM_CORE`'s inclusion of `valori-studio-storage`).

---

## 15. Concept Duplication Audit

| Concept | Canonical implementation | Duplicates found | Differences | Migration risk |
|---|---|---|---|---|
| **Project** | `valori_domain::Project` / `ProjectId` (identity) | (1) `valori-daemon::ProjectManifest` (persistence), (2) `valori-studio-storage::StudioProjectRecord` (Studio reference), (3) `ui/src/app/cloud/CloudProjectsClient.tsx::CloudProject` (Cloud dashboard DTO), (4) `ui/src/lib/hooks/useProjectManifest.ts::ManifestProject` (daemon API DTO for the local project list UI) | Each carries only what its layer needs — `ProjectManifest` has `dim`/`index`/`workspace`/`restart_policy`; `StudioProjectRecord` has `favorite`/`last_opened_at`/`registered_at`; `CloudProject` has `region`/`dim`/`index_type`/`status`/`node_url`/`replication`; `ManifestProject` (TS) has `nodes`/`shardCount`/`collections`/`status`. This is the deliberate domain/persistence/API/UI split documented in `docs/architecture/ownership.md`, not an accidental duplication — but it is four representations of "project," worth naming together | Low — already the intended, documented pattern; the risk is a future field getting added to one representation and quietly meaning something different in another, which `docs/architecture/ownership.md`'s registry exists to prevent |
| **Collection** | `valori-core::NamespaceId` (kernel), `valori-metadata::Collection` (control-plane persistence) | None found in `valori-studio-storage` — confirmed no `Collection` type exists there | — | None — Studio never touches this concept |
| **Session** | Two genuinely distinct concepts sharing the English word, **not** the same thing duplicated: (1) `valori_domain::SessionId` + `crate::session::StudioSessionRecord` (Studio application process run), (2) `valori-planner`/`valori-metadata`'s `ExecutionId`/`ExecutionRecord` (a Valori operation execution) | Explicitly disclaimed as distinct in `crate::session`'s own module doc ("A session is... not a Valori execution") | The two concepts have zero field overlap and zero shared code | None — this was a deliberate naming clarification made during S2b-2c, not left ambiguous |
| **Model** | `valori-models::ModelManifest` (package/artifact metadata) | None found in `valori-studio-storage` — no model table, no model type | — | None |
| **Provider** | `ui/src/lib/hooks/useEmbeddingConfig.ts::EmbeddingProvider` (TS union type: `"openai" \| "cohere" \| "ollama" \| "custom"`), separately `crates/valori-daemon/src/project.rs::EmbeddingConfig.provider: Option<String>` (untyped string, not a shared enum) | Two representations, one typed (TS union) and one untyped (Rust `String`) — no shared Rust/TS contract enforces the daemon's `provider` field actually matches one of the four TS union values | Real drift risk: a value written to the daemon's `EmbeddingConfig.provider` by some other path (or a future provider added to the TS union without a corresponding daemon-side update) would not be caught by any type system | Medium — flagged here since it wasn't previously documented; not a `studio.redb` concern directly (this field lives in the daemon's `project.json`, not Studio storage) but adjacent enough to note |
| **Runtime** | `valori-daemon::Runtime` trait (`LocalRuntime` the only implementor) | None found in `valori-studio-storage` | — | None |
| **CloudProject** | See "Project" row above | — | — | — |
| **LocalProject** | `valori_domain::LocalProject` (per `docs/architecture/ownership.md`'s registry) — not found to be directly used inside `valori-studio-storage` (which uses `ProjectKind::Local { path }` instead, a narrower shape) | `ProjectKind::Local` vs. `valori_domain::LocalProject` | `LocalProject` (per earlier phase docs) is `{ project, root: PathBuf }`; `ProjectKind::Local` is just `{ path: PathBuf }`, with `id`/`display_name` living one level up on `StudioProjectRecord` instead | Low — different shapes for a reason (Studio's registry record already carries id/name at its own level), but worth naming as a place two "local project" shapes exist without one importing the other |
| **SyncState** | `crates/valori-studio-storage/src/sync.rs::StudioSyncState` — the only implementation found anywhere in the audited scope | None — no competing `SyncState` type exists elsewhere (Cloud's own sync mechanism, if any, lives in the private Cloud repository, out of this audit's scope) | — | None currently — but see §7's "not currently defined" findings; a future Cloud-side sync implementation could easily diverge from this shape since nothing connects them yet |

---

## 16. Target Architecture

```text
IMPLEMENTED:
UI (Next.js/React)
 ↓  (Tauri IPC — invoke/commands, not a Rust dependency)
Tauri commands / native bridge (desktop/src-tauri/src/{preferences,project_registry,session}_service.rs)
 ↓
typed services (StudioPreferencesService, ProjectRegistryService, SessionService)
 ↓
valori-studio-storage (StudioDatabase — typed accessors only, no raw redb in callers)
 ↓
studio.redb  (7 data tables + meta, recovery-aware open, bounded rolling backups)
```

```text
Studio metadata                                    STATUS
        │
        ├── Local project references                IMPLEMENTED (projects table)
        ├── Cloud project references                 IMPLEMENTED (ProjectKind::Cloud, thin reference only)
        ├── Preferences                               IMPLEMENTED (theme has a dual-write gap — see §4)
        ├── Sessions                                   IMPLEMENTED (unbounded growth — see §12)
        ├── Telemetry                                   IMPLEMENTED (queue + consent boundary + bounded + pruned)
        ├── Update state                                PLANNED — schema exists, zero production wiring
        └── Sync state                                  PLANNED — schema exists, zero production wiring,
                                                          engine/conflict-model/WAL-connection NOT DEFINED

Actual project data                                 STATUS
        │
        └── projects/<project>/
             ├── project.json                          IMPLEMENTED (valori-daemon)
             ├── WAL (events.log)                       IMPLEMENTED (valori-storage/valori-wire)
             ├── snapshots (snapshot.val)                IMPLEMENTED (valori-kernel)
             ├── indexes                                  IMPLEMENTED (inside the kernel snapshot format)
             └── vectors                                   IMPLEMENTED (inside the kernel snapshot format)

Studio storage recovery                              STATUS
        │
        └── backups/, studio-recovery.jsonl, corrupt-aside preservation
                                                        IMPLEMENTED (DR phase, structurally isolated
                                                        from the project-data tree above)

Not part of studio.redb at all, confirmed by this audit:
        │
        ├── Model metadata / artifacts (~/.valori/models)   IMPLEMENTED (valori-models, separate store)
        ├── Provider API keys (embedding/LLM)                 [FIXED — S3] desktop: credentialRef + OS keychain;
                                                                   web: still plaintext localStorage (documented gap)
        ├── Window size/position                               IMPLEMENTED via tauri-plugin-window-state,
                                                                   NOT via studio.redb despite a schema field existing
        ├── Getting-started checklist widget flags              IMPLEMENTED via raw localStorage, out of scope by design
        ├── Search history                                       NOT DEFINED — no code found anywhere
        └── Operational log file (studio.log)                    NOT DEFINED — console-only tracing today
```

---

## 17. Recommended Next Phases

Ranked by (1) correctness, (2) security, (3) data-loss risk, (4)
architectural dependency, (5) user impact, (6) implementation complexity —
**not** defaulted to the next sequential S-number, per instruction.

### 1. Fix the `theme.tsx` dual-write and the telemetry-consent-revocation gap `[FIXED — S2c]`

**Done** — see `docs/phases/phase-studio-S2c-privacy-boundary-cleanup.md`.
The rest of this section is left as originally written (the reasoning
that justified doing this first, for the record).

**Why now:** Both are small, concrete, already-diagnosed correctness bugs
found *by this audit* — not speculative. Neither requires new
architecture; both are a few lines each.
**What it unlocks:** Removes a real (if minor) drift source between
`localStorage` and `studio.redb` for theme, and closes a genuine privacy
gap — a user who revokes telemetry consent today can still have
already-queued events delivered afterward, which contradicts the spirit
of "what happens when consent is disabled" that every phase's telemetry
work has been careful about elsewhere.
**What it changes:** `theme.tsx` (stop writing `localStorage` when
native — or explicitly document why it's kept as a browser-mode
fallback and gate it on `!nativeAvailable()`); `drain_queue` (check
consent before sending, or clear the queue on consent revocation —
product decision needed on which).
**What it must NOT change:** Any other preference field's behavior; the
telemetry queue's data shape; the existing bounded/pruning behavior.
**Dependencies:** None.
**Risks:** Very low — small, localized, testable changes. The main risk
is picking the wrong product behavior for "revoke consent with events
already queued" (drop them vs. still send them) without a
product-owner decision — this audit deliberately does not decide that
for you.

### 2. Provider credential migration to OS keychain (the S3 target already designed)

**Why now:** This is the single highest-severity open finding across
every audit this project has produced (S1 through this one) — plaintext
API keys in `localStorage`, unresolved for the project's entire audited
history. Security should not keep losing to sequencing.
**What it unlocks:** Closes the one real "unsafe" row in §9's secrets
table; makes `credential_ref`-shaped fields in `studio.redb` (already
anticipated in `docs/architecture/studio-storage.md` §12's target
architecture) actually implementable without contradicting "never store
secrets in `studio.redb`."
**What it changes:** `useEmbeddingConfig.ts`/`useLLMConfig.ts` (stop
persisting `apiKey` to `localStorage`); a new OS-keychain integration
(Tauri has plugins for this, not audited here since implementation is
out of scope); `studio.redb`'s `preferences` schema likely gains
`credential_ref`-shaped fields per the already-documented target.
**What it must NOT change:** The daemon's `EmbeddingConfig.api_key_ref`
pattern (already correct, already reference-only) — this phase should
make the UI side match the daemon side's existing discipline, not
invent a third pattern.
**Dependencies:** A secrets-store decision (which keychain API, which
Tauri plugin) — genuinely a design decision this audit correctly did not
make.
**Risks:** Medium-high implementation complexity (cross-platform
keychain access has real platform differences — Keychain on macOS,
Credential Manager on Windows, Secret Service on Linux) and real
UX risk (users must re-enter keys once, migration path needed for
existing plaintext values).

### 3. Bound `sessions` table growth

**Why now:** The only genuinely unbounded table found in this audit
(§12) — a real, if slow-moving, data-loss/performance-risk category
(the same category the DR phase was built to address for the whole
database) applied to one specific table nobody has looked at yet.
**What it unlocks:** Keeps `studio.redb`'s size, backup-copy time, and
`validate_database_file` cost from growing forever — directly protects
the DR phase's own recovery-time guarantees from silently degrading
over a long-lived installation.
**What it changes:** `crate::session::SessionStore` gains a
pruning/retention method (e.g. "keep the last N sessions" or "prune
sessions older than N days"), called on a similar cadence to the
telemetry queue's own pruning (`prune_older_than` on `TelemetryQueue`
is the direct precedent to follow).
**What it must NOT change:** Crash-reconciliation logic
(`reconcile_crashed`) — pruning must not interfere with detecting a
crashed prior session; `sessions` remains disposable (per §3's
classification), so pruning is safe by the architecture's own existing
rules, not a new risk category.
**Dependencies:** None — self-contained within `valori-studio-storage`.
**Risks:** Low — bounded, well-precedented (mirrors
`TelemetryQueue::prune_older_than` almost exactly), easy to test.

**Explicitly not recommended first, despite being "next" in the
existing phase sequence:** S2b-2e (update_state/sync_state wiring) and
sync itself — both are real, but §7/§8 of this audit found their
underlying design questions (sync's conflict model, its connection to
the WAL, its unit of synchronization) genuinely unanswered in the
codebase today. Wiring `update_state` with no clear product need beyond
"the schema exists" (§8's own finding) is lower urgency than a live
security gap and a live unbounded-growth issue.

---

## 18. Open Architectural Decisions

- **Sync unit, conflict model, and WAL connection** — `UNKNOWN`, per §7.
  Needs a design pass before any sync engine work starts, independent of
  whether `sync_state`'s current schema turns out to be right.
- **Whether `update_state` is worth wiring at all**, versus relying on
  `tauri-plugin-updater`'s own internal state — `UNKNOWN`, a product
  call, per §8.
- **Provider-field type safety** (`EmbeddingProvider` TS union vs.
  untyped `String` on the daemon side) — no decision made here; flagged
  in §15 as a drift risk worth a future look, not a Studio-storage
  concern directly.
- **Telemetry-consent-revocation semantics** (drop queued events vs.
  still deliver them) — a product decision this audit surfaces (§10,
  §17 item 1) but does not make.
- **Whether a `studio.log` file sink should exist at all** — confirmed
  absent (§2, §11); whether one should be added is a product/ops
  decision, not decided here.
- **Whether `theme.tsx`'s `localStorage` write is an intentional
  browser-dev-mode fallback that just needs a `!nativeAvailable()` guard,
  or genuinely forgotten dead code from S2b-2a** — this audit could not
  determine intent from the code alone; the fix in §17 item 1 works
  either way, but the "why" is unresolved.
