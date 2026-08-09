# Phase Studio S2b-1 — Real Startup Migration Integration

**Branch:** `feat/node-object-store-durability-and-metrics`
**Depends on:** [Phase Studio S2a (legacy migration engine)](phase-studio-S2a-legacy-migration.md)
**Status:** ✅ S2b-1 complete. **Stopped before S2b-2** — runtime consumers not yet migrated.

---

## 1. Goal

Wire the S2a migration engine into the real Tauri desktop application's startup lifecycle (`desktop/src-tauri`), resolving actual OS-specific paths via Tauri, opening/creating `studio.redb`, running legacy migration if needed, and managing `Arc<StudioDatabase>` in application state.

---

## 2. Delivered

### `desktop/src-tauri/` (Desktop application startup integration)

* **`desktop/src-tauri/Cargo.toml`**: Added `valori-studio-storage = { path = "../../crates/valori-studio-storage" }` dependency.
* **`desktop/src-tauri/src/studio_storage.rs`** (new):
  * `resolve_legacy_paths(app)`: Resolves real on-disk legacy paths for `preferences.json` and `events.jsonl` using Tauri's path resolution (`app.path().app_config_dir()`).
  * `init_studio_storage_with_paths(db_path, legacy_paths)`: Opens/creates `studio.redb` at the specified location, executes legacy migration with `chrono::Utc::now().timestamp_millis()`, logs non-sensitive diagnostics with `tracing`, and returns `(Arc<StudioDatabase>, LegacyMigrationSummary)`.
  * `init_studio_storage(app)`: Standard entry point resolving `$VALORI_HOME/studio.redb` or `~/.valori/studio.redb` via `default_db_path()` and Tauri config paths, returning `Arc<StudioDatabase>`.
  * `log_migration_summary(summary)`: Logs human-readable diagnostic messages without exposing sensitive data, API keys, or raw telemetry payloads.
  * Unit tests validating fresh install, existing install, and idempotency on second launch.
* **`desktop/src-tauri/src/lib.rs`**:
  * Added `mod studio_storage;`.
  * In `.setup(move |app| { ... })`: Initialized `studio_storage::init_studio_storage(app.handle())` and registered `app.manage(studio_db)`.

### `crates/valori-studio-storage/` (Integration verification)

* **`crates/valori-studio-storage/tests/startup_integration.rs`** (new, 5 integration tests):
  * `fresh_installation_creates_db_and_succeeds_without_legacy_files`: `studio.redb` created, schema v1 verified, `source_found: false` reported without error.
  * `existing_installation_migrates_legacy_files_and_preserves_originals`: `preferences.json` and `events.jsonl` imported transactionally, legacy files byte-identical before and after.
  * `second_startup_is_idempotent_and_performs_no_duplicate_import`: Second and third startups detect `already_migrated: true`, no duplicate telemetry queued, no second migration.
  * `migration_failure_on_corrupt_legacy_file_preserves_file_and_leaves_db_recoverable`: Corrupted JSON in `preferences.json` fails with serde error, legacy file preserved untouched, database remains uncorrupted and retryable.
  * `migration_preserves_existing_unrelated_files_and_projects`: Verifies existing `metadata.redb` and `projects/` directories are completely untouched.

---

## 3. Findings & Storage Roots Architecture

1. **Two Distinct Storage Roots (Canonical vs. Legacy-Only)**:
   * **Canonical Valori Application Data (`~/.valori/`)**:
     ```text
     Valori application data
             ↓
     ~/.valori/ (or $VALORI_HOME)
             │
             ├── studio.redb       (Valori Studio local persistence)
             ├── metadata.redb     (Daemon & planner control plane)
             ├── projects/         (Project manifests and storage)
             ├── models/           (Model packages and artifacts)
             ├── logs/             (System logs)
             ├── crashes/          (Local crash markers)
             ├── cache/            (Project display cache)
             └── downloads/        (Staged update artifacts)
     ```
   * **Tauri Application Config (Legacy-Only)**:
     * macOS: `~/Library/Application Support/com.valori.desktop/`
     * Windows: `%APPDATA%\com.valori.desktop`
     * Linux: `~/.config/com.valori.desktop`
     This path is used strictly as a read-only legacy source during startup migration to import existing `preferences.json` and `events.jsonl`.
2. **Redb File Lock Concurrency**:
   * `redb::Database` holds an exclusive lock while open. Startup lifecycle cleanly initializes a single `Arc<StudioDatabase>` per process and manages it in Tauri application state.
3. **Fail-Safe Invariants**:
   * Legacy files are strictly read-only and never deleted, renamed, or modified.
   * If migration fails on a malformed legacy file, the error is logged, the legacy file is untouched, the DB is untouched, and the completed flag is not set, allowing future recovery.

---

## 4. Validation

* **Actual Desktop Application Execution**:
  * Executed the compiled `valori-desktop` binary in the real macOS environment.
  * **First launch output**:
    ```text
    INFO valori_desktop_lib::studio_storage: Studio storage initialization started
    INFO valori_desktop_lib::studio_storage: Studio database opened at /Users/as-mac-0272/.valori/studio.redb
    INFO valori_desktop_lib::studio_storage: Legacy preferences imported: 1 records (0 skipped)
    DEBUG valori_desktop_lib::studio_storage: Legacy telemetry queue not found; no migration needed
    INFO valori_desktop_lib::studio_storage: Studio storage initialization completed
    ```
  * **Second launch output (idempotency verified)**:
    ```text
    INFO valori_desktop_lib::studio_storage: Studio storage initialization started
    INFO valori_desktop_lib::studio_storage: Studio database opened at /Users/as-mac-0272/.valori/studio.redb
    DEBUG valori_desktop_lib::studio_storage: Legacy preferences already migrated
    DEBUG valori_desktop_lib::studio_storage: Legacy telemetry queue not found; no migration needed
    INFO valori_desktop_lib::studio_storage: Studio storage initialization completed
    ```
* **Test Suites**:
  * **`cargo test -p valori-studio-storage`**: 82 passed (77 unit + 5 startup integration), 0 failed.
  * **`cargo test` in `desktop/src-tauri`**: 8 passed, 0 failed.
  * **`cargo test -p valori-kernel -p valori-node`**: All kernel and node tests passed (including `dependency_direction.rs`).
  * **`cargo check --workspace`**: Clean compilation across all workspace members.
  * **`cargo clippy -p valori-studio-storage --all-targets -- -D warnings`**: 0 warnings.

---

## 5. Follow-ups & Staged S2b-2 Rollout

Runtime consumer migration is deferred to S2b-2 in discrete, testable sub-phases:
1. **S2b-2a**: Preferences bridge migration (reading/writing `StudioPreferences`).
2. **S2b-2b**: Project & recent project registry migration.
3. **S2b-2c**: Sessions store migration.
4. **S2b-2d**: Telemetry queue & uploader migration.
5. **S2b-2e**: Update state migration.
6. **S3**: Credential isolation and OS keychain integration.
