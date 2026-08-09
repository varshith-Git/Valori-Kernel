// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Architecture tripwire — Studio S6 (Desktop Filesystem Consolidation).
//!
//! Same source-scanning technique as `installation_id_architecture.rs`,
//! `credential_security_architecture.rs`, and
//! `session_retention_architecture.rs`. The Rust-side compile-time
//! boundary (`valori-studio-storage` cannot depend on
//! `valori-kernel`/`valori-storage`/`valori-node`/`valori-daemon`) is
//! already enforced by `crates/valori-node/tests/dependency_direction.rs`'s
//! `SEALED_CRATES` allowlist — this file adds the checks that boundary
//! doesn't cover: the desktop crate's own dependency list, and the
//! TypeScript/browser side, which has no Cargo graph to lean on.
//!
//! See `docs/reviews/studio-filesystem-audit.md` and
//! `docs/phases/phase-studio-S6-filesystem-management.md`.

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

fn read_production_code(path: &Path) -> String {
    let contents = read(path);
    match contents.find("#[cfg(test)]") {
        Some(idx) => contents[..idx].to_string(),
        None => contents,
    }
}

/// Recursively collects every `.ts`/`.tsx` file under `dir`, skipping
/// `node_modules` and `.next` build output.
fn walk_ts_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "node_modules" || name == ".next" {
                continue;
            }
            walk_ts_files(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "ts" || ext == "tsx" {
                out.push(path);
            }
        }
    }
}

// ── §21: studio storage → project internals is a compile-time boundary ────

/// `valori-studio-storage`'s `Cargo.toml` must never list
/// `valori-kernel`, `valori-storage`, `valori-node`, or `valori-daemon` as
/// a dependency — the compile-time half of "studio storage → project
/// internals/WAL/snapshots" being structurally impossible. Redundant with
/// `dependency_direction.rs`'s `SEALED_CRATES` check by design (belt and
/// suspenders — this one is scoped to exactly this phase's concern and
/// fails with a filesystem-specific message).
#[test]
fn studio_storage_crate_cannot_depend_on_project_internals() {
    let root = repo_root();
    let manifest = read(&root.join("crates/valori-studio-storage/Cargo.toml"));
    for forbidden in dependency_line_patterns(&[
        "valori-kernel",
        "valori-storage",
        "valori-node",
        "valori-daemon",
    ]) {
        assert!(
            !manifest.contains(&forbidden),
            "valori-studio-storage/Cargo.toml must never depend on {forbidden:?} — \
             Studio metadata must never reach into project internals"
        );
    }
}

/// A crate name only counts as "depended on" as an actual TOML dependency
/// line (`name = {` or `name = "`), not any prose mention of the name in
/// a comment — both `Cargo.toml`s here have doc comments *describing*
/// this exact boundary, which a plain substring check would wrongly flag.
fn dependency_line_patterns(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .flat_map(|n| [format!("{n} = {{"), format!("{n} = \"")])
        .collect()
}

/// Same check for `desktop/src-tauri` itself — the Tauri layer resolves
/// project *paths* only (via `valori-studio-storage::StudioPaths` /
/// `valori-daemon`'s own manifest, at the process-spawn boundary), it must
/// never link against the crates that actually read/write WAL, snapshot,
/// index, or vector bytes.
#[test]
fn desktop_crate_cannot_depend_on_project_internals() {
    let root = repo_root();
    let manifest = read(&root.join("desktop/src-tauri/Cargo.toml"));
    for forbidden in dependency_line_patterns(&["valori-kernel", "valori-storage", "valori-node"]) {
        assert!(
            !manifest.contains(&forbidden),
            "desktop/src-tauri/Cargo.toml must never depend on {forbidden:?}"
        );
    }
}

/// `filesystem_service.rs` must never name a project-internal file/format
/// — it has no business knowing what a WAL segment or snapshot file is
/// called, only Studio's own canonical directories.
#[test]
fn filesystem_service_never_names_project_internal_files() {
    let root = repo_root();
    let contents = read_production_code(&root.join("desktop/src-tauri/src/filesystem_service.rs"));
    for forbidden in ["events.log", "snapshot.val", ".namespaces.json"] {
        assert!(
            !contents.contains(forbidden),
            "filesystem_service.rs must never reference the project-internal filename {forbidden:?}"
        );
    }
}

// ── §16/§21: UI → arbitrary filesystem ─────────────────────────────────────

/// No `.ts`/`.tsx` file under `ui/src` may import `@tauri-apps/plugin-fs`
/// — the compile-time-adjacent guard for "no platform-specific filesystem
/// logic in TypeScript." `ui/src/lib/server/*.ts` legitimately touches the
/// filesystem via Node's own `fs`/`path`/`os` modules (it's the Next.js
/// server process, not the browser — see the filesystem audit's UI
/// section for why that's a different, allowed layer), so this check is
/// specifically about the Tauri filesystem plugin, which would mean the
/// *browser* layer reaching for raw disk access — the one thing §16
/// forbids.
#[test]
fn ui_never_imports_the_tauri_filesystem_plugin() {
    let root = repo_root();
    let mut files = Vec::new();
    walk_ts_files(&root.join("ui/src"), &mut files);
    assert!(
        !files.is_empty(),
        "expected to find .ts/.tsx files under ui/src"
    );

    for path in files {
        let contents = read(&path);
        assert!(
            !contents.contains("@tauri-apps/plugin-fs"),
            "{} must not import @tauri-apps/plugin-fs — the browser layer must go \
             through a Tauri command, never raw filesystem access",
            path.display()
        );
    }
}

/// Cloud's client-side surface (`ui/src/app/cloud/**`) must never import
/// Node's `fs`/`path`/`os` modules or the Tauri filesystem plugin — Cloud
/// is Supabase-backed exclusively (confirmed in the S4 persistence audit)
/// and must have no local filesystem footprint, matching §21's "cloud →
/// local filesystem" prohibition.
#[test]
fn cloud_surface_never_touches_the_local_filesystem() {
    let root = repo_root();
    let cloud_dir = root.join("ui/src/app/cloud");
    let mut files = Vec::new();
    walk_ts_files(&cloud_dir, &mut files);
    assert!(
        !files.is_empty(),
        "expected to find files under ui/src/app/cloud"
    );

    for path in files {
        let contents = read(&path);
        for forbidden in [
            "from \"fs\"",
            "from 'fs'",
            "require(\"fs\")",
            "require('fs')",
            "@tauri-apps/plugin-fs",
        ] {
            assert!(
                !contents.contains(forbidden),
                "{}: Cloud surface must never touch the local filesystem (found {forbidden:?})",
                path.display()
            );
        }
    }
}
