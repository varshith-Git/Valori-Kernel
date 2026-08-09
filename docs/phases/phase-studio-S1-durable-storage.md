# Phase Studio S1 — Durable Studio storage with redb

**Branch:** `feat/node-object-store-durability-and-metrics`
**Depends on:** [`docs/architecture/studio-storage-audit.md`](../architecture/studio-storage-audit.md) (the read-only audit this phase implements)
**Status:** Complete — storage foundation only. **Not wired into `desktop/src-tauri`.** No existing Studio persistence touched.

> Naming note: this is **Studio track S1**, unrelated to the node/sharding
> track's `phase-S1-multi-raft-skeleton.md`. Filed as
> `phase-studio-S1-durable-storage.md` to avoid collision with that
> numbering.

---

## Goal

Establish a production-quality, lightweight, durable Studio-local metadata
database using `redb`, entirely separate from `~/.valori/metadata.redb` and
any Raft `redb` file, as a new independently-testable crate — without
migrating any existing Studio persistence (`preferences.json`,
`events.jsonl`, `localStorage`) or touching `desktop/src-tauri`.

## Delivered

### New crate: `crates/valori-studio-storage`

| File | Contents |
|---|---|
| `src/lib.rs` | Crate docs — ownership, what this is/is not, dependency direction |
| `src/db.rs` | `StudioDatabase` — `open`/`open_default`, schema creation, migration runner (`run_migrations`), typed store accessors |
| `src/schema.rs` (private) | Table definitions, `CURRENT_SCHEMA_VERSION = 1`, JSON-over-redb helpers (`get_json`/`put_json`/`delete_key`/`list_json`/`update_json`), schema-version read/write |
| `src/error.rs` | `StudioStorageError` (mirrors `valori_metadata::MetadataError`'s redb variants, plus `UnsupportedSchemaVersion`/`MigrationFailed`) |
| `src/path.rs` | `default_home_dir`/`default_db_path` — `$VALORI_HOME`/`~/.valori` resolution, a **deliberate duplicate** of `valori_daemon::default_home()` (documented why — this crate must stay leaf-ward) |
| `src/preferences.rs` | `StudioPreferences`, `TelemetryConsent`, `WindowState`, `PreferencesStore` |
| `src/project.rs` | `StudioProjectRecord`, `ProjectKind` (Local/Cloud), `ProjectRegistry` — identity-preserving upsert semantics |
| `src/project_cache.rs` | `StudioProjectCacheEntry`, `ProjectCacheStore` — disposable, independent of `project.rs` |
| `src/session.rs` | `StudioSessionRecord`, `SessionStore` — Studio application sessions, explicitly not Valori executions |
| `src/telemetry.rs` | `StudioTelemetryEvent`, `TelemetryQueue` — bounded (`MAX_QUEUE_LEN = 500`), enqueue/peek_batch/mark_delivered/increment_retry/count/prune_older_than |
| `src/sync.rs` | `StudioSyncState`, `SyncStateStore` |
| `src/update.rs` | `StudioUpdateState`, `UpdateStateStore` |
| `README.md` | Crate-level reference (module table, database layout, dependency position, invariants) |

**Tests:** 58 across 9 test binaries (`crates/valori-studio-storage/tests/`)
+ 2 unit tests in `src/path.rs` — see Validation.

### Workspace wiring

- `Cargo.toml`: added `crates/valori-studio-storage` to `[workspace] members`
  and to `[workspace.dependencies]` (not added to `default-members`,
  matching the existing convention for crates not yet consumed by a
  default-member — e.g. `valori-daemon`, `valori-models`).
- `crates/valori-node/tests/dependency_direction.rs`: added
  `("valori-studio-storage", &["valori-domain"])` to `SEALED_CRATES`,
  added `valori-studio-storage` to `OSS_PLATFORM_CORE` (the Cloud-concept
  ban now also applies to it), added
  `("valori-studio-storage", "valori-domain")` to `EXPECTED_EDGES`. No
  existing rule weakened — only new assertions added.

### Documentation

- [`docs/architecture/studio-storage.md`](../architecture/studio-storage.md)
  (new) — the crate's contract: ownership, filesystem location, schema,
  table ownership (authoritative vs. cached), serialization format, schema
  versioning/migration, concurrency model, durability assumptions,
  corruption behavior, backward compatibility, and what must never enter
  `studio.redb`.
- This phase doc.
- `docs/phases/README.md` — status table row added (see below).

### Explicitly not touched

`preferences.json`, `tauri-plugin-store` usage, `events.jsonl`,
`localStorage`, existing telemetry producer/sender (`desktop/src-tauri/src/telemetry.rs`),
the existing updater, `desktop/src-tauri` (no dependency added, no `.rs`
file touched), the daemon's project persistence/manifest format,
`~/.valori/metadata.redb` / `valori-metadata`, any Raft `redb` file /
`valori-consensus`, Valori Cloud.

## Findings

- **`redb::WriteTransaction::open_table` is create-if-absent, never
  destructive** (confirmed by reading `redb` 2.6.3's source, not assumed) —
  this is what makes `create_all_tables` safe to call on every `open()`,
  including the "pre-versioning-shaped database" backward-compat case
  covered by `opening_a_pre_versioning_shaped_database_backfills_version_without_data_loss`.
- **A subtle borrow-checker trap** in the read-modify-write helper
  (`schema::update_json`): matching directly on `t.get(key)?` inside a
  block whose result feeds an outer `let` fails to compile (E0597, "`t`
  does not live long enough") even though the borrow is logically fine —
  the `?` operator's temporary interacts badly with the block's drop order.
  Fixed by converting the `AccessGuard` to an owned `Vec<u8>` before the
  inner block ends, rather than deserializing while still borrowed. Left a
  comment-free fix (the pattern is a known `rustc` rough edge, not a design
  decision worth documenting in-line) but flagging here since a future
  contributor extending `schema.rs` will likely hit it again.
- **`redb::Database` does not implement `Debug`**, so `StudioDatabase`
  needed a hand-written `impl Debug` (non-exhaustive, no table contents)
  rather than `#[derive(Debug)]` — `#[derive]` would have failed to compile
  the moment a test tried to use `{:?}` on an error path.
- **No mechanical test yet enforces "no secrets in this crate's records"**
  (§12 of `studio-storage.md`) — `tests/projects.rs`'s
  `cloud_reference_never_carries_credentials_and_uses_string_org_ref` is a
  structural guard on one record type (searches the serialized JSON for a
  handful of secret-shaped field names), not a crate-wide static
  assertion. Documented as a S1 limitation, not silently assumed solved.

## Validation

```
cargo test -p valori-studio-storage
```

58 tests, 0 failed, 0 ignored, across:

| Test file | Count | Covers |
|---|---|---|
| `src/path.rs` (unit) | 2 | `$VALORI_HOME` override, `$HOME`/`.valori` fallback |
| `tests/database.rs` | 7 | fresh creation, parent-dir creation, reopen (single + 5-cycle), unsupported future schema version (refused, file untouched), pre-versioning fixture backfill (no data loss), corrupt file (refused, file untouched) |
| `tests/preferences.rs` | 6 | defaults, set/get, update, delete→defaults, reopen, old-shaped-JSON forward compatibility |
| `tests/projects.rs` | 10 | register/lookup, rename preserves id, path change preserves id, re-registration is idempotent and preserves favorite/registered_at/last_opened_at, restart preserves identity, favorite toggle, recent sort order, cloud reference (no secrets), unregister, rename-of-unknown → NotFound |
| `tests/project_cache.rs` | 4 | put/get, **clearing the cache does not affect the project registry**, delete, reopen |
| `tests/sessions.rs` | 7 | start/get, end (not crashed / crashed), ending unknown session → NotFound, open_sessions excludes ended, recent sort order, reopen |
| `tests/telemetry.rs` | 9 | enqueue/count, peek_batch ordering, batch limit, mark_delivered deletes (no lingering row), increment_retry, retry-on-unknown → NotFound, prune_older_than, **bounded queue evicts oldest at MAX_QUEUE_LEN**, reopen |
| `tests/sync_and_updates.rs` | 8 | sync write/read/update-from-fresh/delete/reopen; update-state defaults/write-read/reopen |
| `tests/concurrency.rs` | 5 | concurrent writers same table (no lost writes), concurrent writers different tables, concurrent readers see only committed values, panicking update closure does not partially apply, reopen after heavy concurrent write load |

```
cargo test -p valori-node --test dependency_direction --test architecture
```
7 tests, 0 failed — confirms the new crate's dependency graph is correctly
sealed, `valori-studio-storage` is in the acyclic graph, and no duplicate
source files were introduced.

```
cargo check --workspace
```
Clean — confirms adding the new member and workspace-dependency entry did
not break any other crate.

```
cargo fmt -p valori-studio-storage
cargo clippy -p valori-studio-storage --all-targets
```
Both clean (clippy: zero warnings under the workspace's lint config).

```
cargo test -p valori-kernel
```
Passes unmodified (11+ tests in the sampled run; this phase did not touch
`valori-kernel`) — sanity check only, not a claim about this phase's scope.

**Not run in this pass:** the full `cargo test -p valori-node` suite
(integration tests that spin up HTTP servers; large and unrelated to this
phase's only change to that crate, a test-file addition already exercised
directly above) and the full `cargo test --workspace`. This phase's code
changes are additive and confined to a new crate plus one test file in
`valori-node`; the targeted runs above cover every file this phase touched.

### Answering the required question

**Can an existing Valori Studio installation be upgraded to this version
without losing or invalidating existing preferences, telemetry, projects,
metadata, or other existing state?**

**Yes.** Evidence, not assumption: `desktop/src-tauri`'s `Cargo.toml` and
every `.rs` file under `desktop/` and `ui/` are unchanged by this phase (verify: `git diff --stat` shows only `crates/valori-studio-storage/**`,
`crates/valori-node/tests/dependency_direction.rs`, `Cargo.toml`, and
`docs/**` touched). `studio.redb` is a new file at a path
(`~/.valori/studio.redb`) no prior version of Valori Studio has ever
written to or read from, so its creation cannot collide with, shadow, or
invalidate `preferences.json`, `events.jsonl`, `localStorage`,
`metadata.redb`, or any project's `project.json`/`events.log`/snapshot.
`cargo check --workspace` confirms nothing else in the workspace was
broken by the addition.

## Follow-ups

- **S2 (per the phase brief): wire `desktop/src-tauri` to actually open
  `StudioDatabase`** (via `open_default()`) at startup, and — separately,
  incrementally — point existing preference/telemetry/session read-write
  call sites at it (dual-write transition period recommended, mirroring
  `crates/valori-daemon/src/migration/m001_project_registry.rs`'s
  "never deletes the source" discipline). Not started in S1 by design —
  see the phase brief's explicit scope rule.
- **Secrets store** — the plaintext `apiKey` exposure in `ui/`'s
  `localStorage` (documented in `studio-storage-audit.md` §11) is
  unchanged by this phase and remains a hard prerequisite before any
  credential-shaped value may ever be persisted anywhere in Studio,
  `studio.redb` included.
- **Corruption recovery** — no automatic backup/repair path exists yet
  (`studio-storage.md` §10); today a corrupt `studio.redb` surfaces a clear
  error and stops there. A future phase should decide the product behavior
  (move-aside-and-recreate vs. block startup) — that is a product decision,
  not a default this crate should impose.
- **Fault-injection test** — no test simulates a process kill mid-`fsync`;
  durability rests on redb's own guarantee (already trusted for the Raft
  log) plus a clean-shutdown reopen test, not an independent proof within
  this codebase (`studio-storage.md` §8, `studio-storage-audit.md` §13's
  pre-existing gap, still open).
- **Mechanical secrets guard** — currently one structural test on one
  record type; a crate-wide static assertion (e.g. a test that greps every
  `src/*.rs` struct definition for secret-shaped field names, mirroring
  `dependency_direction.rs`'s `cloud_only_concepts_are_not_defined_in_oss_platform_core`)
  would be a stronger guarantee and is a natural small addition for
  whichever phase starts storing more record types.
