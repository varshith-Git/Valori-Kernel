# valori-studio-storage

Durable Studio-local metadata store for Valori Studio (the Tauri desktop
app): preferences, the local/cloud project registry, a disposable project
display cache, application sessions, a durable telemetry queue, sync
bookkeeping, and updater state.

Storage backend: [`redb`](https://github.com/cberner/redb) — the same
embedded key-value store `valori-metadata` and `valori-consensus` already
use, but in a **separate file** (`~/.valori/studio.redb`), owned by this
crate alone. See `docs/architecture/studio-storage.md` for the full design
and `docs/architecture/studio-storage-audit.md` for the read-only audit this
crate implements (S1).

## What this crate is not

- **Not the Valori project data store.** Vectors, documents, WAL, snapshots,
  indexes, and model artifacts stay in `valori-kernel`/`valori-wire`/
  `valori-storage`/`valori-models`.
- **Not `valori-metadata`.** `~/.valori/metadata.redb` is a different file
  with a different owner (the node/daemon control plane).
- **Not a secrets store.** No table here may ever hold an API key, OAuth
  token, or other credential.
- **Not wired into the running desktop app yet.** `desktop/src-tauri` does
  not depend on this crate today; no existing runtime consumer
  (`preferences.json`, `events.jsonl`, `localStorage`, the telemetry
  sender, the updater) reads from or writes through `StudioDatabase`. This
  crate is self-contained and independently testable
  (`cargo test -p valori-studio-storage`).
- **S2a added a *migration engine*, not application wiring.** `crate::migration`
  can import `preferences.json`/`events.jsonl` bytes into `studio.redb` —
  detect/validate/import-transactionally/verify/mark-complete, idempotent,
  never touches the legacy files — but nothing calls it from the live app
  yet. See `crate::migration` module docs and
  `docs/architecture/studio-storage.md` §"Legacy data migration (S2a)".

## Modules

| Module | Contents |
|---|---|
| `db` | `StudioDatabase` — the single typed owner of `studio.redb`; `open`/`open_default`; schema creation and migration; `LegacyStudioPaths`/`LegacyMigrationSummary` + `migrate_legacy_*`/`run_legacy_migration` wrapper methods |
| `migration` | S2a — one-time, idempotent import of `preferences.json`/`events.jsonl` into `studio.redb`. Read-only against the legacy files; never wired into the live app (that's a later phase) |
| `path` | `default_home_dir`/`default_db_path` — `$VALORI_HOME`/`~/.valori` resolution, deliberately duplicated from `valori-daemon` (see module docs — this crate must stay leaf-ward) |
| `preferences` | `StudioPreferences` (now includes `installation_id`), `PreferencesStore` |
| `project` | `StudioProjectRecord`, `ProjectKind`, `ProjectRegistry` — local + cloud-reference project registry |
| `project_cache` | `StudioProjectCacheEntry`, `ProjectCacheStore` — disposable, never authoritative |
| `session` | `StudioSessionRecord`, `SessionStore` — **Studio application** sessions, not Valori executions |
| `telemetry` | `StudioTelemetryEvent`, `TelemetryQueue` — bounded, durable queue (storage layer only) |
| `sync` | `StudioSyncState`, `SyncStateStore` — Studio-side sync bookkeeping; Cloud stays authoritative |
| `update` | `StudioUpdateState`, `UpdateStateStore` |
| `error` | `StudioStorageError`, `StudioStorageResult` |
| `schema` (private) | Table definitions, schema version, JSON-over-redb helpers |

## Database layout

One `StudioDatabase` per Studio installation (`~/.valori/studio.redb`,
override with `$VALORI_HOME`):

| Table | Key | Value |
|---|---|---|
| `meta` | `"schema_version"` \| `"legacy_preferences_migrated_at"` \| `"legacy_telemetry_migrated_at"` \| `"legacy_project_names"` | JSON `u32` / `i64` / `i64` / `LegacyProjectNames` |
| `preferences` | `"singleton"` | JSON `StudioPreferences` |
| `projects` | `ProjectId` (string) | JSON `StudioProjectRecord` |
| `project_cache` | `ProjectId` (string) | JSON `StudioProjectCacheEntry` |
| `sessions` | `SessionId` (string) | JSON `StudioSessionRecord` |
| `telemetry_queue` | event id (uuid string) | JSON `StudioTelemetryEvent` |
| `sync_state` | `ProjectId` (string) | JSON `StudioSyncState` |
| `update_state` | `"singleton"` | JSON `StudioUpdateState` |

## Dependency graph position

```
valori-core  ──► valori-domain ──► valori-studio-storage   ← this crate
                                          │
                                    desktop/src-tauri  (not wired yet — S2b)
```

Sealed in `crates/valori-node/tests/dependency_direction.rs`'s
`SEALED_CRATES`: this crate may depend on `valori-domain` and nothing else
in the workspace — never `valori-daemon`, `valori-node`, `valori-metadata`,
or `valori-consensus`. (`chrono` was added in S2a, used only by
`migration.rs` to parse legacy RFC3339 timestamps — an external crate, not
a workspace dependency, so it doesn't touch `SEALED_CRATES`.)

## Key invariants

- One `StudioDatabase` file (`studio.redb`) per Studio installation, entirely
  separate from `~/.valori/metadata.redb` and any `raft-shardN.redb`.
- `project_cache` is disposable — clearing it must never affect `projects`.
- `projects`/`sync_state` never hold credentials; `ProjectKind::Cloud`'s
  `organization_id` is a plain opaque `String` reference, not the
  Cloud-owned `OrganizationId` type.
- Every stored value is JSON (`serde_json`), matching `valori-metadata`'s
  convention — forward-compatible via `#[serde(default)]`, not tied to Rust
  struct memory layout.
- Schema is explicitly versioned (`meta.schema_version`); opening a database
  from a newer schema version than this build supports fails clearly and
  leaves the file untouched — see `crate::db`.
- `crate::migration`'s legacy-data import never mutates, renames, or
  deletes `preferences.json`/`events.jsonl` — read-only against them,
  always. It never invents a `ProjectId` for a legacy project name; those
  land in `meta.legacy_project_names` as inert residue, not the `projects`
  table — see `crate::migration` module docs.
