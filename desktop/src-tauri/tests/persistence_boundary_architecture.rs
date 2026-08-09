// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The final persistence architecture guard — Studio S7
//! (`docs/phases/phase-studio-S7-persistence-boundary.md`).
//!
//! Mechanically prevents the five things a future developer could
//! accidentally do, per the S7 task:
//!
//! 1. UI → `localStorage` for desktop state that should be in `studio.redb`
//! 2. UI → raw filesystem access
//! 3. A random module minting its own `~/.valori`/`$VALORI_HOME` resolution
//! 4. Studio metadata reaching into project WAL/snapshot internals
//! 5. A new embedded database appearing outside the three known ones
//!
//! Some of these are reinforced, not first introduced, here — #2 and #4
//! already have dedicated tests in `filesystem_architecture.rs`
//! (S6); this file adds #1, #3, #5, and keeps #2/#4 as light
//! confirmations so this one file is a complete, single answer to "what
//! stops the five things," matching the S7 task's framing.

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

fn walk_files(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "node_modules" || name == ".next" || name == "target" {
                continue;
            }
            walk_files(&path, exts, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if exts.contains(&ext) {
                out.push(path);
            }
        }
    }
}

/// Strips `//`-prefixed comment lines — cheap, line-based, not a real
/// parser, but sufficient to avoid flagging documentation that
/// *describes* a pattern (e.g. this very file's own doc comment,
/// `native.ts`'s prose mention of the retired `valori:privacy` key) as if
/// it were live code using it.
fn strip_line_comments(text: &str, comment_prefix: &str) -> String {
    text.lines()
        .map(|line| {
            if line.trim_start().starts_with(comment_prefix) {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── 1. UI → localStorage for desktop state (allowlist) ─────────────────────

/// Every `"valori:..."`-shaped `localStorage` key literal actually used in
/// `ui/src` — anything not on this list is new and must be deliberately
/// added here (forcing the "should this go in studio.redb instead?"
/// question to actually be asked, not skipped).
///
/// Entries and why each is legitimately `localStorage`, not `studio.redb`:
/// - `valori:llm_config`/`valori:embedding_config`/`valori:reranker_config`
///   — provider config; secret portion migrated to the OS keychain (S3),
///   non-secret portion deliberately left here (S3/S4 — see the S7 phase
///   doc's "not done" section for why moving it is still deferred).
/// - `valori:projects-list` — an explicit SWR fallback cache, not a source
///   of truth (see its own source comment).
/// - `valori:archived-projects`, `valori:auto-snap:*`, `valori:activity` —
///   small, purely web-and-desktop-shared UI preferences with no server
///   mirror; genuinely fine as browser state on both surfaces.
/// - `valori:notifs` — desktop now uses `studio.redb` (S7); this key
///   remains legitimate on the **web** build only (no `studio.redb`
///   there) — see `SettingsModal.tsx`'s `nativeAvailable()` branch.
/// - `valori:tree:`, `valori:ask-history:`, `valori:tamper:`,
///   `valori:erasures:` — per-namespace, explicitly rebuildable/disposable
///   caches (tree structure cache, local Q&A history, tamper-check
///   baselines, erasure records), not durable Studio state.
const ALLOWED_LOCALSTORAGE_KEY_PREFIXES: &[&str] = &[
    "valori:llm_config",
    "valori:embedding_config",
    "valori:reranker_config",
    "valori:projects-list",
    "valori:archived-projects",
    "valori:auto-snap:",
    "valori:activity",
    "valori:notifs",
    "valori:tree:",
    "valori:ask-history:",
    "valori:tamper:",
    "valori:erasures:",
];

#[test]
fn every_valori_localstorage_key_is_on_the_explicit_allowlist() {
    let root = repo_root();
    let mut files = Vec::new();
    walk_files(&root.join("ui/src"), &["ts", "tsx"], &mut files);
    assert!(
        !files.is_empty(),
        "expected to find .ts/.tsx files under ui/src"
    );

    let mut violations = Vec::new();
    for path in &files {
        let contents = strip_line_comments(&read(path), "//");
        for (lineno, line) in contents.lines().enumerate() {
            // Only lines that actually call localStorage — a key constant
            // defined elsewhere and referenced by variable name here is
            // covered by the constant-definition scan below instead.
            if !line.contains("localStorage.") {
                continue;
            }
            let Some(start) = line.find("\"valori:") else {
                continue;
            };
            let Some(rest) = line.get(start + 1..) else {
                continue;
            };
            let Some(end) = rest.find('"') else { continue };
            let key = &rest[..end];
            if !ALLOWED_LOCALSTORAGE_KEY_PREFIXES
                .iter()
                .any(|allowed| key == *allowed || key.starts_with(allowed))
            {
                violations.push(format!(
                    "{}:{}: {key:?}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    lineno + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a new \"valori:*\" localStorage key was used without being added to \
         ALLOWED_LOCALSTORAGE_KEY_PREFIXES in this test — decide whether it \
         belongs in studio.redb instead before allowlisting it:\n{}",
        violations.join("\n")
    );
}

/// Same check, for `const`-defined key variables (`STORAGE_KEY`,
/// `ARCHIVED_KEY`, etc.) rather than inline literals — covers every
/// `localStorage.setItem(SOME_CONST, ...)` call site the literal-scan
/// above can't see.
#[test]
fn every_valori_localstorage_key_constant_is_on_the_explicit_allowlist() {
    let root = repo_root();
    let mut files = Vec::new();
    walk_files(&root.join("ui/src"), &["ts", "tsx"], &mut files);

    let mut violations = Vec::new();
    for path in &files {
        let contents = strip_line_comments(&read(path), "//");
        for (lineno, line) in contents.lines().enumerate() {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("const ") || trimmed.starts_with("let ")) {
                continue;
            }
            let Some(start) = line.find("\"valori:").or_else(|| line.find("`valori:")) else {
                continue;
            };
            let quote = line.as_bytes()[start] as char;
            let Some(rest) = line.get(start + 1..) else {
                continue;
            };
            let Some(end) = rest.find(quote) else {
                continue;
            };
            let key = &rest[..end];
            if !ALLOWED_LOCALSTORAGE_KEY_PREFIXES
                .iter()
                .any(|allowed| key == *allowed || key.starts_with(allowed))
            {
                violations.push(format!(
                    "{}:{}: {key:?}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    lineno + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a new \"valori:*\" localStorage key constant was defined without being \
         added to ALLOWED_LOCALSTORAGE_KEY_PREFIXES in this test:\n{}",
        violations.join("\n")
    );
}

// ── 2. UI → filesystem (reinforces filesystem_architecture.rs) ─────────────

#[test]
fn ui_still_never_imports_the_tauri_filesystem_plugin() {
    let root = repo_root();
    let mut files = Vec::new();
    walk_files(&root.join("ui/src"), &["ts", "tsx"], &mut files);
    for path in files {
        let contents = read(&path);
        assert!(
            !contents.contains("@tauri-apps/plugin-fs"),
            "{} must not import @tauri-apps/plugin-fs",
            path.display()
        );
    }
}

// ── 3. Random module → ~/.valori/... (allowlist) ────────────────────────────

/// Every Rust source file (outside `tests/`, `examples/`, and this crate's
/// own `#[cfg(test)]` blocks) allowed to construct a `.valori` path
/// directly. Two are canonical (`valori-daemon::default_home()`,
/// `valori-studio-storage::path::default_home_dir()` — each names the
/// other in its own doc comment); the rest are pre-existing,
/// independently-legitimate fallback defaults (`valori-node`'s
/// `VALORI_MODELS_DIR`-with-fallback in both server binaries' `models_health`,
/// `valori-cli`'s setup wizard/import commands) found by this phase's audit
/// but not consolidated in S7 — see the phase doc's follow-ups.
const RUST_FILES_ALLOWED_TO_JOIN_DOT_VALORI: &[&str] = &[
    "crates/valori-daemon/src/lib.rs",
    "crates/valori-studio-storage/src/path.rs",
    "crates/valori-node/src/server.rs",
    "crates/valori-node/src/cluster_server.rs",
    "crates/valori-cli/src/commands/import.rs",
    "crates/valori-cli/src/commands/wizard.rs",
];

/// The one TypeScript file allowed to compute `$VALORI_HOME`'s default —
/// `getValoriHome()`, introduced in S7 to replace three independent
/// copies (`api-client.ts`, `connection.ts`, `cluster-config.ts`) plus two
/// `VALORI_HOME`-env-blind hardcoded fallbacks (`projects.ts`,
/// `project-adapter.ts`).
const TS_FILES_ALLOWED_TO_RESOLVE_VALORI_HOME: &[&str] = &["ui/src/lib/server/valori-home.ts"];

#[test]
fn no_new_rust_module_mints_its_own_dot_valori_path() {
    let root = repo_root();
    let mut violations = Vec::new();

    for krate_src in [
        "crates/valori-daemon/src",
        "crates/valori-studio-storage/src",
        "crates/valori-node/src",
        "crates/valori-cli/src",
        "desktop/src-tauri/src",
    ] {
        let src = root.join(krate_src);
        if !src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        walk_files(&src, &["rs"], &mut files);

        for file in files {
            let rel = file.strip_prefix(&root).unwrap_or(&file);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if RUST_FILES_ALLOWED_TO_JOIN_DOT_VALORI
                .iter()
                .any(|allowed| rel_str == *allowed)
            {
                continue;
            }
            let contents = read(&file);
            let production = match contents.find("#[cfg(test)]") {
                Some(idx) => &contents[..idx],
                None => &contents[..],
            };
            if production.contains(".join(\".valori\")") {
                violations.push(rel_str);
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a Rust file outside the allowlist constructs a \".valori\" path directly — \
         use StudioPaths (valori-studio-storage) or default_home() (valori-daemon), \
         or add it to RUST_FILES_ALLOWED_TO_JOIN_DOT_VALORI with a documented reason:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_new_typescript_module_mints_its_own_valori_home_default() {
    let root = repo_root();
    let mut files = Vec::new();
    walk_files(&root.join("ui/src"), &["ts", "tsx"], &mut files);

    let mut violations = Vec::new();
    for file in files {
        let rel = file.strip_prefix(&root).unwrap_or(&file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if TS_FILES_ALLOWED_TO_RESOLVE_VALORI_HOME
            .iter()
            .any(|allowed| rel_str == *allowed)
        {
            continue;
        }
        let contents = read(&file);
        if contents.contains("os.homedir(), \".valori\"") {
            violations.push(rel_str);
        }
    }

    assert!(
        violations.is_empty(),
        "a TypeScript file outside the allowlist computes its own $VALORI_HOME \
         default — import getValoriHome() from ui/src/lib/server/valori-home.ts \
         instead:\n{}",
        violations.join("\n")
    );
}

// ── 4. Studio metadata → project WAL/snapshot (reinforces the S6 test) ─────

#[test]
fn studio_storage_still_cannot_depend_on_project_internals() {
    let root = repo_root();
    let manifest = read(&root.join("crates/valori-studio-storage/Cargo.toml"));
    for forbidden in ["valori-kernel = {", "valori-storage = {", "valori-node = {"] {
        assert!(
            !manifest.contains(forbidden),
            "valori-studio-storage/Cargo.toml must never depend on {forbidden:?}"
        );
    }
}

// ── 5. New database → another persistence system (allowlist) ───────────────

/// Every Rust file allowed to call `Database::create(` (redb) or open a
/// new kind of embedded database at all. Three real, distinct databases
/// exist in this codebase, each with exactly one opener:
/// `studio.redb` (`valori-studio-storage::db`), `metadata.redb`
/// (`valori-metadata::db` — dormant, see its crate doc; the opener still
/// exists as prepared M3 infrastructure), and the per-shard Raft log
/// (`valori-consensus::log_store_redb`). No fourth is allowlisted.
const FILES_ALLOWED_TO_OPEN_AN_EMBEDDED_DATABASE: &[&str] = &[
    "crates/valori-studio-storage/src/db.rs",
    "crates/valori-metadata/src/db.rs",
    "crates/valori-consensus/src/log_store_redb.rs",
];

/// Database-engine crates/APIs that must never appear anywhere in this
/// workspace's source — introducing any of these would be "a new embedded
/// database," the exact thing this test exists to catch, regardless of
/// which file it's in.
const FORBIDDEN_DATABASE_ENGINES: &[&str] = &["rusqlite::", "sled::", "sqlx::", "rocksdb::"];

#[test]
fn only_the_three_known_files_open_an_embedded_redb_database() {
    let root = repo_root();
    let mut violations = Vec::new();

    for krate_src in ["crates", "desktop/src-tauri/src"] {
        let src = root.join(krate_src);
        if !src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        walk_files(&src, &["rs"], &mut files);

        for file in files {
            let rel = file.strip_prefix(&root).unwrap_or(&file);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.contains("/tests/") || rel_str.contains("/examples/") {
                continue; // test/example fixtures opening a temp db are fine
            }
            if FILES_ALLOWED_TO_OPEN_AN_EMBEDDED_DATABASE
                .iter()
                .any(|allowed| rel_str == *allowed)
            {
                continue;
            }
            let contents = read(&file);
            let production = match contents.find("#[cfg(test)]") {
                Some(idx) => &contents[..idx],
                None => &contents[..],
            };
            if production.contains("Database::create(") {
                violations.push(rel_str);
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a Rust file outside the allowlist calls Database::create() — a fourth \
         embedded database must not appear without an explicit architecture \
         decision (updating this allowlist):\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_second_database_engine_is_introduced() {
    let root = repo_root();
    let mut violations = Vec::new();

    for krate_src in ["crates", "desktop/src-tauri/src"] {
        let src = root.join(krate_src);
        if !src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        walk_files(&src, &["rs"], &mut files);

        for file in files {
            let contents = read(&file);
            for engine in FORBIDDEN_DATABASE_ENGINES {
                if contents.contains(engine) {
                    violations.push(format!(
                        "{}: references {engine:?}",
                        file.strip_prefix(&root).unwrap_or(&file).display()
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a second database engine was introduced — redb is the only embedded \
         database this workspace uses (studio.redb, metadata.redb, the Raft \
         log); do not add sqlite/sled/rocksdb/etc:\n{}",
        violations.join("\n")
    );
}
