# Phase Studio S2a — Legacy Studio persistence migration engine

**Branch:** `feat/node-object-store-durability-and-metrics`
**Depends on:** [`phase-studio-S1-durable-storage.md`](phase-studio-S1-durable-storage.md) (approved as-is)
**Status:** Complete — migration **engine** only. **Not wired into `desktop/src-tauri`.** No legacy files modified, renamed, or deleted. No runtime consumer changed.

> Naming note: same Studio track as S1, unrelated to the node/sharding
> track's `S`-numbered phases.

---

## Goal

Build a one-time, idempotent, transactionally-safe engine that imports
`preferences.json` and `events.jsonl` into `studio.redb` — detect →
validate → import transactionally → verify → mark complete — without
wiring it into the live desktop app, deleting the legacy files, or
changing any existing runtime consumer. Per the S1 review, split
deliberately from S2b ("wire the application") so the migration logic
itself is reviewable on its own before anything starts depending on it.

## Delivered

### `crates/valori-studio-storage/src/migration.rs` (new)

The five-step contract (detect/validate/import-transactionally/verify/
mark-complete), implemented for both legacy sources:

- `migrate_legacy_preferences(db, json_bytes, migrated_at)` /
  `migrate_legacy_preferences_from_path(db, path, migrated_at)` —
  parses `preferences.json`'s real shape (`onboardingVersion`,
  `telemetryConsent`, `installationId`, `lastPage`, `recentProjects`,
  `favoriteProjects`, `lastOpenedProject`), **merges** onto any
  pre-existing `StudioPreferences` row (never a blind overwrite), and
  writes the name-only project lists to `meta.legacy_project_names` —
  explicitly **not** the `projects` table (see Findings).
- `migrate_legacy_telemetry_queue(db, jsonl_bytes, migrated_at)` /
  `migrate_legacy_telemetry_queue_from_path(db, path, migrated_at)` —
  parses `events.jsonl`'s `TelemetryEnvelope` shape line by line;
  malformed lines, invalid timestamps, and invalid session ids are
  recorded in `MigrationReport::skipped` with a reason and excluded, not
  fatal; respects `TelemetryQueue::MAX_QUEUE_LEN` at import time (keeps
  the newest 500 by timestamp, same policy live `enqueue()` already
  enforces).
- `legacy_project_names(db)` — reads back the residue described above.
- All four import functions are **idempotent**: a `meta` flag
  (`legacy_preferences_migrated_at` / `legacy_telemetry_migrated_at`, a
  JSON `i64`) short-circuits a second call to a no-op.
- Every write (imported data + completed flag) happens in **one** redb
  write transaction; a failure before commit leaves nothing written.
- Every legacy-file access is `std::fs::read` only — no write, rename, or
  delete anywhere in this module.

### `StudioDatabase` wrapper methods (`db.rs`)

`migrate_legacy_preferences[_from_path]`,
`migrate_legacy_telemetry_queue[_from_path]`, `run_legacy_migration`
(orchestrates both against caller-supplied `LegacyStudioPaths`, each
independent — one failing doesn't block the other), `legacy_project_names`.
New public types: `LegacyStudioPaths`, `LegacyMigrationSummary`,
re-exported `MigrationReport`/`SkippedRecord`/`LegacyProjectNames` from
`crate::migration`.

### `StudioPreferences` extended (`preferences.rs`)

Added `installation_id: Option<valori_domain::InstallationId>` — a
genuine singleton fact (unlike the name-only project lists), matching
`ui/src/lib/native.ts`'s `getInstallationId()`, which today lazily writes
this into the same `preferences.json` key. No redb schema/table change —
purely an additive struct field, forward-compatible via `#[serde(default)]`,
consistent with `CURRENT_SCHEMA_VERSION` staying `1`.

### New `meta` keys (`schema.rs`)

`legacy_preferences_migrated_at`, `legacy_telemetry_migrated_at` (both
JSON `i64`), `legacy_project_names` (JSON `LegacyProjectNames`). No new
tables — these are `meta`-table scalars/small-structs, consistent with
S1's own "don't blindly create tables" discipline.

### New dependency

`chrono = { version = "0.4", default-features = false, features = ["std"] }`
— used only by `migration.rs` to parse `events.jsonl`'s RFC3339
`timestamp` field (the same format `desktop/src-tauri` already writes via
`chrono::Utc::now().to_rfc3339()`). No `"clock"` feature — this crate still
never reads the system clock itself.

### Documentation

- `docs/architecture/studio-storage.md` — new §6.5 "Legacy data migration
  (S2a)"; §12 extended with the `credential_ref`/OS-keychain target
  architecture (documented as an S3 design target, not implemented); §13
  rewritten to cover both S1 and S2a's scope boundaries; status header
  and cross-references updated.
- `crates/valori-studio-storage/README.md` — module table, database
  layout table, dependency graph, and invariants updated.
- This phase doc.

### Explicitly not touched

Same list as S1, plus: `preferences.json` and `events.jsonl` are read but
never written, renamed, or deleted; `desktop/src-tauri` gained no new
dependency and no `.rs` file there was touched; no runtime consumer
(the JS preference store, the telemetry sender, the updater, the daemon's
project registry) changed behavior.

## Findings

- **The legacy `recentProjects`/`favoriteProjects`/`lastOpenedProject`
  lists have no `ProjectId`** — `ui/src/lib/native.ts` has only ever
  tracked projects by name. This is a real design fork: minting a fresh
  `ProjectId` per name would create an identity the daemon's own
  `project.json` doesn't know about, directly violating the
  identity-preservation discipline S1 built `ProjectRegistry` around and
  the duplicate-identity problem `docs/architecture/ownership.md` exists
  to prevent. **Resolved by not populating the `projects` table at all**
  from this source — the names are preserved losslessly in
  `meta.legacy_project_names` as inert residue, explicitly documented as
  requiring a later phase to reconcile (by name, against the daemon's real
  project list) before any `ProjectId`-keyed entry is ever registered from
  them. This was a genuine judgment call, not an oversight — flagged here
  for review rather than silently decided.
- **`preferences.json` itself carries no credential-shaped field** —
  confirmed against the real source (`ui/src/lib/native.ts`); the `apiKey`
  exposure documented in the S1 audit lives entirely in `localStorage`
  (`useEmbeddingConfig`/`useLLMConfig`), a separate store this migration
  does not read in any form. `LegacyPreferences`'s typed-field
  deserialization (only seven named fields recognized) is a second,
  structural line of defense: even if a future `preferences.json` gained
  an `apiKey` key, it would be silently dropped by `serde`, not copied
  through, unless someone deliberately added a field for it — which this
  phase's tests guard against
  (`preferences_migration_tolerates_unknown_fields_and_never_copies_secrets`).
- **A borrow-checker/transaction-scoping subtlety recurred**: writing both
  a data table and the `meta` flag atomically required the
  `StudioDatabase.db` field to become `pub(crate)` (S1 kept it fully
  private) so `migration.rs` could open one write transaction spanning
  both tables — the existing per-store `schema::*_json` helpers each open
  their own transaction and can't be composed for cross-table atomicity.
  Documented at the field declaration with why.
- **Reused, not duplicated, the queue-capacity policy**: rather than
  inventing separate "what happens when migration finds too many events"
  logic, `migrate_legacy_telemetry_queue` applies the exact same
  newest-500-survives rule `TelemetryQueue::enqueue` already enforces for
  live traffic — verified by
  `telemetry_migration_respects_queue_capacity_keeping_newest` with 505
  synthetic events.

## Validation

```
cargo test -p valori-studio-storage
```
**77 tests, 0 failed, 0 ignored** (58 from S1 + 19 new in `tests/migration.rs`):

| New test file | Count | Covers |
|---|---|---|
| `tests/migration.rs` | 19 | real-shaped preferences.json/events.jsonl fixtures; idempotency (both sources); merge-not-overwrite onto pre-existing preferences; unknown-field/secret non-propagation; `legacy_project_names` residue correctness; `projects` table untouched; missing-file → `source_found: false`, not error; malformed preferences.json fails whole call without partial writes, retry still possible; malformed telemetry lines skipped not fatal; invalid session id skipped as a field, event still imported; unparseable timestamp skips the line; queue-capacity bounding (505 events → newest 500 kept); `run_legacy_migration` orchestrator (independent per-source, no-paths no-op); **legacy files never modified** (byte-for-byte before/after) |

```
cargo test -p valori-node --test dependency_direction --test architecture
```
7 tests, 0 failed — the new `chrono` dependency is external (not a
workspace crate), so it doesn't touch `SEALED_CRATES`; re-confirmed clean.

```
cargo check --workspace
```
Clean.

```
cargo fmt -p valori-studio-storage
cargo clippy -p valori-studio-storage --all-targets
```
Both clean (one clippy nit in a test — `useless_format!` — fixed via
`cargo clippy --fix`).

### Answering the required question

**Can an existing Valori Studio installation be upgraded to this version
without losing or invalidating existing preferences, telemetry, projects,
metadata, or other existing state?**

**Yes.** Same evidence class as S1: no file under `desktop/` or `ui/` is
touched (`git diff --stat` confirms), and this phase additionally proves —
by test, not assertion — that even the files this phase's code *does* read
(`preferences.json`, `events.jsonl`) are never written to:
`legacy_files_are_never_modified_by_migration` performs a byte-for-byte
comparison of both files before and after calling the migration functions
against them.

## Follow-ups

- **S2b: wire the application.** Nothing here is called from
  `desktop/src-tauri` yet. Per the review, this needs its own phase and
  its own review before proceeding — resolving Tauri's OS-specific
  `app_config_dir()` to real `preferences.json`/`events.jsonl` paths,
  calling `StudioDatabase::run_legacy_migration` once at startup
  (non-fatal on error), and — separately — making the actual runtime
  consumers (preference reads/writes, the telemetry sender, session
  lifecycle) read/write through `StudioDatabase` going forward.
- **Reconcile `meta.legacy_project_names` against real `ProjectId`s** —
  explicitly left undone (see Findings). Whichever phase has a live
  connection to the daemon's project list should resolve these names by
  lookup and register proper `ProjectId`-keyed `projects` entries, then
  the residue can be cleared.
- **Credential migration (S3)** — the `provider`/`model`/`credential_ref`
  + OS-keychain architecture is now documented as the design target
  (`studio-storage.md` §12) but not implemented. The `localStorage`
  plaintext `apiKey` exposure remains open and must not be "solved" by
  simply copying it into `studio.redb` — that was the review's explicit
  instruction and remains the constraint for S3.
- **Legacy file retirement** — deliberately not addressed. `preferences.json`/
  `events.jsonl` stay authoritative and untouched until S2b has proven the
  app runs correctly on `StudioDatabase`; only then should a later phase
  decide whether/when to stop reading them.
- Same open items as S1: no automatic corruption repair/backup, no
  fault-injection/crash test beyond clean-shutdown reopen.
