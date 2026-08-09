# Phase Studio S2b-2a — Preferences Runtime Consumer Migration

**Branch:** `feat/node-object-store-durability-and-metrics`
**Depends on:** [Phase Studio S2b-1 (real startup migration integration)](phase-studio-S2b-1-real-startup-migration.md)
**Status:** ✅ S2b-2a complete. **Stopped before S2b-2b** — project registry, sessions, telemetry queue uploader, sync, and updates deferred.

---

## 1. Goal

Migrate all runtime preference consumers (both Next.js/React frontend and Rust-native telemetry/updater call sites) from `tauri-plugin-store` and `preferences.json` to the typed `StudioPreferencesService` backed by `studio.redb`, while ensuring legacy `preferences.json` remains completely unmodified and read-only.

---

## 2. Delivered

### Preference Consumer Audit

Every preference consumer across Rust and TypeScript was audited before implementation:
1. **Rust-side consumers**:
   - `desktop/src-tauri/src/telemetry.rs`: `analytics_consent` (reads `telemetryConsent.analytics`) and `installation_id` (reads and lazily assigns persistent UUID).
2. **TypeScript/React consumers**:
   - `ui/src/lib/native.ts`: `getPreference`, `setPreference`, `isOnboardingComplete`, `markOnboardingComplete`, `resetOnboarding`, `getLastPage`, `setLastPage`, `getTelemetryConsent`, `setTelemetryConsent`, `getInstallationId`.
   - `ui/src/lib/theme.tsx`: `ThemeProvider`, `useTheme`, `setTheme` reading and writing the `theme` preference.
   - Settings & Onboarding components: `SettingsModal.tsx`, `settings/page.tsx`, `Welcome.tsx`, `AppShellGate.tsx`, `Sidebar.tsx`, `DaemonBanner.tsx` calling through `native.ts`.

### `desktop/src-tauri/` (Typed Service & Tauri Commands)

* **`desktop/src-tauri/src/preferences_service.rs`** (new):
  * `StudioPreferencesService`: Encapsulates operations on `db.preferences()`. Provides `get_all`, `set_all`, `get_field(key)`, `set_field(key, val)`, `get_or_init_installation_id()`, `get_telemetry_consent()`, and `set_telemetry_consent()`.
  * Tauri commands: `get_preference`, `set_preference`, `get_all_preferences`, `get_installation_id_command`, `get_telemetry_consent_command`, `set_telemetry_consent_command`.
  * Unit tests: `test_preferences_service_crud_and_idempotency` verifying CRUD, default values, atomic updates, permanent installation id stability, and persistence across database reopen.
* **`desktop/src-tauri/src/telemetry.rs`**:
  * Migrated `analytics_consent` to read `p.telemetry_consent.map(|c| c.analytics)` directly from `Arc<StudioDatabase>`.
  * Migrated `installation_id` to read and lazily initialize `InstallationId::new()` in `studio.redb`.
  * Removed all `app.store("preferences.json")` calls.
* **`desktop/src-tauri/src/lib.rs`**:
  * Registered `mod preferences_service;` and wired the typed preference commands into `tauri::generate_handler!`.
* **`desktop/src-tauri/Cargo.toml`**:
  * Added `valori-domain = { path = "../../crates/valori-domain" }`.

### `ui/` (Frontend Native Bridge)

* **`ui/src/lib/native.ts`**:
  * Replaced `tauri-plugin-store` and `LazyStore("preferences.json")` with typed `invoke("get_preference", { key })` and `invoke("set_preference", { key, value })`.
  * `getInstallationId()` now invokes `get_installation_id_command`, returning the permanent UUID from `studio.redb`.
  * `getTelemetryConsent()` and `setTelemetryConsent()` now invoke `get_telemetry_consent_command` and `set_telemetry_consent_command`.
  * In-memory fallback provided for browser dev mode.
* **`ui/src/lib/theme.tsx`**:
  * Initial mount in desktop shell loads stored theme via `getPreference<ThemePref>("theme")`.
  * `setTheme` updates `studio.redb` via `setPreference("theme", p)`.
  * Fallback to `localStorage` retained for browser execution.

### Integration Tests

* **`crates/valori-studio-storage/tests/startup_integration.rs`**:
  * Added `preferences_runtime_flow_persists_to_studio_redb_and_leaves_legacy_file_unchanged` test. Demonstrably proves:
    1. Legacy preferences are migrated into `studio.redb` during first launch.
    2. Theme and telemetry consent changes write to `studio.redb`.
    3. Legacy `preferences.json` file is **byte-for-byte identical before and after runtime writes**.
    4. Second startup detects migration marker (`already_migrated: true`).
    5. Restarted application reads modified theme (`"light"`), telemetry consent (`{ analytics: false, crash: true }`), and identical `installation_id`.
    6. Legacy `preferences.json` remains untouched.

---

## 3. Findings

1. **Clean Separation of Concerns**:
   * With `StudioPreferencesService`, the UI no longer requires arbitrary JSON file storage. Preferences are strongly typed and validated.
2. **Permanent Identity Invariant**:
   * `installation_id` is lazily minted once as a UUID, saved into `studio.redb`'s singleton preferences row, and returned identically across all future sessions.
3. **Legacy Preservation**:
   * `preferences.json` is never written to by any service or command. It remains purely an archived snapshot for backward compatibility.

---

## 4. Validation

* **`cargo test -p valori-studio-storage`**: **83 passed, 0 failed** (including all unit, migration, concurrency, and startup integration tests).
* **`cargo test` in `desktop/src-tauri`**: **9 passed, 0 failed** (including `test_preferences_service_crud_and_idempotency`).
* **`cargo test -p valori-kernel -p valori-node`**: **All passed** (including `dependency_direction.rs` and route parity).
* **`cargo check --workspace`**: **Clean compilation** across all workspace crates.
* **`npm run build` in `ui/`**: **0 errors**, all 71 routes compiled and statically/dynamically optimized.
* **Real Application Launch & Smoke Test**:
  * Ran `./target/debug/valori-desktop` on macOS.
  * Verified startup log output:
    ```text
    INFO valori_desktop_lib::studio_storage: Studio database opened at /Users/as-mac-0272/.valori/studio.redb
    DEBUG valori_desktop_lib::studio_storage: Legacy preferences already migrated
    INFO valori_desktop_lib::studio_storage: Studio storage initialization completed
    ```
  * Inspected `/Users/as-mac-0272/.valori/studio.redb` (size 1.58 MB, active and updated).
  * Inspected `~/Library/Application Support/com.valori.desktop/preferences.json` (completely unchanged).

---

## 5. Follow-ups & Staged S2b-2 Rollout

* **S2b-2b (Next)**: Project & recent project registry runtime migration (`ProjectRegistry`).
* **S2b-2c (Deferred)**: Sessions store runtime migration.
* **S2b-2d (Deferred)**: Telemetry queue uploader migration (`telemetry_queue` draining).
* **S2b-2e (Deferred)**: Update state runtime migration.
* **S3 (Deferred)**: Credential isolation and OS keychain integration.
