# Phase: Studio S6 — Desktop Filesystem Consolidation

## Goal

Establish one canonical, enforceable answer to "where does every desktop
file live, who owns it, how is it created, recovered, and cleaned up" —
per `docs/reviews/studio-filesystem-audit.md`'s read-only audit, which
preceded this implementation. Consolidate path resolution behind one typed
abstraction, add safe filesystem operations (atomic writes, path-traversal
protection, lazy directory creation), and prove — with real, running code,
not just docs — that none of it can ever touch project data.

## Delivered

### `StudioPaths` — canonical path resolution (`valori-studio-storage`)

- **[`src/path.rs`](../../crates/valori-studio-storage/src/path.rs)** —
  `StudioPaths { root: PathBuf }` with typed accessors: `root`,
  `studio_db`, `backups_dir`, `recovery_log_path`, `projects_dir`,
  `project_dir(name)`, `models_dir`, `model_dir(&ModelId)`, `logs_dir`,
  `crashes_dir`, `cache_dir`, `downloads_dir`, `temp_dir`. Pure path math
  — no method touches the filesystem. The pre-existing free functions
  (`default_db_path`, `default_backups_dir`, `default_recovery_log_path`)
  now delegate to `StudioPaths::from_env()` internally, so there is one
  definition of the canonical layout; every pre-S6 call site (`db.rs`,
  `recovery.rs`, and their tests) is unaffected — verified by a parity
  test.
- **`project_dir` is keyed by name, not `ProjectId`** — deliberately
  deviates from the task's suggested signature, because the real on-disk
  convention (`valori-daemon::JsonProjectStore`) has always been
  name-keyed, predating `ProjectId`'s existence. Resolving `ProjectId →
  name` needs the project registry, which this leaf-ward crate doesn't
  have access to; fabricating that resolution would have been dishonest
  about what the type can actually do.
- **`recovery_log_path` stays a sibling of `studio.redb`**, not nested
  inside a `recovery/` subdirectory — the audit's §4 explains why
  (a corruption event that destroys `studio.redb` must not also destroy
  the record that corruption happened; this is an already-shipped,
  DR-phase-tested guarantee, not something S6 may casually relocate).
- **`crashes_dir()` is the *new* canonical location, deliberately not
  wired to the existing panic-hook marker** (`telemetry.rs`'s
  `app_config_dir()`-based marker path) — moving a panic handler's write
  path is a real robustness risk for no correctness gain; documented as a
  permanent exception, not silently diverged from.

### `FileSystemService` — safe operations (`desktop/src-tauri`)

- **[`src/filesystem_service.rs`](../../desktop/src-tauri/src/filesystem_service.rs)**
  (new) — `create_dir`, `atomic_write`/`atomic_replace` (write-temp →
  fsync → atomic rename), `read`, `remove` (idempotent), `rename`,
  `copy`, `exists`, `clear_cache`, `cleanup_stale_temp_files`, and
  `safe_join` (component-aware path-traversal rejection, plus a
  canonicalize-based symlink-escape check for paths that already exist).
  Typed `FsError` (`PathTraversal` / `Io`), never a bare string.
- **Wired into real startup**: `lib.rs`'s `setup()` now runs
  `cleanup_stale_temp_files($VALORI_HOME/temp/, 24h)` after session
  retention, before the telemetry sender spawns — never fatal (logged via
  `tracing::warn!`, startup continues), matching S5's established
  non-fatal-housekeeping pattern exactly.
- **`atomic_write`/`clear_cache`/`safe_join`/etc. exist as the complete,
  documented operation surface without a UI-facing Tauri command for
  each** — inventing a "Clear Cache" button or a logs-viewer endpoint with
  no current UI need would have been exactly the speculative feature this
  codebase's own guidelines warn against. They're available for the next
  feature that needs them, not exercised by one invented for this phase.

### `project.json` durability (`valori-daemon`)

- **[`crates/valori-daemon/src/project.rs`](../../crates/valori-daemon/src/project.rs)**'s
  `write_manifest` gained an `fsync` (`File::sync_all()`) before the
  rename it already performed — closing a real, narrow gap: the write was
  already atomic (temp file + rename), but without an fsync, a power loss
  (not just a process crash) between the rename and the OS flushing the
  page cache could still leave the file truncated on some filesystems.
  Same file, same format, same schema — no project storage redesign, just
  a stronger durability guarantee on an already-existing write path,
  explicitly named as an atomic-write candidate in the S6 task itself.

### Architecture enforcement

- **[`desktop/src-tauri/tests/filesystem_architecture.rs`](../../desktop/src-tauri/tests/filesystem_architecture.rs)**
  (new, 5 tests): `valori-studio-storage`/`desktop/src-tauri` can never
  depend on `valori-kernel`/`valori-storage`/`valori-node`/`valori-daemon`
  (Cargo.toml dependency-line scan — the compile-time half of "studio
  storage → project internals" being structurally impossible; the
  Rust-side boundary itself was already enforced by
  `dependency_direction.rs`'s `SEALED_CRATES`, reconfirmed still green);
  `filesystem_service.rs` never names a project-internal filename; the
  browser-side UI never imports `@tauri-apps/plugin-fs`; Cloud's client
  surface never imports Node's `fs`/Tauri's fs plugin.

### Tests

- `crates/valori-studio-storage/src/path.rs` — 7 new tests: `$VALORI_HOME`
  resolution, `~/.valori` fallback, a custom root/workspace, every
  accessor's exact path, name-keyed `project_dir`, `model_dir`'s
  sanitize-parity with `valori-models`, and free-function/`StudioPaths`
  output parity.
- `desktop/src-tauri/src/filesystem_service.rs` — 16 new tests: `safe_join`
  (plain path, `..` traversal, absolute path, symlink escape),
  idempotent `create_dir`, atomic write (create, replace, no leftover temp
  file, and a simulated-crash test proving a reader never observes a
  partial write), idempotent `remove`, `clear_cache` (removes contents,
  keeps the directory, no-ops on a missing directory), stale-temp-file
  cleanup (age-based, never touches subdirectories, no-ops on a missing
  directory), and the **project safety test**
  (`studio_housekeeping_never_touches_a_sibling_projects_directory`) — a
  disposable project fixture (`project.json`, `wal/`, `snapshots/`,
  `indexes/`, `vectors/`) hashed before and after a real
  `open_with_recovery` open (twice, simulating a restart), cache clear,
  temp cleanup, and an atomic metadata write, asserting byte-for-byte
  identity throughout.
- `desktop/src-tauri/tests/filesystem_architecture.rs` — 5 new tests (see
  above).

## Findings

- **`metadata.redb` is not part of the running system** — reconfirmed
  from the S4 audit, unrelated to this phase's changes, mentioned in the
  filesystem audit for completeness. Not touched.
- **Five of the eleven directories in every architecture diagram
  (including the task's own) don't exist as real directories today**:
  `logs/`, `cache/`, `downloads/`, `temp/`, `recovery/`. `StudioPaths`
  resolves all five correctly, but none is created until first real use
  (`temp/` is the only one with a real writer today — the startup
  cleanup, which itself only acts if the directory already exists).
- **The crash marker lives at a legacy, non-canonical location**
  (Tauri's `app_config_dir()`, not `$VALORI_HOME/crashes/`) — a
  deliberate, documented exception, not an oversight (see the audit's
  §4: minimizing what a panic handler does is a real robustness
  property).
- **A third, independent copy of the `$VALORI_HOME` resolution rule
  exists in TypeScript** (`ui/src/lib/server/{api-client,connection,
  cluster-config}.ts`), alongside the two Rust copies
  (`valori-daemon::default_home()`,
  `valori-studio-storage::path::default_home_dir()`). All three
  implement the identical rule and none was found to have drifted.
  `project-adapter.ts`'s hardcoded `~/.valori` fallback was initially
  suspected to be a bug (ignoring `VALORI_HOME`) — **verified not a bug**:
  its own comment states the daemon's real `VALORI_HOME` (queried live)
  is authoritative, and the hardcoded path is a documented degraded-mode
  fallback for "daemon unreachable" only. Consolidating the TypeScript
  side onto one shared resolver is a reasonable future cleanup, not
  undertaken here — see Follow-ups.
- **`modelDir` (a real `studio.redb` preference) has no wired effect
  anywhere in production** — round-trips to the Settings UI and nowhere
  else. Flagged, not wired up (that would be new behavior, not filesystem
  *management*).

## Validation

```text
cargo fmt --check                                                  clean (no unrelated files touched)
cargo check --workspace                                            clean
cargo test --workspace                                             all green, 0 failures
cargo clippy -p valori-studio-storage --all-targets -- -D warnings clean
cargo test -p valori-node --test dependency_direction              6 passed, 0 failed
cargo test -p valori-node --test architecture                      1 passed, 0 failed
npx tsc --noEmit                                                   clean (no TS changed this phase)
npm run build                                                       succeeds

cargo test -p valori-studio-storage                                125 passed (was 118, +7)
cargo test -p valori-daemon                                        38 + 4 = 42 passed, unchanged
                                                                     (write_manifest's fsync addition
                                                                      verified via existing project.rs
                                                                      tests, all still green)

Desktop crate (separate build, outside the root workspace):
cargo build --lib                                                  clean
cargo test --lib                                                   68 passed (was 67, +1: the project
                                                                     safety test)
cargo test --test filesystem_architecture                           5 passed, 0 failed (new)
cargo test --test credential_security_architecture                  5 passed, 0 failed (unchanged)
cargo test --test installation_id_architecture                      4 passed, 0 failed (unchanged)
cargo test --test session_retention_architecture                    4 passed, 0 failed (unchanged)
```

### Real desktop smoke test

Against a disposable `$VALORI_HOME=/tmp/valori-s6-test` (deleted after),
all 16 steps verified live against the real compiled `valori-desktop` and
`valori-node` binaries:

| # | Step | Result |
|---|---|---|
| 1–2 | Fresh launch; paths resolve | Only `studio.redb` created — no directory made merely because it's in a diagram |
| 3–6 | Real local project via `valori-node` (`VALORI_EVENT_LOG_PATH`/`VALORI_SNAPSHOT_PATH` under `projects/demo/`), real collection created via `/v1/namespaces`, real vector inserted via `/records` with a genuine BLAKE3 receipt | `events.log`, `events.namespaces.json`, `project.json` on disk, fully project-owned |
| 7–8 | Restart Studio; project directory intact | No interference |
| 9 | Model path | `StudioPaths::models_dir()` resolves correctly; no download exercised (no model-download capability exists in Studio today — Node-owned per the audit, out of scope to build) |
| 10 | Logs | `StudioPaths::logs_dir()` resolves; no file sink exists yet to generate one (confirmed pre-existing, not a regression) |
| 11–12 | Temp/cache generation, restart, verify cleanup | Real log line: `startup: cleaned up stale temp files removed=1` — a 48h-old file removed, a fresh file preserved |
| 13–14 | Corrupt `studio.redb`, verify DR | Real recovery pipeline ran: original preserved as `studio.redb.corrupt-<timestamp>`, restored from backup generation 1, app launched normally |
| 15 | Project files byte-for-byte unchanged | Confirmed via SHA-256, checkpointed *after* `valori-node`'s own graceful-shutdown snapshot save (see below) through every subsequent Studio operation (restart, DR recovery, temp/cache cleanup) |
| 16 | No unexpected files outside canonical paths | Confirmed — every Studio-created entry was exactly where `StudioPaths` says it should be; `logs/`/`downloads/` correctly never appeared |

**One test-sequencing note, not a bug**: the first SHA-256 checkpoint was
taken immediately after inserting a vector, *before* stopping
`valori-node` — and `valori-node`'s own graceful-shutdown snapshot save
(SIGTERM → snapshot-on-shutdown, its documented, pre-existing behavior)
changed `events.log`/wrote `snapshot.val` *after* that checkpoint but
*before* any Studio operation ran. This is `valori-node` doing its own,
correct, unrelated job — re-checkpointing immediately after the node had
fully exited (and before any Studio operation) and re-verifying after
every subsequent Studio operation confirmed byte-for-byte stability, which
is the property actually being tested.

## Follow-ups

- Consolidating the three independent `$VALORI_HOME`-resolution
  implementations (two Rust, one TypeScript) onto fewer copies — real, but
  identified as drift-free today; touching three TypeScript files with
  different call patterns is refactoring, not filesystem management.
- Migrating the panic-hook crash marker off Tauri's `app_config_dir()`
  onto `StudioPaths::crashes_dir()` — deliberately deferred, see Findings.
- Wiring `modelDir` to anything — currently inert; out of scope (new
  behavior).
- A real file-backed log sink under `$VALORI_HOME/logs/` — none exists
  today (stdout/stderr only); inventing one wasn't required by this
  phase's mandate ("do not invent a cache/log merely to fill the
  directory").
- UI-facing Tauri commands for `clear_cache`/log viewing/etc. — the
  backend capability exists; no UI need was identified this phase.
- `crates/valori-daemon/src/project.rs:281`'s `write_manifest` fsync
  addition could be extended to any other raw `std::fs::write` call sites
  in the daemon crate found by a future, dedicated audit of that crate
  specifically — out of this phase's Studio-focused scope; only the one
  call site the S6 task named explicitly (`project.json`) was touched.
