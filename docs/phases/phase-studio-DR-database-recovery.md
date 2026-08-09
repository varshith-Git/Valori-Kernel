# Phase Studio DR — Studio database resilience & recovery

**Branch:** `feat/node-object-store-durability-and-metrics`
**Depends on:** `docs/phases/phase-studio-S2b-2c-sessions-runtime-migration.md` and the S2b-2d telemetry-queue migration
**Status:** Complete.

**The invariant this phase exists to guarantee:** `studio.redb` contains
recoverable Studio metadata. It must never be allowed to make Valori
Studio permanently unlaunchable, and corruption of `studio.redb` must
never delete or modify the user's actual Valori project data.

---

## 1. Audit (evidence-based, produced before any code was written)

**What exact errors occur when `studio.redb` is corrupt?**
`redb::Database::create` (what `StudioDatabase::open` calls) returns
`DatabaseError::Storage(StorageError::Corrupted(String))` for a file that
isn't a valid redb database at all. Confirmed by reading `redb` 2.6.3's
source (`src/error.rs`) and by the pre-existing S1 test
`corrupt_or_invalid_file_fails_clearly_without_being_recreated`. A future
schema version surfaces `StudioStorageError::UnsupportedSchemaVersion`
(this crate's own error, not redb's). A file locked by another handle
surfaces `DatabaseError::DatabaseAlreadyOpen` — a **different** failure
mode from corruption, load-bearing for this phase's design (see §4).

**Did startup fail before this phase?**
Yes, in two layers. `StudioDatabase::open` always returned `Err` for any
of the above — that part was correct and unchanged. But
`desktop/src-tauri`'s `init_studio_storage` (fixed in the prior review
pass, before this phase) already degraded gracefully rather than
panicking the whole app — so the *app* did not fail to launch, but
`studio.redb` itself, once corrupted, stayed corrupted forever: nothing
ever attempted to open a backup or recreate it, so Studio storage was
permanently disabled for that installation until a human manually deleted
the file. That gap is what this phase closes.

**Can a partially damaged database be opened?**
No — redb has no notion of "partially open"; either the file parses as a
valid redb database or `Database::create`/`Database::open` returns `Err`.
There is no partial-read fallback to design around.

**Does `redb` expose any integrity/recovery mechanism already used by the
project?** `redb::Builder::set_repair_callback` exists (a callback fired
during redb's own internal WAL-style repair on open, for recovering an
unclean shutdown) but is not called anywhere in this workspace today
(`valori-metadata`, `valori-consensus`), and it is not a "restore from an
external backup" mechanism — it can't help with a file that's corrupted
beyond redb's own internal recovery. This phase's backup/restore system
is implemented at the filesystem level, independent of it.

**Where should backups live?**
`$VALORI_HOME/backups/` — inside the existing canonical `$VALORI_HOME`
layout, as its own subdirectory, matching the project/models/logs/crashes/
cache/downloads siblings already documented for that directory.

**Which Studio state can be rebuilt? Which is disposable? Which must be
preserved?** See the evidence-based classification table in
`crates/valori-studio-storage/src/recovery.rs`'s module doc (reproduced in
`docs/architecture/studio-storage.md` §10) — summary: `preferences`
restore-from-backup-or-safe-defaults; `update_state`/`telemetry_queue`/
`sessions` are trivially rebuildable-to-empty or already disposable;
`sync_state` is Cloud-re-derivable; the `projects` registry is **not**
auto-rebuilt (no parser for the daemon's `project.json` exists in this
crate, and adding one would violate the dependency firewall); WAL/
snapshots/vectors/indexes are never touched, structurally (this crate has
no code path to them).

**Does any startup code assume `StudioDatabase::open()` can never fail?**
No — every existing Tauri command already goes through
`app.try_state::<Arc<StudioDatabase>>()` and fails that one command with a
clear message if absent (established in the prior review-fix pass). The
gap was narrower: `init_studio_storage` called the plain `StudioDatabase::open`
with no recovery attempt at all, so a corrupt file meant "Studio storage
permanently unavailable" rather than "recovered, or clearly and visibly
unavailable for one session."

---

## 2. Delivered

### `crates/valori-studio-storage/src/recovery.rs` (new)

- `open_with_recovery(db_path, backups_dir, recovery_log_path)` — the
  full order: try current → preserve original → try backup generations
  newest-first → fresh fallback. Never fails for a condition a fresh
  database can resolve.
- `RecoveryOutcome` (`Healthy` / `RestoredFromBackup` / `FreshDatabaseCreated`)
  and `RecoveryState` (`Healthy`/`RecoveryRequired`/`RestoringBackup`/
  `Rebuilding`/`Recovered`/`RecoveryFailed`) — the explicit states
  requested, reusing this crate's existing report-struct pattern
  (`MigrationReport`/`SkippedRecord` from S2a) rather than inventing a
  parallel error model.
- `RecoveryLogEntry` + append-only `studio-recovery.jsonl` (a **sibling**
  of `studio.redb`, so a corruption event that destroys `studio.redb`
  can't also destroy the record that corruption happened).
- `preserve_corrupt` — atomic `fs::rename` to
  `studio.redb.corrupt-<unix_ms>`, collision-guarded, never deletes.
- `validate_database_file` — read-existing-only (`Database::open`, never
  `::create`), checks schema version range + every table opens; never
  mutates the file it's validating.
- `restore_backup` — copy to temp, atomic `fs::rename` into place; never
  a half-written `studio.redb` at its live path.
- `create_backup` / `backup_before_migration` / `maybe_periodic_backup` —
  bounded rolling backups (`BACKUP_GENERATIONS = 3`), triggered only
  before a schema migration and at most once per 24h on a healthy open;
  never on a preference write, telemetry enqueue, or any other hot path.
- `has_preserved_corrupt_marker` + the crash-safety branch in
  `open_with_recovery` — makes recovery idempotent and resumable after a
  crash between "preserve" and "restore", by deriving state purely from
  the filesystem rather than a separate lock file.
- **`DatabaseAlreadyOpen` is explicitly never treated as corruption** — a
  locked-but-healthy database (another process, or a bug that opened two
  handles) returns a plain `Err`, not a recovery attempt. Treating a lock
  conflict as corruption would have been actively destructive.

### `crates/valori-studio-storage/src/db.rs`

`StudioDatabase::open_default_with_recovery()` — the recovery-aware
convenience wrapper at the default paths, alongside the existing plain
`open`/`open_default`.

### `crates/valori-studio-storage/src/path.rs`

`default_backups_dir()`, `default_recovery_log_path()`.

### `desktop/src-tauri/src/studio_storage.rs`

- `init_studio_storage_with_paths` now calls `open_with_recovery` instead
  of the plain `StudioDatabase::open`, and returns the `RecoveryOutcome`
  alongside the database and migration summary.
- `RecoveryStatusDto` — the JSON-serializable projection for the
  frontend, camelCase fields / snake_case `"kind"` tag (matching this
  codebase's established convention for internally-tagged enums crossing
  IPC — verified by a dedicated shape-pinning test, not assumed).
- `get_studio_recovery_status` Tauri command + a `studio-recovery` event
  emitted once during `setup()`.
- `init_studio_storage` still returns `Option<Arc<StudioDatabase>>` and
  is still never allowed to fail `setup()` — the pre-existing
  graceful-degradation fix is preserved and is now what backstops the one
  remaining failure mode (fresh-database creation itself failing).

### `desktop/src-tauri/src/lib.rs`

Registered `get_studio_recovery_status` in `generate_handler!` and its
import — no other change to the startup sequence (recovery happens
*inside* `init_studio_storage`, which was already called at the same
point).

### `ui/src/lib/native.ts` / `ui/src/components/layout/AppShellGate.tsx`

`StudioRecoveryStatus` discriminated union + `getStudioRecoveryStatus()`.
`AppShellGate` queries it once on mount and shows a non-blocking toast
(reusing the existing `toast()` helper — no new UI component) for any
non-healthy outcome; a healthy launch is silent.

### Tests

- `crates/valori-studio-storage/tests/recovery.rs` — 13 new tests (full
  list in §4 below).
- `desktop/src-tauri/src/studio_storage.rs` — 2 new unit tests (recovery
  wired through `init_studio_storage_with_paths`, plus the DTO
  wire-shape pin).

### Documentation

`docs/architecture/studio-storage.md` §10 rewritten (Corruption behavior
and recovery), new §15 (Recovery UI), §16 (Logging), §17 (Concurrency and
recovery ordering); §13's startup diagram updated to show
`open_with_recovery` in place of the plain `open`. This phase doc.
`docs/phases/README.md` and `CHANGELOG.md` updated.

### Explicitly not touched (per the stop condition)

Sync migration, credentials/keychain, analytics/marketplace/model/RAG/
LLM/GPU/workflow analytics — none of these were implemented, consistent
with the phase brief's explicit exclusion list.

---

## 3. Findings

- **`DatabaseAlreadyOpen` vs. corruption is a real distinction the initial
  design missed until review**: blindly treating every `StudioDatabase::open`
  failure as "corrupt, enter recovery" would have made a second process
  (or a same-process double-open bug) trigger destructive-looking
  behavior — preserving-aside and rewriting backups — against a database
  that was never actually broken. Fixed by matching on the specific
  `redb::DatabaseError::DatabaseAlreadyOpen` variant and propagating it as
  a plain error instead.
- **Crash-safety between "preserve" and "restore" needed a real design
  decision, not just a note**: the chosen solution (detect leftover
  `studio.redb.corrupt-*` markers and re-enter the same recovery path,
  rather than a separate resume path or an extra lock file) was verified
  by an explicit test (`resuming_after_a_crash_between_preserve_and_restore_is_safe`)
  that hand-constructs exactly that crash state rather than trying to
  literally kill the process mid-operation.
- **A synthetic "needs migration" test caught a real, correct interaction**:
  hand-writing `meta.schema_version = 0` to simulate an old database
  proved that (a) the pre-migration backup trigger fires *before* knowing
  whether migration will succeed (as specified), and (b) because
  `crate::db::MIGRATIONS` is currently empty, such a database correctly
  fails to open (`MigrationFailed`, "no migration path registered") — and
  the recovery path then correctly falls through to a fresh database,
  since the just-taken backup is itself version 0 and fails validation
  too. The first draft of this test asserted the wrong outcome
  (`Healthy`) before this chain was traced through; fixed before
  submission, not left as a known-wrong test.
- **No `studio.log` file sink exists in this codebase.** The phase brief
  references logging recovery events to "the normal operational log:
  studio.log" — audited and confirmed no such file sink currently exists;
  `desktop/src-tauri` logs via `tracing_subscriber::fmt()` to
  stdout/stderr only. Recovery events log through the same `tracing`
  macros as everything else (see `docs/architecture/studio-storage.md`
  §16) rather than inventing a new file-logging subsystem this phase
  wasn't asked to build.
- **No dedicated "recovery screen" UI was built**, by design, not
  omission: every failure `open_with_recovery` can hit already resolves
  automatically to a working database. The only unresolved case
  (`"unavailable"` — even fresh creation failed) gets a visible toast, not
  a modal, because no in-app UI can fix a disk-full/permissions problem
  anyway. Documented as a deliberate scope decision in
  `docs/architecture/studio-storage.md` §15, not silently left out.

---

## 4. Validation

```
cargo test -p valori-studio-storage
```
**101 tests, 0 failed** (88 pre-existing + 13 new in `tests/recovery.rs`):

| Test | Covers |
|---|---|
| `healthy_database_opens_normally` | Healthy path, no recovery log written |
| `fresh_install_with_nothing_on_disk_is_healthy_not_recovery` | Fresh install stays silent |
| `corrupt_database_with_valid_backup_restores_it_and_preserves_the_original` | Corrupt + backup → restored, original preserved, still healthy on a second open |
| `corrupt_database_with_no_backup_creates_fresh_database_and_stays_launchable` | Corrupt + no backup → fresh DB, app stays usable |
| `skips_corrupt_backup_generations_and_restores_the_first_valid_one` | Multiple backups: gen 1 & 2 corrupt, gen 3 valid → gen 3 restored |
| `all_backup_generations_corrupt_falls_back_to_fresh` | All 3 generations corrupt → fresh fallback |
| `takes_a_backup_before_a_database_that_needs_migration_is_opened` | Pre-migration backup trigger fires unconditionally; synthetic migration failure recovers correctly |
| `running_recovery_twice_does_not_destroy_or_duplicate_artifacts` | Idempotency |
| `resuming_after_a_crash_between_preserve_and_restore_is_safe` | Crash-safety: process killed mid-recovery, next launch resumes correctly |
| `a_database_locked_by_another_open_handle_is_never_treated_as_corrupt` | Cross-process/concurrency: `DatabaseAlreadyOpen` never triggers recovery |
| `project_data_is_never_touched_by_studio_database_recovery` | **The load-bearing test** — real WAL/snapshot/index/manifest files, byte-for-byte hash-verified unchanged, for both the backup-restore and fresh-fallback scenarios |
| `recovery_log_records_the_event_without_sensitive_payloads` | Recovery log written, no preference values leak into it |
| `no_backup_taken_when_database_is_missing_marks_original_as_none` | Edge case: leftover corrupt marker with no underlying file |

```
cd desktop/src-tauri && cargo test
```
**25 tests, 0 failed** (23 pre-existing + 2 new): recovery wired through
`init_studio_storage_with_paths` (healthy + corruption-recovers cases),
and `recovery_status_dto_serializes_to_the_shape_native_ts_expects` (pins
the exact JSON — camelCase fields, snake_case `"kind"` tag — against a
regression that TypeScript alone can't catch).

```
cargo check --workspace                                        clean
cargo test -p valori-node --test dependency_direction --test architecture   7/7
cargo fmt -p valori-studio-storage / cd desktop/src-tauri && cargo fmt      clean
cargo clippy -p valori-studio-storage --all-targets -- -D warnings          clean
cd desktop/src-tauri && cargo clippy --all-targets                          clean
cd ui && npx tsc --noEmit                                        clean, exit 0
```

### Real desktop application launch (disposable `$VALORI_HOME`, not the
developer's production data)

Built `cargo build --bin valori-desktop` and ran the actual binary four
times against `VALORI_HOME=/tmp/valori-dr-test-1` (never the real
`~/.valori`), each time inspecting `tracing` output and the resulting
filesystem state, then deleted the disposable directory afterward:

1. **Fresh/healthy**: `Studio database opened at /tmp/valori-dr-test-1/studio.redb` — no recovery log written.
2. **Corrupt, no backup**: seeded a real project tree (`projects/demo/{events.log,snapshot.val,project.json}`) with fixed content and recorded SHA-256 hashes, corrupted `studio.redb` with garbage bytes, relaunched. Log:
   `WARN studio database open failed and no backup was valid; created a fresh database. Original preserved at /tmp/valori-dr-test-1/studio.redb.corrupt-1786208884867`.
   **All three project files' hashes matched exactly, unchanged.**
3. **Corrupt, valid backup**: copied the (now fresh, healthy) `studio.redb` into `backups/studio.redb.1`, corrupted the live file again, relaunched. Log:
   `WARN studio database open failed; restored from backup generation 1. Original preserved at /tmp/valori-dr-test-1/studio.redb.corrupt-1786208912720`.
   **Project file hashes matched exactly, unchanged, again.**
4. **Healthy relaunch after recovery**: `Studio database opened at ...` — no warnings, confirming the recovered database is fully normal going forward.

`studio-recovery.jsonl` contained the full, correctly-ordered sequence of
states (`recovery_required` → `restoring_backup` → `rebuilding`/`recovered`
matching the scenario) for each run.

### Answering the required question

**If `studio.redb` becomes corrupted, can Valori Studio still launch,
recover from a valid backup, or create a fresh metadata database without
modifying the user's actual project data?**

**Yes — verified by both automated tests and a real desktop application
launch against a disposable `$VALORI_HOME`, not assumed.** In every
scenario exercised (corrupt with no backup, corrupt with a valid backup,
corrupt with a mix of valid and invalid backup generations, a crash
interrupting recovery itself), Valori Studio opened successfully, and a
representative project's WAL/snapshot/manifest files were confirmed
byte-for-byte unchanged by SHA-256/content hash both in the automated test
suite and in the live application run.

---

## 5. Follow-ups

- **`projects` registry auto-rebuild** — still not implemented, by design
  (see Findings/Audit); whichever future phase wires Studio to the
  daemon's live project list is the natural point to also repopulate a
  freshly-recovered registry, the same way first install already does.
- **A `studio.log` file sink** — doesn't exist; if one is ever added,
  route recovery's `tracing` calls through it for free (no `recovery.rs`
  change needed, it already uses the standard `tracing` macros).
- **Corrupted-file retention/cleanup** — `studio.redb.corrupt-*` files
  and old `backups/` generations are never automatically deleted (beyond
  the bounded 3-generation backup rotation); a future phase could add a
  "clean up recovery artifacts older than N days" maintenance task if
  disk usage becomes a concern in practice.
- **S2b-2e / S3** — deferred per the phase brief's own stop condition,
  unchanged by this phase.
