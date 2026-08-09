# Valori Studio storage — `studio.redb` (S1 + S2a + S2b-1 + S2b-2a + S2b-2b + S2b-2c + S2b-2d + DR + S2c + Installation Identity + S3 + S5 + S6 + S7)

**Status:** S1 (storage foundation), S2a (legacy migration engine), S2b-1
(startup lifecycle wiring), S2b-2a (preferences runtime migration), S2b-2b
(project registry runtime migration), S2b-2c (session store runtime
migration), S2b-2d (telemetry queue runtime migration), **DR (database
resilience & recovery)**, **S2c (privacy boundary & persistence
cleanup — telemetry consent revocation, theme dual-write removal)**,
**Installation Identity (installation_id generation decoupled from
telemetry consent — see §19)**, **S3 (Credential Security —
provider API keys move from plaintext `localStorage` to the OS credential
store on desktop; see §12 and
`docs/phases/phase-studio-S3-credentials.md`)**, **S5 (Session
Retention — bounded `sessions` table growth; see §20 and
`docs/phases/phase-studio-S5-session-retention.md`)**, and **S6 (Desktop
Filesystem Consolidation — canonical `StudioPaths`/`FileSystemService`,
lazy directory creation, atomic writes, path-traversal protection; see
§21 and `docs/phases/phase-studio-S6-filesystem-management.md`)**, and
**S7 (Persistence Boundary Cleanup — unified TS `$VALORI_HOME`
resolution, `modelDir` wired, `metadata.redb`'s dormancy formally
enforced, real bounded `logs/`/`crashes/` content, `tauri-plugin-store`
removed, `valori:notifs` migrated off `localStorage`, final consolidated
persistence-boundary architecture test; see §22 and
`docs/phases/phase-studio-S7-persistence-boundary.md`)** shipped.
`studio.redb`'s `sessions` table tracks desktop process runs (start, clean end, duration, crash).
`projects` table is strictly a Studio registry/reference layer.
Actual local projects (`~/.valori/projects/<name>/` — vectors, WAL, snapshots,
collections, indexes) remain authoritative in the daemon/storage layer.
Cloud projects remain authoritative in Valori Cloud.
`SessionId` (`valori_domain::SessionId`) and `ProjectId` (`valori_domain::ProjectId`) are canonical identity keys.
See `docs/phases/phase-studio-S2b-2c-sessions-runtime-migration.md` and
`docs/phases/phase-studio-DR-database-recovery.md` for full details.

**The DR invariant, stated once, load-bearing everywhere below:**
`studio.redb` is recoverable Studio metadata. It must never be allowed to
make Valori Studio permanently unlaunchable, and corruption of
`studio.redb` must never delete or modify the user's actual Valori project
data.

This document is the crate's own contract — what it owns, what it
guarantees, and what must never land in it. It supersedes the audit's
*proposals* with what actually shipped; where the two differ, this document
is current.

---

## 1. Database ownership

| Database | File | Owner | Consumer |
|---|---|---|---|
| **`studio.redb`** | `~/.valori/studio.redb` (`$VALORI_HOME` override) | `valori-studio-storage` (`StudioDatabase`) | `desktop/src-tauri` (`StudioPreferencesService`, `ProjectRegistryService`, `SessionService`, S2b-2c) |
| `metadata.redb` | `~/.valori/metadata.redb` | `valori-metadata` (`MetadataDb`) | `valori-planner`, future daemon/node wiring |
| `raft-shardN.redb` | inside a project's node data directory | `valori-consensus` | `valori-node` cluster mode |

These are three separate files, opened by three separate types, with no
code path that opens more than one of them. `valori-studio-storage` cannot
reach `valori-metadata` or `valori-consensus` even if it wanted to — see
§9 "Dependency direction" and
`crates/valori-node/tests/dependency_direction.rs`'s `SEALED_CRATES`.

## 2. Filesystem location

`crate::path::default_db_path()` resolves to `default_home_dir().join("studio.redb")`, where `default_home_dir()` is:

1. `$VALORI_HOME`, if set, else
2. `$HOME` (Unix/macOS) or `$USERPROFILE` (Windows), joined with `.valori`.

This is a **deliberate duplicate** of `valori_daemon::default_home()`'s
resolution rule, not a shared dependency — see `crate::path`'s module doc
for why (this crate must stay leaf-ward; depending on `valori-daemon` would
violate that). Both copies must be kept in sync if the rule ever changes;
each names the other in its doc comment.

`StudioDatabase::open(path)` creates `path`'s parent directory
(`std::fs::create_dir_all`) if it doesn't exist, so callers never need to
pre-create `~/.valori/` themselves. `StudioDatabase::open_default()` is the
convenience wrapper most callers should use.

## 3. Schema

Eight tables, all `TableDefinition<&str, &[u8]>` (string key, JSON-encoded
value bytes) — see `crates/valori-studio-storage/README.md`'s table for the
full key/value list. `meta` is internal (schema version only); the other
seven each have exactly one `*Store`/`*Registry` accessor on
`StudioDatabase`.

## 4. Table ownership — authoritative vs. cached

| Table | Authoritative for | Notes |
|---|---|---|
| `preferences` | Studio's own UI/consent state | No other source of truth exists for these fields |
| `projects` | Studio's local bookkeeping (favorite, last-opened, registration) | **Not** authoritative for project *meaning* — display name / path are best-effort mirrors of the daemon's `project.json` / Cloud's row |
| `project_cache` | Nothing — pure cache | Deleting it must never affect `projects`. See `crate::project_cache` module docs |
| `sessions` | Studio application sessions | Distinct from a Valori *execution* — see `crate::session` module docs |
| `telemetry_queue` | Nothing durable — a queue, not a log | Rows are deleted once delivered; no `delivered = true` history is kept |
| `sync_state` | Nothing — Studio's local belief about sync progress | Cloud remains authoritative for Cloud projects |
| `update_state` | What *this install* last checked/downloaded | The available version itself is derived from the update server, re-checked each time |

## 5. Serialization format

**JSON via `serde_json`**, matching `valori-metadata::MetadataDb`'s existing
convention — not `bincode` (used by `valori-consensus`'s Raft log, where
encode/decode cost matters on a hot path and both ends are always the same
build). Studio's workload is the opposite: low frequency, small values,
long-lived across app updates. Every stored struct is `#[serde(default)]`
on its optional fields, so a record written by an older build still
deserializes when a newer build adds a field — see `crate::schema` module
docs.

## 6. Schema versioning and migration strategy

`meta.schema_version` (a JSON `u32`) is the single source of truth for what
shape the database is in — never inferred from the application version.
`CURRENT_SCHEMA_VERSION = 1` today (`crate::schema`).

`StudioDatabase::open`:

- **No `meta` table at all** (fresh database, or a hypothetical
  pre-versioning one — the two are indistinguishable and treated
  identically): create every table in `schema::ALL_TABLES` (additive —
  `redb`'s `open_table` never truncates an existing table) and stamp
  `schema_version = CURRENT_SCHEMA_VERSION`, in one transaction.
- **Stored version == current**: defensively re-ensure every table exists
  (cheap no-op if they already do) and return.
- **Stored version < current**: run every migration in `db::MIGRATIONS`
  whose target is above the stored version, in ascending order, within
  **one** write transaction, then stamp the final version — all in the same
  commit. If any step returns `Err`, the transaction is dropped without
  committing (redb transactions are atomic), so the database is left
  exactly at its pre-migration version — never partially migrated, and the
  error is a structured `StudioStorageError::MigrationFailed { from, to,
  reason }`.
- **Stored version > current** (a newer build wrote this file, this build
  is older): refuse to open, before any write transaction is opened — the
  file is never touched. Returns
  `StudioStorageError::UnsupportedSchemaVersion { found, supported }`.
  Never silently downgrades or truncates unfamiliar tables.

`db::MIGRATIONS` is empty in S1 — schema v1 is the first version this
crate has ever shipped, so there is nothing to migrate *from*. It is the
scaffold every future schema change hangs its migration function on. Two
tests (`tests/database.rs`) exercise the parts of this contract that are
real today without a registered migration: `opening_a_pre_versioning_shaped_database_backfills_version_without_data_loss`
proves the "no `meta` table yet, but other tables already have rows" path
is additive, and `unsupported_future_schema_version_fails_clearly_and_preserves_the_file`
proves the refusal path never touches the file (byte-for-byte comparison
before/after).

## 6.5. Legacy data migration (S2a)

**Not to be confused with §6's schema migration.** §6 migrates
`studio.redb`'s own structure between versions of this crate. This section
covers a different thing entirely: a **one-time import of data from a
different, older store** (`preferences.json`, `events.jsonl`) into
`studio.redb`. `crate::migration` implements it; `db.rs` exposes it as
`StudioDatabase::migrate_legacy_preferences`/`migrate_legacy_telemetry_queue`
(plus `_from_path` variants and a `run_legacy_migration` orchestrator).

**The five-step contract** (mirrors the phase brief exactly):

1. **Detect** — check a `meta` flag (`legacy_preferences_migrated_at` /
   `legacy_telemetry_migrated_at`, a JSON `i64` unix-ms timestamp). Already
   set → return immediately, touching nothing. Safe and cheap to call on
   every app startup once wired up.
2. **Validate** — parse the legacy bytes fully before writing anything.
   `preferences.json` is one JSON object: malformed fails the whole call,
   nothing written. `events.jsonl` is one JSON value per line: each line
   is validated independently, a bad line is recorded in
   `MigrationReport::skipped` with a reason and excluded, the rest still
   imports.
3. **Import transactionally** — the imported data *and* the
   migration-completed flag are written in **one** `redb` write
   transaction. A failure before `commit()` leaves nothing written — never
   "data written but flag unset" (silent re-import next call) or the
   reverse.
4. **Verify** — a fresh read transaction after commit confirms the flag
   and (for telemetry) the row count are actually there.
5. **Mark migration complete** — the flag from step 3, confirmed in step
   4. Not a separate call.

**Never modifies, renames, or deletes the legacy files.** Every migration
function takes bytes or calls `std::fs::read` on a path — no write, no
rename, no delete, anywhere in this module. A missing legacy file is
`MigrationReport::source_found == false`, not an error — the normal case
for a fresh install.

**`preferences.json` field mapping**: `onboardingVersion`,
`telemetryConsent`, `installationId`, `lastPage` merge into
`StudioPreferences` (existing values for fields the legacy JSON doesn't
have are preserved, not overwritten — a real merge, not a blind replace).
Any field not in this list — including a hypothetical `apiKey` — is
silently dropped by `serde`'s "unknown fields are ignored" default, never
copied through; see §12 and `crate::migration`'s module docs for why this
matters.

**`recentProjects`/`favoriteProjects`/`lastOpenedProject` are name-only in
the legacy source** (no `ProjectId` — `ui/`'s preferences.json has never
tracked one). Minting a fresh `ProjectId` for each name here would create
an identity the daemon's own `project.json` doesn't know about — exactly
the duplicate-identity problem `docs/architecture/ownership.md` exists to
prevent. Instead they land in `meta.legacy_project_names`
(`crate::migration::LegacyProjectNames`) as inert, read-only residue —
**not** the `projects` table — for a later phase to reconcile by name
against the daemon's real project list before ever registering a
`ProjectId`-keyed entry.

**`events.jsonl` → `telemetry_queue`**: each envelope's `timestamp`
(RFC3339, via `chrono`) becomes `created_at` (unix ms); `session_id`, if
present, is parsed as a `valori_domain::SessionId` — invalid values are
skipped (recorded, not fatal) rather than aborting the import.
[`TelemetryQueue::MAX_QUEUE_LEN`]'s bound is respected at import time too:
if more than 500 valid events are found, only the newest 500 (by
timestamp) are imported, the rest recorded as skipped with reason
`"queue capacity"` — the same oldest-evicted policy live `enqueue()`
already enforces, applied consistently to a bulk import.

**Tests**: `tests/migration.rs`, 19 tests — real-shaped fixtures (not
synthetic minimal JSON) for both sources; idempotency; merge-not-overwrite
onto pre-existing preferences; unknown-field/secret non-propagation;
missing-file handling; malformed-file/malformed-line handling
(whole-file failure for preferences, per-line skip for telemetry);
timestamp and session-id validation; queue-capacity bounding; the
orchestrator; and a direct byte-for-byte proof that migration never
modifies either legacy file.

## 7. Concurrency model

`StudioDatabase` wraps a bare `redb::Database`, **no** `Arc<Mutex<_>>`
around it — every store method (`preferences().get()`,
`telemetry().enqueue()`, …) opens and commits its own transaction, mirroring
`valori-metadata::MetadataDb`'s existing, already-vetted pattern. This
works because redb serializes write transactions internally and lets
readers proceed against a consistent snapshot without blocking on a
writer — wrapping it in an external mutex would only add contention redb's
own transaction model doesn't need, and would risk a mutex held across an
`.await` if a callsite got it wrong.

**Recommended ownership pattern for callers** (S2b, when `desktop/src-tauri`
wires this in): one `Arc<StudioDatabase>`, constructed once at startup and
shared via Tauri's managed state across every Tauri command, the telemetry
sender task, the session lifecycle, and any future update/sync worker. Do
not construct more than one `StudioDatabase` onto the same file from the
same process.

**Cross-process**: `desktop/src-tauri` already depends on
`tauri-plugin-single-instance` (wired in `lib.rs`), which guarantees only
one Studio process runs at a time — this is what makes "exactly one open
handle on `studio.redb`" hold at the process level, not just within one
`Arc`.

Verified by `tests/concurrency.rs`: concurrent writers to the same table
lose no writes; concurrent writers to different tables don't interfere;
concurrent readers observe only fully-committed values, never a torn read;
a panic inside an `update()` closure aborts the transaction without
partially applying it; the database reopens correctly after heavy
concurrent write load.

## 8. Durability assumptions

redb 2.6.3 (workspace-pinned, same version already used by the Raft log)
commits are fsync-backed by default — the same guarantee
`valori-consensus`'s log store doc comment calls load-bearing for Raft
correctness (a lost vote after acknowledgment can elect two leaders in the
same term). Studio inherits that guarantee for free; its own durability bar
is lower than consensus, not higher.

What this crate does **not** independently verify: byte-level behavior
under a real process kill mid-fsync (no fault-injection test exists for
either existing redb user in this codebase, and none was added here — see
`docs/architecture/studio-storage-audit.md` §13's flagged gap, still open).
`tests/concurrency.rs::survives_reopen_after_heavy_concurrent_writes`
proves a clean-shutdown reopen round-trip; it does not simulate a kill
mid-transaction.

## 9. Dependency direction

`valori-studio-storage` depends on `valori-domain` (for `ProjectId`,
`SessionId`, `InstallationId`) and `redb`/`serde`/`serde_json`/`thiserror`/
`uuid`/`chrono` (the last added in S2a, for RFC3339 parsing in
`migration.rs` only) — nothing else in the workspace. It is sealed in
`crates/valori-node/tests/dependency_direction.rs`'s `SEALED_CRATES`
(allowlist: `valori-domain` only) and included in `OSS_PLATFORM_CORE` (the
Cloud-concept ban applies to it — it may never *define*
`OrganizationId`/`UserId`/etc., though it may hold an *opaque string*
reference to one, as `ProjectKind::Cloud::organization_id` does).
`EXPECTED_EDGES` asserts the `valori-studio-storage → valori-domain` edge
parses. All three assertions are enforced by
`cargo test -p valori-node --test dependency_direction`.

## 10. Corruption behavior and recovery (DR phase)

`StudioDatabase::open` (the plain primitive) never deletes or recreates a
database it failed to open — it still just surfaces `Err`, file untouched.
That primitive is what every store method, every existing test, and
`crate::recovery` itself are built on. What changed in the DR phase is
that **nothing calls the plain primitive at the top of the startup path
anymore** — `desktop/src-tauri` calls `crate::recovery::open_with_recovery`
(via `StudioDatabase::open_default_with_recovery`), which wraps it with the
order below and is designed to always end in a usable database.

### Normal path

```text
studio.redb
    ↓
normal startup
```

### Corruption path

```text
studio.redb
    ↓
preserve original (atomic rename to studio.redb.corrupt-<unix_ms>)
    ↓
validate backups, newest generation first
    ↓
restore the first one that validates
    ↓
   — or, if none do —
    ↓
create a fresh studio.redb
    ↓
Studio starts either way
```

### Recovery order, exactly as implemented (`crate::recovery::open_with_recovery`)

1. Try opening the current `studio.redb`.
2. Opens → `RecoveryOutcome::Healthy`, done. (A locked-but-healthy database —
   `DatabaseError::DatabaseAlreadyOpen`, e.g. a second process — is
   returned as a plain `Err` here, **not** treated as corruption; see
   §17 "Concurrency".)
3. Doesn't open → preserve the original: `fs::rename` to
   `studio.redb.corrupt-<unix_ms>` in the same directory (same
   filesystem — atomic on macOS/Windows/Linux), with a counter suffix if
   that exact name is somehow already taken. Never deletes, never
   overwrites an existing preserved file.
4. Try backup generations `studio.redb.1` (newest) through
   `studio.redb.{BACKUP_GENERATIONS}` (oldest, currently 3), in that
   order.
5. The first one that both **opens** (`redb::Database::open`, not
   `::create` — never mutates the backup) and has a schema version in
   `1..=CURRENT_SCHEMA_VERSION` with every table in `schema::ALL_TABLES`
   openable is copied to a temp file beside `studio.redb` and
   `fs::rename`d into place (atomic — `studio.redb` is never observed
   half-written), then opened normally → `RecoveryOutcome::RestoredFromBackup`.
6. None validate (or no backups exist) → `StudioDatabase::open` on the
   now-empty path creates a fresh database →
   `RecoveryOutcome::FreshDatabaseCreated`.
7. Rebuilding: see the "Rebuild classification" table below — this step is
   almost entirely a no-op by design, not an unfinished feature.
8. Either way, `open_with_recovery` returns `Ok` — the only `Err` case is
   fresh-database creation itself failing (disk full, unwritable
   directory), which `desktop/src-tauri`'s `init_studio_storage` already
   treats as "Studio storage unavailable, continue without it" (see §"DR
   and the pre-existing graceful-degradation fix" below).

### Backup strategy

`$VALORI_HOME/backups/studio.redb.{1,2,3}` — a small, bounded rolling
window (`BACKUP_GENERATIONS = 3`), never an unbounded archive. Created by
`crate::recovery::create_backup`: copy the live file to a temp name beside
the backups directory, then rotate existing generations down by
`fs::rename` (oldest dropped) and rename the temp copy in as generation 1
— every step is a single atomic rename; a crash mid-rotation loses at most
one generation of history and can never corrupt an existing valid backup
file.

**Two triggers, both cheap and bounded — never on a hot path (a preference
write, a telemetry enqueue, a session start never trigger a backup):**

- **Before a schema migration** — `open_with_recovery` inspects the
  on-disk version (read-only, via `Database::open`, never mutates) before
  attempting the real open; if it's below `CURRENT_SCHEMA_VERSION`, it
  backs up first. If the migration then fails, the normal recovery path
  above has that exact pre-migration state to restore.
- **Periodic** — at most once every 24h (`PERIODIC_BACKUP_INTERVAL_SECS`),
  gated by the newest backup's mtime, checked only on a *healthy* open
  (once per process start at most, never more).

### Backup validation

A backup is never trusted merely because the file exists.
`crate::recovery::validate_database_file` opens it read-existing-only
(`redb::Database::open`, never `::create` — a backup is never mutated by
validation) and checks: `meta.schema_version` is present and in
`1..=CURRENT_SCHEMA_VERSION`, and every table in `schema::ALL_TABLES` opens
cleanly. Only then is it eligible for restoration. An invalid generation
is skipped, not fatal — the next-oldest generation is tried.

### Fresh database fallback

If the current database and every backup generation are all invalid, a
fresh `studio.redb` is created and the app starts — the user is never
locked out. `RecoveryOutcome::FreshDatabaseCreated` (and the recovery log —
see below) explicitly records that this happened; it is never silent.

### Rebuild classification

See `crate::recovery` module's doc comment for the full evidence-based
table (reproduced in `docs/phases/phase-studio-DR-database-recovery.md`).
Summary: `preferences` restores from backup or falls back to safe
defaults; `update_state`/`telemetry_queue`/`sessions` are trivially
rebuildable-to-empty or already-disposable by existing product semantics;
`sync_state` is re-derivable from Cloud once a sync engine exists; the
`projects` registry and `meta.legacy_project_names` are **not**
automatically rebuilt — this crate has no parser for the daemon's
`project.json` and must not gain one (would violate the dependency
firewall), so a fresh registry starts empty, same as first install.
**Never, under any circumstance, rebuilt inside `studio.redb`:** WAL,
snapshots, vectors, indexes — this crate has no code path to them at all.

### Recovery marker / log

`$VALORI_HOME/studio-recovery.jsonl` — a **sibling of** `studio.redb`, not
a table inside it, so a corruption event that destroys `studio.redb`
cannot also destroy the record that corruption happened. Append-only JSON
lines (`crate::recovery::RecoveryLogEntry`): `recovery_timestamp`, `state`
(`RecoveryState::{Healthy,RecoveryRequired,RestoringBackup,Rebuilding,Recovered,RecoveryFailed}`),
`reason` (a short technical string — the underlying error's `Display`,
never the primary user-facing message), `original_database_path`,
`backup_attempted`, `backup_restored`, `fresh_database_created`. Never
contains preference values, telemetry payloads, project content, or
credentials — mechanics only. Writing the log is best-effort: a failure to
write it never fails recovery itself.

### Crash-safe / idempotent recovery

Recovery is safe to interrupt and re-run. Two cases:

- **Run twice on an already-healthy database**: the second call is a plain
  healthy open — no new preserved file, no re-recovery, no duplicate
  backup rotation beyond the periodic-backup gate.
- **Process crashes between "preserve" and "restore/fresh"**: `studio.redb`
  is absent (already renamed aside) on the next launch. Rather than
  silently treating "absent" as "fresh install" and creating an empty
  database (losing the chance to restore a perfectly good backup),
  `open_with_recovery` checks for a leftover `studio.redb.corrupt-*`
  marker and, if one exists, re-enters the *same* recovery path (backups
  → fresh) rather than a separate "resume" code path that could drift out
  of sync with it. No separate lock/marker file is needed — the state is
  derived purely from what's on disk.

Every filesystem mutation recovery performs (preserve, backup rotation,
restore) is a single atomic `fs::rename`, so there is never a
half-written `studio.redb` observable at its live path, on any of macOS,
Windows, or Linux.

### DR and the pre-existing graceful-degradation fix

`desktop/src-tauri`'s `init_studio_storage` already treats any failure to
produce a usable `StudioDatabase` as non-fatal to the app (a fix from
before the DR phase — see the phase history). DR's contribution is making
that failure mode nearly unreachable in practice: the only way
`open_with_recovery` still returns `Err` is if even creating a brand-new,
empty `studio.redb` fails (disk full, an unwritable `~/.valori`) — at
which point Studio storage is disabled for that session (a
`RecoveryStatusDto::Unavailable` notice is shown; see §"Recovery UI"
below) and the rest of the app is unaffected, exactly as before DR
existed.

## 11. Backward compatibility

Neither S1 nor S2a touches any existing Studio persistence:
`preferences.json`/`tauri-plugin-store`, `events.jsonl`, `localStorage`,
the daemon's `project.json` format, `metadata.redb`, or any Raft
`redb` file. `desktop/src-tauri` does not depend on `valori-studio-storage`
as of S2a either, so no existing installation's behavior changes at all by
merely upgrading to this code — the crate is inert (S1) or read-only
against legacy files and inert on their content (S2a) until something
actually calls it from the live app.

**Can an existing Valori Studio installation be upgraded to this version
without losing or invalidating existing preferences, telemetry, projects,
metadata, or other existing state?**

Yes — verified, not assumed: `cargo test --workspace` (see the phase docs
for exact counts) shows every pre-existing test still passes unmodified,
and no file either phase touches is a file any existing Studio installation
reads or writes. `studio.redb` is a brand-new file at a path
(`~/.valori/studio.redb`) nothing has ever created before, so its mere
existence cannot collide with or shadow anything. S2a's migration functions
only ever call `std::fs::read` on the legacy files — `tests/migration.rs::legacy_files_are_never_modified_by_migration`
proves this with a byte-for-byte before/after comparison. The claim is
bounded to what S1+S2a actually ship — it says nothing about S2b, which
will be the phase that actually points existing read/write call sites at
this crate and therefore the phase where a real backward-compatibility
risk first exists.

## 12. Data that must never enter `studio.redb`

Enforced by convention and by the crate's own module docs (not yet by a
mechanical test — see phase-doc follow-ups):

- API keys, OAuth access/refresh tokens, provider secrets, S3/GitHub
  credentials, database passwords, private keys — **never**, in any table,
  in any field. `ProjectKind::Cloud`'s `organization_id` is a plain opaque
  reference string, not a credential. This is an architectural rule, not
  just a description of what happens not to be here today — S2a's
  `crate::migration` deliberately does not carry through an `apiKey` field
  even though the legacy `ManifestProject`/embed-config TS shapes have one
  (see `crate::migration` module docs); a future migration of *that* data
  must not simply widen this rule to let it in.
  - **Provider credentials (Studio S3, shipped)**: the actual secret lives
    only in the OS credential store (`keyring`, via
    `desktop/src-tauri/src/credential_service.rs`'s `CredentialService`),
    never `studio.redb`:
    ```text
                    Studio
                      │
           ┌──────────┴──────────┐
           │                     │
     Configuration           Credentials
           │                     │
           ▼                     ▼
      localStorage             OS keychain
    (provider, model,      (the actual secret,
     credentialRef)          keyed by credentialRef)
    ```
    `studio.redb` does not persist provider configuration at all, in
    either the pre-S3 or post-S3 shape — it never did (verified by the
    credentials audit). Provider config (`provider`, `model`,
    `credentialRef`) stays in `localStorage`, its existing location since
    before S1, unchanged by S3 — only the secret-bearing field's shape
    changed, from a plaintext `apiKey` to an opaque `credentialRef`. This
    is deliberate: routing provider config through `studio.redb` for the
    first time would have been an unrelated persistence-location change,
    out of S3's explicit scope. See
    `docs/reviews/studio-credentials-audit.md` and
    `docs/phases/phase-studio-S3-credentials.md` for the full design and
    why `studio.redb`'s own zero-secret guarantee (this section) still
    holds unchanged — S3 added a regression test for it
    (`credential_security_architecture.rs`), it did not need to add a new
    guarantee.
    The web/Cloud build (no OS keychain reachable from a browser tab)
    keeps storing `apiKey` directly in `localStorage`, exactly as before
    S3 — a real, documented, deliberately out-of-scope limitation, not a
    silently accepted one.
- Vectors, documents, WAL entries, snapshots, indexes, graph data, model
  artifact bytes — owned by `valori-kernel`/`valori-wire`/`valori-storage`/
  `valori-models`.
- Collections/namespace mappings, planner cache, execution history — owned
  by `valori-metadata`'s `MetadataDb`.
- Raft log entries, vote state — owned by `valori-consensus`.
- A second, competing copy of `Project`/`ProjectId` meaning —
  `StudioProjectRecord` is a thin Studio-local persistence record built
  *around* `valori_domain::ProjectId`, never a replacement for it.

## 13. S2b-1 Startup Integration & Physical Path Behavior

In S2b-1, `desktop/src-tauri` connects the S2a migration engine to Tauri startup:

```text
Launch Valori Studio
        ↓
telemetry::init_session_id()
        ↓
resolve default_db_path() / default_backups_dir() / default_recovery_log_path()
        ↓
open_with_recovery(&db_path, &backups_dir, &recovery_log_path)   ← DR phase
        ↓ (preserve/restore-backup/fresh-fallback happens here if needed —
        ↓  see §10; this step cannot fail the app's launch)
resolve_legacy_paths(app) via app.path().app_config_dir()
        ↓
db.run_legacy_migration(&legacy_paths, now)
        ↓
log_migration_summary() (sanitized, non-sensitive)
        ↓
app.manage(studio_db: Arc<StudioDatabase>)
app.manage(RecoveryStatusDto), app.emit("studio-recovery", ...)   ← DR phase
        ↓
continue normal Tauri startup (menus, tray, deep links, webview)
```

`StudioDatabase::open(&db_path)` (the plain, non-recovering primitive) is
no longer what `desktop/src-tauri` calls at startup — see §10 for why.

### Physical path resolution per platform:
- **`studio.redb`**: Always resolves via `valori_studio_storage::path::default_db_path()`, which checks `$VALORI_HOME` override or falls back to `$HOME/.valori/studio.redb` (macOS/Linux) and `%USERPROFILE%\.valori\studio.redb` (Windows).
- **Legacy `preferences.json` and `events.jsonl`**: Resolved via Tauri's OS-native `app.path().app_config_dir()`:
  - **macOS**: `~/Library/Application Support/com.valori.desktop/`
  - **Windows**: `%APPDATA%\com.valori.desktop`
  - **Linux**: `~/.config/com.valori.desktop`

## 14. S2b-2 Staged Rollout (S2b-2a + S2b-2b + S2b-2c + S2b-2d + S2b-2d.1 Complete)

Runtime consumer migration is split into discrete, independently testable sub-phases:
- **S2b-2a (Complete)**: `StudioPreferencesService` — Next.js and Rust-native preference reads/writes go to `studio.redb`'s `preferences` table. `preferences.json` is preserved byte-for-byte unmodified.
- **S2b-2b (Complete)**: `ProjectRegistryService` — Project registry, recents, and favorites backed by `studio.redb`'s `projects` table using canonical `valori_domain::ProjectId`. `studio.redb` is strictly a registry index and never the project database.
- **S2b-2c (Complete)**: `SessionService` — Application session history and crash reconciliation backed by `studio.redb`'s `sessions` table using `valori_domain::SessionId`. Process shutdown records clean exit; restarts flag unended sessions as crashed.
- **S2b-2d (Complete)**: `TelemetryQueue` — Reading and draining `telemetry_queue` table in `studio.redb`. `events.jsonl` is now legacy read-only.
- **S2b-2d.1 (Complete)**: Telemetry Consent Boundary Cleanup — Telemetry obtains consent exclusively through canonical `StudioPreferencesService`, while telemetry queue persistence remains owned by `TelemetryStore` / `TelemetryQueue` and `studio.redb`.
  ```text
  Telemetry
      ↓
  PreferencesService
      ↓
  consent decision

  Telemetry
      ↓
  TelemetryStore
      ↓
  telemetry_queue
  ```
  The two concerns are strictly separated: `consent decision ≠ telemetry persistence`.
- **S2b-2e (Deferred)**: `UpdateState` & `SyncState` — Auto-update and sync markers.
- **S3 (Deferred)**: Credential isolation and OS keychain integration.

## 14.5. Telemetry consent enforcement (S2c)

The Studio Persistence Audit (`docs/architecture/studio-persistence-audit.md`
§10) found that revoking analytics consent stopped **new** events from
queuing but did nothing about events **already** in `telemetry_queue` —
`drain_queue` had no consent check of its own, so a previously queued
event could still be uploaded after the user turned analytics off. This
section documents the fix.

**Consent controls both enqueue AND delivery — not just enqueue.**

```text
enqueue
  ↓
consent check (per event category)

AND

drain
  ↓
consent check (re-read fresh, per event, immediately before sending)
  ↓
network upload
```

**Every queued event now carries a category** —
`valori_studio_storage::telemetry::TelemetryCategory::{Analytics, Crash}`
— matching `StudioPreferences.telemetry_consent`'s two existing fields
exactly (no third category was added; none was found to exist anywhere in
the codebase — see the audit's evidence). This is what makes it possible
to revoke one category without touching the other: previously the queue
had no way to tell an analytics-consented row from a crash-consented one,
so revocation could only have been "clear everything" (wrong — it would
silently discard crash reports the user still wants) or "clear nothing"
(the original bug). Rows written before this field existed deserialize
with `category = Analytics` via `#[serde(default)]` — the historically
accurate value, since every event was in fact gated by the old
analytics-only check regardless of its name.

**A related, previously-unnoticed bug fixed in the same change:**
`studio_crashed` events were always routed through the same
`enqueue_telemetry_event` Tauri command as analytics events, and that
command checked *only* analytics consent — so a user with `crash: true,
analytics: false` had their crash reports silently dropped at enqueue
time, even though crash consent was explicitly on. `enqueue_telemetry_event`
now takes an explicit `category` parameter and checks
`consent_for_category` (the field matching that category), fixing this.

**Revocation invalidates existing queued events of that category —
eagerly, and defensively.**

1. **Eager**: `preferences_service.rs`'s `set_telemetry_consent_command`
   calls `discard_revoked_telemetry_categories` immediately after
   persisting the new consent — for each category that just turned off,
   `TelemetryQueue::discard_category` deletes every row of that category
   outright (the same "delete on success/never accumulate a `blocked=true`
   history" discipline `mark_delivered` already uses — see §"Durability
   assumptions"). Idempotent: discarding an empty or already-clean
   category is a no-op.
2. **Defensive** (the uploader boundary, not just the enqueue guard):
   `desktop/src-tauri/src/telemetry.rs`'s `drain_queue` re-reads
   `consent_for_category` **fresh, per event, immediately before**
   dispatching that event's HTTP request — never a cached value, never
   assumed from enqueue time. If consent for that event's category is off
   right now, the event is deleted (never sent, never retried) instead of
   uploaded.

**The ordering guarantee, stated precisely (not overclaimed):**
`studio.redb` (redb) transactions are atomic and serialized. Once a
consent-revocation write transaction commits, every subsequent read
transaction — including the per-event check inside `drain_queue`'s loop —
observes the new value; there is no way to read a stale or torn value
after that commit. The one thing this cannot prevent is an HTTP request
that was **already dispatched** (physically in flight, not cancellable) at
the exact moment revocation commits. Checking consent **per event inside
the drain loop**, rather than once for the whole batch, bounds that window
to "at most the one event whose request was already in flight," not the
whole batch. No new synchronization primitive was introduced — this is
built entirely on `StudioDatabase`'s existing transaction guarantees, per
the phase's own instruction to use the existing service/database
architecture rather than a second global state mechanism.

**Independent categories are preserved**, not collapsed: `TelemetryConsent`
is still exactly `{ analytics: bool, crash: bool }` — no third field, no
single combined toggle. `analytics: false, crash: true` still means
"crash reports may queue and upload; analytics may not," end to end,
including at the uploader boundary now, not just at enqueue.

## 15. Recovery UI

Automatic recovery (the overwhelming common outcome — see §10) never
blocks the normal UI from opening. `desktop/src-tauri` emits a
`studio-recovery` Tauri event once, synchronously, during `setup()` — but
since no window is guaranteed to be listening that early, the frontend
also queries `get_studio_recovery_status` (a Tauri command backed by
managed `RecoveryStatusDto` state) once on mount
(`AppShellGate.tsx`). A healthy launch shows nothing at all — no toast, no
delay.

| `RecoveryOutcome` | `RecoveryStatusDto.kind` | Frontend behavior |
|---|---|---|
| `Healthy` | `"healthy"` | Nothing — silent, the common case |
| `RestoredFromBackup` | `"restored_from_backup"` | Non-blocking toast: *"Studio recovered its local metadata database from a backup. Your project data was not modified."* |
| `FreshDatabaseCreated` | `"fresh_database_created"` | Non-blocking toast: *"Studio recreated its local metadata database. Your project data was not modified; some Studio preferences and recent activity may need to be set again."* |
| *(recovery itself failed — see §10's last paragraph)* | `"unavailable"` | Non-blocking toast: *"Studio's local settings database is unavailable for this session. Your project data was not affected."* |

Raw `redb`/IO error text is never sent to the frontend as the primary
message — `RecoveryOutcome::user_message()` supplies the non-technical
sentence; the technical detail (the error's `Display` text) lives only in
`studio-recovery.jsonl` and the `tracing` log, under `RecoveryLogEntry::reason`.

A genuinely blocking "dedicated recovery screen" (Restore backup / Start
fresh / Open recovery folder / Quit) is not built — by design, every
failure `open_with_recovery` can hit already resolves automatically to a
working database (backup or fresh); the only case that doesn't
(`"unavailable"`) is the pathological one where even creating an empty
`studio.redb` fails, which no amount of in-app "retry" UI can fix (it
needs the underlying disk/permissions problem solved outside the app).
That case still gets a visible notice rather than silent degradation, just
via the same toast mechanism, not a modal.

## 16. Logging

Recovery events log through the same `tracing` macros as everything else
in `desktop/src-tauri` (there is no separate `studio.log` file sink today —
see `docs/phases/phase-studio-DR-database-recovery.md`'s audit for why
this document doesn't claim one exists). Representative lines from
`crate::recovery`:

```text
WARN studio database open failed: database error: I/O error: invalid data
WARN studio database preserved at /…/studio.redb.corrupt-1735689000000
INFO studio database recovery: attempting backups
WARN backup generation 1 failed validation, skipping
INFO studio database recovered from backup generation 2
```

Never logged, in `tracing` output or in `studio-recovery.jsonl`:
preference values, telemetry payloads, project contents, or credentials —
only recovery mechanics (timestamps, states, paths, short error strings).

## 17. Concurrency and recovery ordering

Recovery runs synchronously inside `desktop/src-tauri`'s `setup()`,
*before* `app.manage(Arc<StudioDatabase>)` and therefore before any Tauri
command, the telemetry sender, session start, or any future update/sync
worker can observe the database — none of those services can race
recovery because none of them have a handle to race with yet:

```text
resolve data root
        ↓
initialize/recover Studio DB   (open_with_recovery)
        ↓
register Studio DB             (app.manage)
        ↓
start preferences/session/telemetry/update services
        ↓
start UI
```

Cross-process: a database already open and healthy in another handle
surfaces `DatabaseError::DatabaseAlreadyOpen` and is deliberately **not**
treated as corruption (§10, §"Recovery order" step 2) — recovering it
would be actively destructive to a database that is perfectly fine and in
active use. In practice `tauri-plugin-single-instance` already prevents a
second Valori Studio process from reaching this code path at all (§7);
the `DatabaseAlreadyOpen` check is the defense-in-depth line behind it,
not the primary mechanism.

## 18. Theme persistence (S2c)

The Studio Persistence Audit (`docs/architecture/studio-persistence-audit.md`
§4) found `ui/src/lib/theme.tsx` writing theme to **both**
`studio.redb` (via `setPreference`, when running in the desktop app) and
raw browser `localStorage`, unconditionally, on every toggle — two
sources of truth inside the same desktop app. This section documents the
fix.

**One authoritative store per environment, not one store overall** — the
codebase runs in two genuinely different environments and always has:

```text
Tauri (Valori Studio desktop):
  StudioPreferences → studio.redb   (sole authoritative store — no
                                      localStorage write, ever)

Browser (Valori Cloud web UI, or `ui/` run standalone via `npm run dev`):
  localStorage                       (unchanged — there is no studio.redb
                                      in this environment at all)
```

`ui/src/lib/theme.tsx` branches on the existing `nativeAvailable()` check
(the same native-bridge test every other Studio-vs-browser distinction in
this codebase already uses — see `native.ts`) rather than silently
maintaining both stores:

- **Read** (`loadTheme`): native → `getPreference<ThemePref>("theme")`
  only; non-native → `localStorage.getItem("valori-theme")` only.
- **Write** (`setTheme`): native → `setPreference("theme", p)` only —
  **no `localStorage.setItem` call in the desktop app anymore**;
  non-native → `localStorage.setItem("valori-theme", p)` only, exactly as
  before.

**Migration compatibility.** An existing desktop installation may have a
theme value only in `localStorage` — from before `studio.redb` existed,
or from before this fix (every prior build dual-wrote, so any user who
toggled theme even once already has the correct value in `studio.redb`
too; only a user who set a theme once, long enough ago, and never
toggled again could be in this state). `loadTheme` handles this with a
one-time, idempotent, non-destructive backfill: if `getPreference("theme")`
returns nothing, it reads the legacy `localStorage` key (never deleting
it) and — if present — uses that value **and** writes it into
`studio.redb` via the normal `setPreference` path. No separate "have I
migrated" flag exists or is needed: the trigger condition ("`studio.redb`
has no theme value") can only be true once per installation, since the
backfill itself makes it false immediately afterward. This mirrors the
non-destructive discipline `crate::migration` (S2a) already established
for `preferences.json`/`events.jsonl` — read-only against the legacy
source, one-time, safe to call on every launch.

**No `studio.redb` schema change was needed.** `preferences.theme:
Option<String>` already existed (S1); this phase only changed which
code paths read and write it, not the stored shape.

**Recovery compatibility**: unaffected. A recovered `studio.redb` (backup
restore or fresh fallback — see §10) is opened the same way any other
`studio.redb` is; `getPreference("theme")` behaves identically whether the
database is the original, a restored backup, or a fresh one (which
returns `None`, resolved by `loadTheme`'s existing `stored ?? "dark"`
fallback — the same "safe defaults" behavior already documented in §10's
rebuild-classification table for `preferences`).

**No frontend test framework exists in this repository** (`ui/` has no
Jest/Vitest/`*.test.ts*` files at all — confirmed by search during this
phase). `theme.tsx`'s behavior is verified two ways instead, consistent
with how every other `native.ts`-backed feature in this codebase has been
verified since S1: (1) the `theme` field's `studio.redb` round-trip,
default-when-absent, and reopen-persistence behavior are already
exhaustively covered by `crates/valori-studio-storage`'s and
`desktop/src-tauri`'s existing `preferences` test suites (the storage
layer `theme.tsx` calls through), and (2) a real desktop application
launch against a disposable `$VALORI_HOME` (see the phase doc) — proving
the actual production code path (`StudioPreferences.theme` via the same
crate the app links) round-trips correctly and persists across a process
restart. Introducing a new JS test framework for one component was judged
disproportionate to this phase's scope.

## 19. Installation identity (Studio Installation Identity phase)

See `docs/reviews/installation-id-audit.md` for the audit that found this
gap, and `docs/phases/phase-studio-installation-identity.md` for the fix.

**The invariant:** every Valori Studio installation receives one stable
anonymous `InstallationId` on first startup. Its existence is independent
of telemetry consent. Telemetry and sessions consume this identity; they
do not create it.

**Canonical storage:** `studio.redb`'s `preferences.installation_id:
Option<InstallationId>` — the same field that has existed since S1. No new
table, file, or persistence location was introduced.

**Canonical generation:** exactly one Rust function may mint a fresh
`InstallationId` and persist it —
`StudioPreferencesService::get_or_init_installation_id` (idempotent:
returns the existing value if one is already stored, generates and
persists atomically via a single `redb` update transaction only if
absent). It is called **unconditionally** in `lib.rs`'s `setup()`,
immediately after `StudioPreferencesService` is registered as managed
state and before session start — independent of telemetry consent, Cloud
login, or project state.

Before this phase, three independent implementations of this same
get-or-init pattern existed (`preferences_service.rs`, a private helper in
`telemetry.rs`, and `native.ts`'s browser fallback), and the only call
site that ever *invoked* generation was gated behind telemetry consent
(`telemetry.ts::send()`) — so a user who never opted into telemetry never
got an installation id at all. `telemetry.rs` now reads the value through
`StudioPreferencesService` instead of maintaining its own copy;
`native.ts`'s browser-only fallback remains, since it serves a genuinely
separate identity for the non-Tauri web build (never a second desktop
persistence path — see the architecture test below).

**Session linkage:** `lib.rs`'s `setup()` reads the now-guaranteed
`installation_id` and passes it into `SessionService::start_session` for
every new session. Existing sessions recorded before this phase (with
`installation_id: None`, from installs that had telemetry off) are **not**
rewritten — they remain accurate historical records of what the app
actually knew at the time. Only new sessions carry the guaranteed value.

**Complete-loss behavior:** if `studio.redb` and all recoverable backups
are irrecoverably lost, `open_with_recovery` (see §10) falls back to a
fresh, empty `studio.redb` — the DR-phase behavior this section does not
change. `get_or_init_installation_id` finds no existing value in that
fresh database and mints a new one. **A new installation identity after
total metadata loss is expected and acceptable** — there is no
out-of-database source (`localStorage`, telemetry, logs, session history)
this value is ever recovered from, by design (it is anonymous local
metadata, not a credential or a durable audit artifact).

**Architecture enforcement:**
`desktop/src-tauri/tests/installation_id_architecture.rs` mechanically
checks that exactly one Rust generation site exists, that no second
desktop persistence file/table is referenced, and that the Tauri branch of
`native.ts::getInstallationId()` never touches `localStorage`.

## 20. Session retention (S5)

Confirmed by `docs/reviews/studio-persistence-consolidation-audit.md` as
the one P0 finding: the `sessions` table (§1's table, `crate::session`)
had no pruning of any kind — one row per app launch, forever. `recent()`
truncates a **read** result; it never deleted anything. This section
documents the fix.

**The policy** (`crate::session::SessionRetentionPolicy`, defaults
`max_completed_sessions: 100`, `completed_retention_days: 90`,
`crashed_retention_days: 180`):

- An **open** session, and the **current** session specifically
  (regardless of its own state), are never pruned. Belt-and-suspenders —
  the intended call site runs pruning before the current session's row
  even exists, but `SessionStore::prune` also takes `current_session_id`
  explicitly and excludes it unconditionally, rather than relying on
  ordering alone.
- A **completed** session (`ended_at.is_some() && !crashed`) is deleted
  only when **both** conditions hold: it is not among the newest
  `max_completed_sessions` by `started_at`, **and** it is older than
  `completed_retention_days`. A row-count overflow alone never deletes
  anything still within the age window — an "excess" but recent session
  survives until it ages out on a later prune.
- A **crashed** session is deleted once older than
  `crashed_retention_days`. No count cap applies to crashed sessions —
  crash history is comparatively rare and higher-signal, kept longer, and
  bounded by age alone.

**Storage API**: `SessionStore::prune(current_session_id, &policy, now) ->
StudioStorageResult<SessionPruneStats>` — `crate::session`, deterministic
(caller-supplied `now`, no wall-clock read inside the crate), and touches
only the `sessions` table (no other table is read or written). Deletion
order among eligible rows is oldest-`started_at`-first. `SessionPruneStats`
reports `scanned`/`deleted`/`retained` plus `protected_active`/
`protected_current`/`protected_within_retention`, for logging.

**Desktop integration** (`desktop/src-tauri/src/lib.rs`'s `setup()`):
ordered DB open → installation identity → **crash reconciliation → prune
→ start current session** — reconciliation must see accurate state before
pruning runs, and pruning must not touch a session row that doesn't exist
yet. **Never fatal**: a pruning failure is logged
(`tracing::warn!`) and startup continues — it must never trigger
`studio.redb` recovery (pruning is a plain table operation on an already-
open, already-recovered database; a failure here says nothing about the
database's health) and never panic. Enforced by
`desktop/src-tauri/tests/session_retention_architecture.rs`.

**Backward compatibility**: purely additive — no schema version bump
(`CURRENT_SCHEMA_VERSION` unchanged), no new table, existing session rows
read and prune identically to newly-written ones. A `studio.redb` with
thousands of historical sessions opens and prunes correctly (bounded
memory: only the `sessions` table is scanned, not the whole database).

**Recovery interaction**: unchanged — `sessions` was already classified
"trivially rebuildable-to-empty" in the DR rebuild-classification table
(§10 above); pruning doesn't change that classification, it just keeps
the table from growing past what that classification already assumed
was disposable.

See `docs/phases/phase-studio-S5-session-retention.md` for full validation.

## 21. Desktop filesystem consolidation (S6)

The authoritative, verified answer to "where does every desktop file
live" — see `docs/reviews/studio-filesystem-audit.md` for the read-only
audit this is based on, and
`docs/phases/phase-studio-S6-filesystem-management.md` for the
implementation. **This diagram supersedes every earlier, partly
aspirational one in this document and elsewhere** — directories marked
`(lazy)` do not exist until their owner's first real write; none is
created merely for appearing here.

```text
$VALORI_HOME/                          ($VALORI_HOME, else ~/.valori — one root, no other)
│
├── studio.redb                        Studio-owned, AUTHORITATIVE, always present after first launch
├── studio-recovery.jsonl              Studio-owned, sibling of studio.redb (deliberately — §10 above)
├── backups/                           Studio-owned, rolling (BACKUP_GENERATIONS = 3), (lazy: first backup)
│
├── projects/                          PROJECT-OWNED — Studio resolves the path, never the contents
│   └── <name>/                        keyed by name (predates ProjectId), not StudioPaths' concern past here
│       ├── project.json                daemon-owned, atomic write-then-fsync-then-rename (S6)
│       ├── events.log                   node/kernel/storage-owned — StudioPaths/FileSystemService never
│       ├── snapshot.val                 open, read, or write these; no method in either type names them
│       └── (cluster mode: events-nN.log, current-nN.snap, raft-nN.redb, node-nN.log)
│
├── models/ (lazy)                     Node-owned (valori-models, wired into valori-node) — NOT Studio-owned;
│   └── <sanitized-model-id>/           StudioPaths resolves the default layout for display/reference only
│
├── logs/ (lazy)                        [S7] real content — tracing-appender file sink, daily rotation,
│                                        alongside (not instead of) stdout; 7-day cleanup at every startup
├── crashes/ (lazy)                     [S7] real content — best-effort archival copy of each crash marker
│                                        once check_and_clear_crash_marker reads it, 30-day cleanup; the
│                                        LIVE panic-hook marker itself still uses Tauri's app_config_dir(),
│                                        a permanent, documented exception (see below) — archival is additive
├── cache/ (lazy)                       explicitly disposable — deleting its contents must never break
│                                        anything; no real cache producer exists yet (S6 adds the
│                                        infrastructure — FileSystemService::clear_cache — not a
│                                        speculative cache implementation)
├── downloads/ (lazy, currently unused)  staging area for the stage → verify → atomic-move contract;
│                                        no current writer (valori-models installs directly into models/)
└── temp/ (lazy)                       Studio-owned scratch space; cleaned at every startup
                                         (FileSystemService::cleanup_stale_temp_files, 24h age cutoff,
                                         never fatal, wired into lib.rs's setup() — see the S6 phase doc)
```

**Legacy locations, deliberately not moved onto this diagram** (see the
filesystem audit's §4 for the full reasoning, not repeated here): the
panic-hook crash marker (Tauri `app_config_dir()/crashes/crash_marker.json`)
and the two S2a migration sources (`app_config_dir()/preferences.json`,
`app_config_dir()/events.jsonl`, both permanently read-only).

**Canonical abstraction**:
```text
StudioPaths (valori_studio_storage::path)   — "where": pure path resolution,
        │                                       no filesystem access at all
        ▼
FileSystemService (desktop/src-tauri)        — "how": create_dir, atomic_write/
                                                atomic_replace, read, remove, rename,
                                                copy, exists, clear_cache,
                                                cleanup_stale_temp_files, cleanup_old_logs,
                                                cleanup_old_crash_archives [S7], safe_join
                                                (path-traversal rejection)
```

**Directory lifecycle classification**:

| Directory | Always required | Lazily created | Disposable | Recoverable | Owner |
|---|---|---|---|---|---|
| `studio.redb` | Yes | — | No | Yes (DR pipeline) | Studio (application metadata) |
| `studio-recovery.jsonl` | Yes | — | No | N/A (it *is* the recovery record) | Studio |
| `backups/` | No | Yes | No (durable-by-design) | N/A | Studio |
| `projects/<name>/` | No | Yes (daemon-created) | **No — user data** | Not via `studio.redb`'s DR system | Project |
| `models/` | No | Yes | Partially (SHA-256-reverifiable, re-downloadable) | Yes (re-download) | Node |
| `logs/` | No | Yes (unused today) | Yes | N/A | Studio (machine-owned) |
| `crashes/` | No | Yes (unused today) | Yes (one-shot markers) | N/A | Studio |
| `cache/` | No | Yes | **Yes — must satisfy "delete it, app still works"** | N/A | Studio (machine-owned) |
| `downloads/` | No | Yes (unused today) | Yes (staging only) | N/A | Studio/Node (machine-owned) |
| `temp/` | No | Yes | Yes (24h-cleaned at startup) | N/A | Studio (machine-owned) |

**Compile-time enforcement**: `valori-studio-storage` cannot depend on
`valori-kernel`/`valori-storage`/`valori-node`/`valori-daemon` (already
enforced by `dependency_direction.rs`'s `SEALED_CRATES`, reconfirmed by
S6); `desktop/src-tauri` carries the same restriction (new,
`filesystem_architecture.rs`); the browser-side UI can never import
`@tauri-apps/plugin-fs`, and Cloud's client surface can never import
Node's `fs` module — both enforced by source-scanning tests in
`desktop/src-tauri/tests/filesystem_architecture.rs` (5 tests total). The
sacred boundary — Studio housekeeping (recovery, cache clear, temp
cleanup, atomic metadata writes) never touches a sibling project
directory — is proven with real production types against a real project
fixture in `filesystem_service.rs`'s own test module, not just asserted.

See `docs/phases/phase-studio-S6-filesystem-management.md` for full
validation, including a real desktop smoke test's 16-step verification.

## 22. Persistence boundary cleanup (S7)

Closes S6's six follow-ups. Full detail in
`docs/phases/phase-studio-S7-persistence-boundary.md`; summary:

- **TypeScript `$VALORI_HOME` unified**: `ui/src/lib/server/valori-home.ts`'s
  `getValoriHome()` is now the one TS-side copy, replacing three
  independent (agreeing) copies and fixing two that silently ignored a
  `VALORI_HOME` override.
- **`modelDir` decided and wired**: overrides only model artifact
  installation (`ModelManager::new_with_models_dir`,
  `VALORI_MODELS_DIR`), independent of `workspaceDir` — matching the
  Settings UI's two-separate-pickers design. `models_dir()`'s doc comment
  in `path.rs` should be read alongside this: `StudioPaths` still only
  resolves the *default* layout; the live override travels through the
  daemon's own environment, same pattern as `workspaceDir`/`VALORI_HOME`.
- **`metadata.redb` decided**: neither wired nor deleted — its
  `Project`/`Collection` tables are real, tested, deliberately-paused M3
  infrastructure (`docs/phases/phase-M0-M2-platform-contracts.md`).
  Dormancy is now enforced by
  `dependency_direction.rs`'s `metadata_db_open_stays_out_of_production_binaries` —
  reactivating it in a real binary requires updating that test, not an
  accident.
- **`logs/`/`crashes/` given real content**: see §21's diagram, updated
  in place — both directories moved from "reserved, unused" to "real
  writer, bounded retention" in this phase; the diagram is not
  duplicated here.
- **Deprecated persistence removed**: `tauri-plugin-store` (dependency +
  registration + capability), and `valori:notifs` migrated off
  `localStorage` onto `studio.redb`'s `preferences.notification_prefs`
  on desktop (web unchanged). Legacy `preferences.json`/`events.jsonl`
  deletion was evaluated and explicitly declined — no tested retention
  policy exists for them; S6's "never delete user data automatically"
  rule still applies.
- **Final persistence-boundary architecture test**:
  `desktop/src-tauri/tests/persistence_boundary_architecture.rs` (8
  tests) — the one consolidated answer to the five accidental-regression
  patterns named in the S7 task (UI→localStorage-for-desktop-state,
  UI→filesystem, random-module→`~/.valori`, Studio→project-internals,
  new-database-elsewhere), each enforced via an explicit, documented
  allowlist rather than a vague convention.

See `docs/phases/phase-studio-S7-persistence-boundary.md` for full
validation.
