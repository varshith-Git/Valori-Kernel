// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Architecture tripwire — Studio Installation Identity phase.
//!
//! The installation-id audit (`docs/reviews/installation-id-audit.md`) found
//! three independent get-or-init implementations. This test mechanically
//! enforces the consolidated architecture so a fourth one can't quietly
//! reappear:
//!
//! ```text
//! Desktop
//!    ↓
//! StudioPreferencesService::get_or_init_installation_id   (the ONE canonical impl)
//!    ↓
//! StudioDatabase
//!    ↓
//! preferences.installation_id                             (the ONE persisted field)
//! ```
//!
//! Every other Rust/TS call site must be a *read through* this service (or,
//! for the browser/non-Tauri build, the deliberately separate web-identity
//! fallback in `native.ts` — never a second desktop persistence path).

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // desktop/src-tauri/tests/ -> desktop/src-tauri -> desktop -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// Every file this test scans puts its `#[cfg(test)] mod tests { ... }` at
/// the end of the file (repo-wide convention, confirmed across
/// `preferences_service.rs`, `telemetry.rs`, `session_service.rs`). Test
/// fixtures legitimately construct `InstallationId::new()` (e.g. to seed a
/// pre-existing id) — that's not a duplicate production generation site, so
/// strip the test module before scanning production code.
fn read_production_code(path: &Path) -> String {
    let contents = read(path);
    match contents.find("#[cfg(test)]") {
        Some(idx) => contents[..idx].to_string(),
        None => contents,
    }
}

/// Exactly one Rust function may construct a fresh `InstallationId` and
/// persist it to `studio.redb`: `StudioPreferencesService::get_or_init_installation_id`.
/// `telemetry.rs` must not have its own copy — it consolidated onto this one
/// in the Studio Installation Identity phase.
#[test]
fn exactly_one_rust_installation_id_generation_site_exists() {
    let root = repo_root();

    let preferences_service =
        read_production_code(&root.join("desktop/src-tauri/src/preferences_service.rs"));
    assert!(
        preferences_service.contains("pub fn get_or_init_installation_id"),
        "the canonical get-or-init must live in preferences_service.rs"
    );
    assert_eq!(
        preferences_service.matches("InstallationId::new()").count(),
        1,
        "preferences_service.rs must generate a fresh InstallationId in exactly one place \
         (inside get_or_init_installation_id) — a second occurrence means a duplicate \
         generation path crept back in"
    );

    let telemetry = read_production_code(&root.join("desktop/src-tauri/src/telemetry.rs"));
    assert!(
        !telemetry.contains("InstallationId::new()"),
        "telemetry.rs must not generate installation ids itself — it must read the value \
         through StudioPreferencesService::get_or_init_installation_id (see that function's \
         doc comment for why this was consolidated)"
    );
    assert!(
        !telemetry.contains("db.preferences().update(|p| {\n            if p.installation_id"),
        "telemetry.rs must not write installation_id into the preferences table directly"
    );
}

/// `studio.redb`'s `preferences` table is the only desktop persistence
/// location for installation identity. No second file/table/localStorage
/// path may be introduced on the desktop side.
#[test]
fn no_second_desktop_persistence_location_for_installation_id() {
    let root = repo_root();

    // Forbidden desktop-side persistence file names / patterns from the task
    // spec: installation.id, installation.json, installation.redb, and any
    // desktop-side localStorage usage for this value.
    let desktop_src = root.join("desktop/src-tauri/src");
    for entry in fs::read_dir(&desktop_src).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let contents = read(&path);
        assert!(
            !contents.contains("installation.json")
                && !contents.contains("installation.redb")
                && !contents.contains("installation.id"),
            "{} must not reference a second installation-identity persistence file",
            path.display()
        );
    }
}

/// `native.ts`'s Tauri branch of `getInstallationId()` must be a pure read
/// through the Rust command — it must never fall back to (or dual-write)
/// `localStorage` for the desktop build. The `localStorage`/`getPreference`
/// fallback is only reachable in the non-Tauri (`isTauri() === false`)
/// browser branch.
#[test]
fn native_ts_desktop_branch_never_touches_local_storage_for_installation_id() {
    let root = repo_root();
    let native_ts = read(&root.join("ui/src/lib/native.ts"));

    let start = native_ts
        .find("export async function getInstallationId")
        .expect("getInstallationId must exist in native.ts");
    let end = native_ts[start..]
        .find("\n}\n")
        .map(|i| start + i)
        .unwrap_or(native_ts.len());
    let fn_body = &native_ts[start..end];

    // Brace-depth walk (not a naive find('}')) — the if-block's own body
    // contains a nested `{ invoke }` destructuring brace pair that a naive
    // search would mistake for the block's close.
    let if_start = fn_body
        .find("if (isTauri()) {")
        .expect("getInstallationId must branch on isTauri()");
    let mut depth = 0i32;
    let mut close_idx = None;
    for (i, ch) in fn_body[if_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close_idx = Some(if_start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let tauri_branch_end = close_idx.expect("unbalanced braces in isTauri() branch");
    let tauri_branch = &fn_body[if_start..=tauri_branch_end];

    assert!(
        tauri_branch.contains("get_installation_id_command"),
        "the Tauri branch must call the Rust-side canonical command"
    );
    assert!(
        !tauri_branch.contains("localStorage") && !tauri_branch.contains("getPreference"),
        "the Tauri (desktop) branch of getInstallationId() must not read/write localStorage — \
         studio.redb is the sole desktop source of truth"
    );
}

/// Classification of every remaining `installation_id`/`installationId`
/// call site in the desktop Rust surface: each one must be a *read*
/// (`get_or_init_installation_id`, `.installation_id` field access) not an
/// independent generation site. This is the audit's item 12 turned into a
/// standing check.
#[test]
fn session_service_and_lib_never_generate_installation_ids() {
    let root = repo_root();

    for file in [
        "desktop/src-tauri/src/lib.rs",
        "desktop/src-tauri/src/session_service.rs",
    ] {
        let contents = read_production_code(&root.join(file));
        assert!(
            !contents.contains("InstallationId::new()"),
            "{file} must not generate a fresh InstallationId — it must obtain one via \
             StudioPreferencesService::get_or_init_installation_id"
        );
    }
}
