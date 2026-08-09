# Phase Studio S2b-2b — Project & Recent Project Registry Migration

**Branch:** `feat/node-object-store-durability-and-metrics`
**Depends on:** [Phase Studio S2b-2a (preferences runtime migration)](phase-studio-S2b-2a-preferences-runtime-migration.md)
**Status:** ✅ S2b-2b complete. **Stopped before S2b-2c** — sessions store, telemetry queue uploader, sync, and updates deferred.

---

## 1. Goal

Migrate Studio's project registry, recents, and favorites to the typed `ProjectRegistryService` backed by `studio.redb`'s `projects` table using canonical `valori_domain::ProjectId`, while enforcing the strict architectural boundary that `studio.redb` is a lightweight reference/registry layer and never the project database itself.

---

## 2. Delivered

### Project Registry Audit

Audited all existing project registry, recents, favorites, and path usage across Rust and TypeScript:
1. **Source of Truth for Local Projects**: `~/.valori/projects/<name>/` (`project.json`, WAL, snapshots, collections, vectors) owned by `valori-daemon` / `valori-metadata` / engine.
2. **Source of Truth for Cloud Projects**: Valori Cloud control plane.
3. **Studio Registry Role (`studio.redb` `projects` table)**:
   - Stores `ProjectId`, `display_name`, `kind` (`Local { path }` or `Cloud { organization_id, cloud_endpoint, region }`), `favorite`, `last_opened_at`, `registered_at`.
   - Never stores vectors, WAL, snapshots, indexes, records, collections, or project database state.
4. **Legacy Residue Handling**: Legacy name-only entries (`"demo"`, `"finance-rag"`) in `meta.legacy_project_names` are reconciled only when matched against real projects with canonical `ProjectId`s; unmatched names remain as inert residue without fabricating fake IDs.

### `desktop/src-tauri/` (Typed Service & Tauri Commands)

* **`desktop/src-tauri/src/project_registry_service.rs`** (new):
  * `ProjectRegistryService`: Encapsulates operations on `db.projects()`. Provides `list_projects`, `get_project(id)`, `find_project(id_or_name)`, `recent_projects(limit)`, `favorite_projects`, `register_local_project(id, name, path, now)`, `register_cloud_project(id, name, org_id, endpoint, region, now)`, `rename_project(id, new_name)`, `set_local_path(id, new_path)`, `set_favorite(id, favorite)`, `touch_last_opened(id, now)`, `unregister_project(id)`, and `reconcile_legacy_project_names`.
  * `StudioProjectDto`: Serializes registry records to JSON, computing `available = path.exists()` dynamically for local projects (missing directory on disk reports `available: false` without deleting the registry record).
  * Tauri commands: `registry_list_projects`, `registry_get_project`, `registry_recent_projects`, `registry_favorite_projects`, `registry_register_local_project`, `registry_register_cloud_project`, `registry_rename_project`, `registry_set_local_path`, `registry_set_favorite`, `registry_touch_last_opened`, `registry_unregister_project`, `registry_reconcile_legacy_names`.
  * Unit tests: `test_project_registry_service_crud_and_invariants`, `test_legacy_reconciliation_resolves_known_and_leaves_unresolved_as_residue`.
* **`desktop/src-tauri/src/lib.rs`**:
  * Registered `mod project_registry_service;` and added all 12 project registry commands to `tauri::generate_handler!`.

### `ui/` (Frontend Native Bridge)

* **`ui/src/lib/native.ts`**:
  * Added typed bindings: `registryListProjects`, `registryGetProject`, `registryRecentProjects`, `registryFavoriteProjects`, `registryRegisterLocalProject`, `registryRegisterCloudProject`, `registryRenameProject`, `registrySetLocalPath`, `registrySetFavorite`, `registryTouchLastOpened`, `registryUnregisterProject`.
  * Updated `getRecentProjects`, `touchRecentProject`, `getLastOpenedProject`, `getFavoriteProjects`, `toggleFavoriteProject`, `forgetProject` to route through typed Tauri commands when `nativeAvailable()` is true, updating `studio.redb` directly.

### Integration Tests

* **`crates/valori-studio-storage/tests/startup_integration.rs`**:
  * Added `project_registry_runtime_lifecycle_and_invariants` test. Demonstrably proves:
    1. Legacy project names are reconciled only when real local projects exist, inheriting favorite and last-opened timestamps.
    2. Unknown legacy names (`"finance-rag"`) are never assigned fake IDs.
    3. Registering local projects assigns canonical `valori_domain::ProjectId`.
    4. Project rename updates `display_name` with `ProjectId` unchanged.
    5. Project move updates `path` with `ProjectId` unchanged.
    6. Missing directory preserves the registry entry with `available: false`.
    7. Recents ordering is strictly derived by `last_opened_at` descending.
    8. Database restart restores all registered projects, recents, and favorites.
    9. Legacy `preferences.json` file is **100% byte-for-byte untouched**.

---

## 3. Findings

1. **Strict Separation of Storage Layers**:
   * `studio.redb`'s `projects` table acts exclusively as a local registry index/pointer. Vector data, WAL, snapshots, and index structures remain isolated in the project data directory (`~/.valori/projects/<name>/`).
2. **Canonical Identity Invariant**:
   * Project identity is strictly governed by `valori_domain::ProjectId`. No temporary, synthetic, or path-hashed IDs are generated.
3. **No Duplicate Sources of Truth**:
   * Recent projects is a derived query (`ORDER BY last_opened_at DESC`), and favorites is a boolean field on `StudioProjectRecord`. No separate desynced tables or lists are stored.

---

## 4. Validation

* **`cargo test -p valori-studio-storage`**: **84 passed, 0 failed** (including unit, concurrency, database, migration, projects, and startup integration tests).
* **`cargo test` in `desktop/src-tauri`**: **11 passed, 0 failed** (including `test_project_registry_service_crud_and_invariants` and `test_legacy_reconciliation_resolves_known_and_leaves_unresolved_as_residue`).
* **`cargo check --workspace`**: **Clean compilation** across all workspace crates.
* **`cargo clippy -p valori-studio-storage --all-targets -- -D warnings`**: **0 warnings**.
* **`npm run build` in `ui/`**: **0 errors**, all 71 routes compiled and statically/dynamically generated in Turbopack.
* **Real Application Launch & Smoke Test**:
  * Built and ran `valori-desktop` on macOS.
  * Verified startup log output:
    ```text
    INFO valori_desktop_lib::studio_storage: Studio database opened at /Users/as-mac-0272/.valori/studio.redb
    DEBUG valori_desktop_lib::studio_storage: Legacy preferences already migrated
    INFO valori_desktop_lib::studio_storage: Studio storage initialization completed
    ```

---

## 5. Follow-ups & Staged S2b-2 Rollout

* **S2b-2c (Next)**: Session store runtime migration (`SessionStore`).
* **S2b-2d (Deferred)**: Telemetry queue uploader migration (`telemetry_queue` draining).
* **S2b-2e (Deferred)**: Update state & sync state runtime migration.
* **S3 (Deferred)**: Credential isolation and OS keychain integration.
