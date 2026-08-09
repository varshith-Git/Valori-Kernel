// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Architecture tripwire — Studio S5 (Session Retention).
//!
//! Same source-text-scanning technique as `installation_id_architecture.rs`
//! and `credential_security_architecture.rs` (this crate's internal modules
//! are private, so an integration test here cannot call into `lib.rs`'s
//! `setup()` closure directly — it isn't unit-testable in isolation, since
//! it needs a running Tauri app context). What *can* be verified
//! mechanically is that the call site never turns a pruning failure into a
//! startup failure.
//!
//! See `docs/phases/phase-studio-S5-session-retention.md`.

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

/// `prune_sessions`'s result must be handled with `match`/`if let`, not
/// `.unwrap()`/`.expect()`/`?` — any of those would turn a pruning failure
/// into a startup panic or an early-return out of `setup()`, exactly what
/// the S5 task's "pruning failure does not prevent desktop startup"
/// requirement forbids.
#[test]
fn prune_sessions_call_site_never_unwraps_or_propagates_its_result() {
    let root = repo_root();
    let contents = read_production_code(&root.join("desktop/src-tauri/src/lib.rs"));

    let call_idx = contents
        .find("session_service.prune_sessions(")
        .expect("lib.rs must call SessionService::prune_sessions at startup");

    // Look at a window right after the call for the handling pattern —
    // covers `match session_service.prune_sessions(...) {` as well as any
    // reasonable reformatting, without being so wide it'd match unrelated
    // code elsewhere in the file.
    let window_start = contents[..call_idx].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let window = &contents[window_start..(call_idx + 400).min(contents.len())];

    assert!(
        window.contains("match") || window.contains("if let"),
        "prune_sessions's result must be handled with match/if let, not unwrapped: {window}"
    );
    assert!(
        !window.contains("prune_sessions(") || !window.contains(").unwrap()"),
        "prune_sessions's result must never be .unwrap()'d — a failure must not panic startup"
    );
    assert!(
        !window.contains("prune_sessions(") || !window.contains(").expect("),
        "prune_sessions's result must never be .expect()'d"
    );
}

/// The pruning failure branch must only log — it must never call back into
/// `init_studio_storage`/recovery, and must never call `std::process::exit`
/// or `panic!`. A textual guard: within the code immediately following the
/// `prune_sessions` call, the only executable actions in an error arm
/// should be `tracing::warn!`.
#[test]
fn prune_sessions_failure_path_only_logs_never_recovers_or_exits() {
    let root = repo_root();
    let contents = read_production_code(&root.join("desktop/src-tauri/src/lib.rs"));

    let call_idx = contents
        .find("session_service.prune_sessions(")
        .expect("lib.rs must call SessionService::prune_sessions at startup");
    // A generous window covering the whole match arm block that follows.
    let block = &contents[call_idx..(call_idx + 800).min(contents.len())];

    assert!(
        block.contains("Err(e)"),
        "must have an explicit Err(e) arm: {block}"
    );
    assert!(
        block.contains("tracing::warn!"),
        "the failure arm must log via tracing::warn!: {block}"
    );
    for forbidden in [
        "init_studio_storage",
        "open_with_recovery",
        "std::process::exit",
        "panic!",
    ] {
        assert!(
            !block.contains(forbidden),
            "prune_sessions's failure handling must never call {forbidden:?}: {block}"
        );
    }
}

/// `prune_sessions` must run before `start_session` in `lib.rs`'s startup
/// sequence — see `SessionStore::prune`'s doc comment for why this
/// ordering, not `start_session`-then-`prune_sessions`, matches the S5
/// phase's documented lifecycle (DB open → installation identity → crash
/// reconciliation → prune → start current session).
#[test]
fn prune_sessions_runs_before_start_session_in_setup() {
    let root = repo_root();
    let contents = read_production_code(&root.join("desktop/src-tauri/src/lib.rs"));

    let prune_idx = contents
        .find("session_service.prune_sessions(")
        .expect("lib.rs must call prune_sessions");
    let start_idx = contents
        .find("session_service.start_session(")
        .expect("lib.rs must call start_session");

    assert!(
        prune_idx < start_idx,
        "prune_sessions must be called before start_session in lib.rs's setup()"
    );
}

/// `reconcile_crashed_sessions` must run before `prune_sessions` — pruning
/// should see accurate crashed/completed state, not sessions still open
/// from a prior crashed run.
#[test]
fn reconcile_crashed_sessions_runs_before_prune_sessions() {
    let root = repo_root();
    let contents = read_production_code(&root.join("desktop/src-tauri/src/lib.rs"));

    let reconcile_idx = contents
        .find("session_service.reconcile_crashed_sessions(")
        .expect("lib.rs must call reconcile_crashed_sessions");
    let prune_idx = contents
        .find("session_service.prune_sessions(")
        .expect("lib.rs must call prune_sessions");

    assert!(
        reconcile_idx < prune_idx,
        "reconcile_crashed_sessions must run before prune_sessions"
    );
}
