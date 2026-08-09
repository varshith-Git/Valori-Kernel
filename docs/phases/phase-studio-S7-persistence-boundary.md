# Phase: Studio S7 — Persistence Boundary Cleanup

## Goal

Close out the six follow-up items from S6's "remaining risks": unify
TypeScript `$VALORI_HOME` resolution, decide and wire `modelDir`
ownership, decide `metadata.redb`'s fate, give `logs/`/`crashes/` real
content, remove deprecated persistence (`tauri-plugin-store`, remaining
desktop `localStorage` state), and add one final, consolidated
architecture test preventing the five accidental-regression patterns
named in the task.

## Delivered

### 1. Unified TypeScript `$VALORI_HOME` resolution

- **[`ui/src/lib/server/valori-home.ts`](../../ui/src/lib/server/valori-home.ts)**
  (new) — the one TypeScript-side `getValoriHome()`. Replaces three
  independent, agreeing copies (`api-client.ts`, `connection.ts`,
  `cluster-config.ts`) and fixes two that were **`VALORI_HOME`-env-blind**
  (`projects.ts`, `project-adapter.ts` — hardcoded `os.homedir() +
  ".valori"`, silently ignoring an explicit override). `cluster-config.ts`
  also had a literal `"~/.valori/cluster"` **string** passed to a spawned
  process — never shell-expanded by Node's `child_process`, a real latent
  bug now fixed as a side effect of unification (resolves to an absolute
  path).
- `project-adapter.ts`'s daemon-unreachable fallback remains genuinely
  daemon-first (unchanged design, now just using the shared resolver for
  its own default instead of a second hardcoded copy).
- Rust stays authoritative for the actual desktop app's `$VALORI_HOME`
  (`StudioPaths`, unchanged) — the TS resolver is explicitly documented as
  a ui-server bootstrap/fallback value, not a competing authority.

### 2. `modelDir` — decided and wired

**Decision**: `modelDir` overrides only where model *artifacts* install —
independent of `workspaceDir`, matching the Settings UI's own two-separate-
folder-pickers design (previously cosmetic; now real).

- **`crates/valori-models/src/lib.rs`** — `ModelManager::new_with_models_dir(home,
  models_dir_override, store)`. The small manifest index
  (`JsonModelStore`'s `<home>/models.json`) stays keyed to `home`
  unconditionally — a deliberate split: metadata stays with Studio/daemon
  home, potentially-large artifacts can move.
- **`crates/valori-daemon/src/{daemon.rs,main.rs}`** — `Daemon::new_with_models_dir`,
  reading `VALORI_MODELS_DIR` at startup. (This env var already existed as
  a *read* fallback in `valori-node`'s `models_health` endpoint —
  confirmed via the audit; S7 is what actually sets it.)
- **`desktop/src-tauri/src/daemon_manager.rs`** — `start_daemon`/`start_daemon_internal`
  gain `model_dir: Option<String>`, passed as `VALORI_MODELS_DIR` to the
  spawned daemon, alongside (never instead of) `VALORI_HOME`.
- **`ui/src/lib/native.ts` + 3 callers** (`AppShellGate.tsx`,
  `DaemonBanner.tsx`, `Welcome.tsx`) — `startDaemon(home, modelDir)`.

### 3. `metadata.redb` — decided, not wired, not deleted

Investigated whether to wire it in or retire it, per the task's binary
framing. Neither, precisely — see Findings for why. The crate's
`Project`/`Collection` tables/adapter (`domain_adapter.rs`) are real,
tested, deliberately-staged **M3** infrastructure (`docs/phases/phase-M0-M2-platform-contracts.md`:
*"Nothing deleted or migrated — M3 stopped for review"*) — a past,
conscious pause, not oversight. Deleting it would destroy that work for
no safety gain, since nothing currently opens the file (confirmed,
again, by this phase's own search). Instead:

- `crates/valori-metadata/src/lib.rs`'s crate doc now states plainly:
  not opened by any production binary, `valori-daemon`/`valori-studio-storage`
  remain the sole live authorities, reactivation is a deliberate decision.
- **New enforcement**: `crates/valori-node/tests/dependency_direction.rs`'s
  `metadata_db_open_stays_out_of_production_binaries` fails the build the
  moment `MetadataDb::open(` appears in `valori-node`, `valori-daemon`, or
  `desktop/src-tauri` — the "don't leave two databases with overlapping
  responsibility" requirement is now impossible to violate *by accident*;
  violating it on purpose requires updating this test, which is the point.

### 4. `logs/` and `crashes/` — real content

- **`logs/`**: `desktop/src-tauri/src/lib.rs`'s `run()` now writes to
  `$VALORI_HOME/logs/studio.log` via `tracing-appender` (daily rotation),
  **in addition to** stdout (unchanged). Non-fatal — if the directory
  can't be created, the app falls back to stdout-only, exactly the
  pre-S7 behavior. Bounded: `FileSystemService::cleanup_old_logs` prunes
  files older than 7 days at every startup (non-fatal, matching S5/S6's
  established pattern).
- **`crashes/`**: the live panic-hook marker path is **unchanged**
  (Tauri's `app_config_dir()` — S6's documented, permanent exception).
  What's new: `telemetry.rs::check_and_clear_crash_marker` now
  best-effort **archives** a copy into `$VALORI_HOME/crashes/` once a
  crash marker is read — giving the canonical directory real, bounded
  content (`FileSystemService::cleanup_old_crash_archives`, 30-day
  window) without touching the one write path that must stay
  minimal-risk.

### 5. Deprecated persistence removed

- **`tauri-plugin-store`** — dependency, plugin registration, and
  `store:default` capability entry all removed. Zero call sites existed
  (confirmed in S4/S6, reconfirmed here); the crate had been superseded
  since S2b-2a.
- **`valori:notifs`** — migrated off `localStorage` on desktop.
  `StudioPreferences` gained `notification_prefs: Option<serde_json::Value>`
  (a `Value`, not a typed struct — deliberately: this is a UI-defined,
  growable notification-type bag, unlike the stable, well-defined
  `telemetry_consent`/`window_state`). `SettingsModal.tsx` now reads/writes
  it via `getPreference`/`setPreference("notifs", …)` on desktop, keeps
  `localStorage` on web (no `studio.redb` there).
- **Legacy `preferences.json`/`events.jsonl`** — audited for deletion
  eligibility, **not deleted**. No explicit, tested retention policy for
  these exists (S6's own "never delete user data automatically without
  one" rule), and they're already permanently read-only/harmless. Decision:
  leave as is; deletion would need a dedicated, separately-approved phase
  with its own retention policy and tests, not a byproduct of this cleanup.

### 6. Final persistence architecture test

**[`desktop/src-tauri/tests/persistence_boundary_architecture.rs`](../../desktop/src-tauri/tests/persistence_boundary_architecture.rs)**
(new, 8 tests) — one file, the task's five patterns:

1. `every_valori_localstorage_key_is_on_the_explicit_allowlist` /
   `..._constant_is_on_the_explicit_allowlist` — every `"valori:*"`
   `localStorage` key (literal or `const`) actually used in `ui/src` is
   checked against a documented allowlist; anything new fails the build,
   forcing "should this be in `studio.redb` instead?" to be a deliberate
   answer, not an accident.
2. `ui_still_never_imports_the_tauri_filesystem_plugin` — reinforces S6.
3. `no_new_rust_module_mints_its_own_dot_valori_path` /
   `no_new_typescript_module_mints_its_own_valori_home_default` — allowlists
   of exactly which files may construct a `.valori`/`$VALORI_HOME` path;
   this audit surfaced **two more pre-existing duplicates** beyond the TS
   ones fixed in item 1 — `valori-node`'s `server.rs`/`cluster_server.rs`
   (`models_health`'s `VALORI_MODELS_DIR`-with-fallback) and `valori-cli`'s
   `import.rs`/`wizard.rs` — allowlisted, not consolidated (out of this
   phase's scope; see Follow-ups).
4. `studio_storage_still_cannot_depend_on_project_internals` — reinforces
   S6/`dependency_direction.rs`.
5. `only_the_three_known_files_open_an_embedded_redb_database` /
   `no_second_database_engine_is_introduced` — an explicit allowlist of
   the three legitimate `Database::create(` call sites
   (`valori-studio-storage`, `valori-metadata`, `valori-consensus`), plus
   a hard ban on `rusqlite`/`sled`/`sqlx`/`rocksdb` appearing anywhere in
   the workspace.

## Findings

- **Correction to every prior Studio phase's `cargo fmt --check` claim
  (S3 through S6)**: `desktop/src-tauri` is a separate Cargo workspace
  from the repository root. Every previous phase's "clean" report for
  this check ran `cargo fmt --check` from the root, which does **not**
  cover `desktop/src-tauri` at all — its formatting was never actually
  verified. Discovered while doing S7's own final verification pass
  (running the check from inside `desktop/src-tauri` for the first time
  surfaced ~20 pre-existing diffs across files this and prior phases
  added). Fixed here (`rustfmt`, whitespace-only, no logic change) and
  now genuinely clean; future phases must run `cargo fmt --check` from
  **both** the root and `desktop/src-tauri` to actually cover both.
- `VALORI_MODELS_DIR` already existed as a **read-side** fallback
  (`valori-node`'s `models_health` endpoint) before this phase — S7's
  daemon-side wiring is what actually *sets* it; the naming was already
  correct, unplanned confirmation the decision matched existing intent.
- The `metadata.redb` decision required reading `domain_adapter.rs` in
  full before concluding anything — a shallower read would have
  classified `Project`/`Collection` as pure dead code and deleted real,
  tested M3-preparation work. The evidence (a paused, not abandoned, past
  phase) changed the decision from "retire" to "document + enforce
  dormancy."
- Building the localStorage allowlist required distinguishing genuine
  storage keys from same-prefix `CustomEvent` names (`"valori:toast"`,
  `"valori:open-settings"`, `"valori:new-project"` — `window.dispatchEvent`/
  `addEventListener`, unrelated to persistence) and from
  `"valori-onboarding:*"` (a different, hyphenated prefix, also
  unrelated) — a naive grep would have produced a wrong, over-broad
  allowlist.

## Validation

```text
cargo fmt --check                                                  clean
cargo check --workspace                                            clean
cargo test --workspace                                             all green, 0 failures
cargo clippy -p valori-studio-storage --all-targets -- -D warnings clean
cargo test -p valori-node --test dependency_direction              7 passed, 0 failed (was 6, +1: metadata_db_open_stays_out_of_production_binaries)
cargo test -p valori-node --test architecture                      1 passed, 0 failed
npx tsc --noEmit                                                   clean
npm run build                                                       succeeds

cargo test -p valori-studio-storage    125 passed (unchanged — new field, no new tests there)
cargo test -p valori-models              78 passed total incl. doctests (+2 new: models_dir override tests)
cargo test -p valori-daemon             42 passed, unchanged
cargo test -p valori-metadata           19 passed, unchanged (doc-only change)

Desktop crate (separate build, outside the root workspace):
cargo build --lib                       clean
cargo test --lib                        71 passed (was 68, +3: cleanup_old_logs ×2, notifs)
cargo test --test persistence_boundary_architecture   8 passed, 0 failed (new)
cargo test --test filesystem_architecture              5 passed, 0 failed (unchanged)
cargo test --test credential_security_architecture     5 passed, 0 failed (unchanged)
cargo test --test installation_id_architecture         4 passed, 0 failed (unchanged)
cargo test --test session_retention_architecture       4 passed, 0 failed (unchanged)
```

## Follow-ups

- `valori-node`'s `server.rs`/`cluster_server.rs` and `valori-cli`'s
  `import.rs`/`wizard.rs` each independently construct `.valori` paths —
  found by this phase's audit, allowlisted (not consolidated). Real
  cleanup opportunity, out of S7's TS-focused unification scope.
- Legacy `preferences.json`/`events.jsonl` deletion — explicitly deferred,
  needs its own retention-policy phase if ever pursued.
- Reranker-config triplication (`SettingsModal.tsx`/`settings/page.tsx`/
  `AskTab.tsx`) — still not unified; still explicitly out of scope
  (named "S6 reranker consolidation" in the original S4 audit, never
  requested since).
- Non-secret provider config (`provider`/`model`/`credentialRef`) staying
  in `localStorage` rather than `studio.redb` — still an open question
  from S4's audit, not resolved here.
