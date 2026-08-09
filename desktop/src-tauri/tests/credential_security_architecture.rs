// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Architecture tripwire — Studio S3 (Credential Security).
//!
//! Companion to `installation_id_architecture.rs`'s technique
//! (pure source-text scanning — this crate's internal modules are private,
//! so integration tests here cannot call into them directly; API-level
//! guard-rail tests instead live inline in `credential_service.rs`'s and
//! `preferences_service.rs`'s own `#[cfg(test)]` blocks, which have full
//! crate-internal access).
//!
//! See `docs/reviews/studio-credentials-audit.md` and
//! `docs/phases/phase-studio-S3-credentials.md`.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
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

/// Strip trailing `#[cfg(test)] mod tests { ... }` — see
/// `installation_id_architecture.rs`'s identical helper and rationale.
fn read_production_code(path: &Path) -> String {
    let contents = read(path);
    match contents.find("#[cfg(test)]") {
        Some(idx) => contents[..idx].to_string(),
        None => contents,
    }
}

// ── §16: studio.redb never contains a secret ──────────────────────────────

/// Source-level guarantee: `set_field`'s match arms are a fixed,
/// exhaustive allowlist with no secret-shaped key literal in it. Pins the
/// allowlist itself so a future edit can't add a secret-shaped arm without
/// this failing. Runtime behavior (that unrecognized keys are silently
/// dropped) is proven separately in `preferences_service.rs`'s own
/// `generic_preference_bridge_rejects_every_secret_shaped_key` test.
#[test]
fn preferences_service_source_has_no_secret_shaped_match_arm() {
    let root = repo_root();
    let contents = read_production_code(&root.join("desktop/src-tauri/src/preferences_service.rs"));

    for forbidden in [
        "\"apiKey\"",
        "\"api_key\"",
        "\"secret\"",
        "\"token\"",
        "\"password\" =>",
        "\"authorization\"",
    ] {
        assert!(
            !contents.contains(forbidden),
            "preferences_service.rs must not gain a match arm for {forbidden} — \
             studio.redb must never be able to persist a secret"
        );
    }
}

// ── §19: telemetry never contains a secret ─────────────────────────────────

/// Source-level guard: no production call site in `telemetry.rs` builds a
/// `StudioTelemetryEvent`/enqueue payload from a variable that looks like a
/// raw secret (as opposed to a `credential_ref`/`installation_id`, which
/// are safe references). Heuristic, not a proof — same caveat this
/// codebase already accepts for `installation_id_architecture.rs`'s
/// equivalent checks.
#[test]
fn telemetry_source_never_interpolates_a_variable_named_like_a_raw_secret() {
    let root = repo_root();
    let contents = read_production_code(&root.join("desktop/src-tauri/src/telemetry.rs"));

    for forbidden in ["api_key", "apiKey", "raw_secret", "password"] {
        assert!(
            !contents.contains(forbidden),
            "telemetry.rs must never reference a variable/field named like a raw secret \
             (found {forbidden:?}) — only credential_ref/installation_id-shaped values are safe"
        );
    }
}

// ── §21: crash reporting never contains a secret ───────────────────────────

/// `CrashInfo`'s field list is closed and enumerated — pins that it stays
/// exactly `{panic_hash, panic_location, previous_session,
/// uptime_before_crash_secs}`, no secret-shaped field.
#[test]
fn crash_info_field_list_has_no_secret_shaped_field() {
    let root = repo_root();
    let contents = read_production_code(&root.join("desktop/src-tauri/src/telemetry.rs"));

    let start = contents
        .find("pub struct CrashInfo {")
        .expect("CrashInfo must exist");
    let end = contents[start..]
        .find("}\n")
        .map(|i| start + i)
        .expect("CrashInfo struct body must close");
    let body = &contents[start..end];

    for forbidden in [
        "api_key",
        "apikey",
        "secret",
        "token",
        "password",
        "credential",
    ] {
        assert!(
            !body.to_lowercase().contains(forbidden),
            "CrashInfo must never gain a secret-shaped field (found {forbidden:?})"
        );
    }
}

// ── §20/§27: logging never contains a secret ────────────────────────────────

/// No `tracing::*!`/`println!`/`eprintln!`/`dbg!` call site in
/// `desktop/src-tauri/src/*.rs` interpolates a variable literally named
/// like a raw secret. Scans only Valori's own source (not vendored crates
/// like `keyring`, which are out of this repository's control).
#[test]
fn no_desktop_source_file_logs_a_variable_named_like_a_secret() {
    let root = repo_root();
    let src_dir = root.join("desktop/src-tauri/src");

    for entry in fs::read_dir(&src_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let contents = read_production_code(&path);
        for line in contents.lines() {
            let is_log_call = line.contains("tracing::")
                || line.trim_start().starts_with("println!")
                || line.trim_start().starts_with("eprintln!")
                || line.trim_start().starts_with("dbg!");
            if !is_log_call {
                continue;
            }
            for forbidden in ["{secret", "{api_key", "{apiKey", "{password", "{raw_secret"] {
                assert!(
                    !line.contains(forbidden),
                    "{}: logging call must not interpolate a secret-named variable: {line}",
                    path.display()
                );
            }
        }
    }
}

// ── §7/§10: no broad "get all credentials" API exists ──────────────────────

/// `CredentialService`/the Tauri command surface must not expose a
/// `get_all_credentials()`-style broad API — only narrowly scoped
/// per-`CredentialRef` operations (store/get/exists/delete).
#[test]
fn credential_service_exposes_no_get_all_credentials_style_api() {
    let root = repo_root();
    let contents = read_production_code(&root.join("desktop/src-tauri/src/credential_service.rs"));
    // Anchored on function definitions specifically, not any substring —
    // this file's own doc comment names `get_all_credentials()` as the
    // anti-pattern to avoid, which a plain substring match would wrongly
    // flag as if it existed.
    for forbidden in [
        "fn get_all_credentials",
        "fn list_credentials",
        "fn all_credentials",
        "fn export_credentials",
    ] {
        assert!(
            !contents.contains(forbidden),
            "credential_service.rs must not define {forbidden:?} — a broad, un-scoped API"
        );
    }
}
