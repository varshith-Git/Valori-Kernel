// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Architecture tripwire — no duplicate source files across crate boundaries.
//!
//! History: the Phase 1.1 workspace restructure moved the storage layer into
//! `valori-storage` and the recovery/bootstrap layer into `valori-state`, and
//! `valori-node/src/lib.rs` re-exported them so old import paths kept
//! compiling. The original files were left behind in `valori-node/src/` as
//! dead copies — never declared as modules, invisible to the compiler, and
//! silently drifting from the live versions (recovery.rs had already
//! diverged when this was caught). Anyone reading `use crate::wal_writer` in
//! engine.rs naturally assumed the local file was the live one.
//!
//! This test makes that failure mode a TEST FAILURE: a source file with the
//! same crate-relative path must not exist in both `valori-node` and any of
//! the extracted crates. When a subsystem moves to its own crate, the old
//! file must be deleted in the same change — leaving it "for reference" is
//! how the drift starts.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Crates whose `src/` trees must not share file paths with valori-node's.
/// Relative to the workspace root (parent of this crate's manifest dir).
const EXTRACTED_CRATES: &[&str] = &[
    "crates/valori-storage",
    "crates/valori-state",
    "crates/valori-metadata",
];

/// Crate-relative paths that may legitimately exist on both sides.
/// `lib.rs` is structural; add others only with a written reason.
const ALLOWED_COLLISIONS: &[&str] = &["lib.rs"];

/// Recursively collect all `.rs` paths under `dir`, relative to `dir`.
fn collect_rs(dir: &Path, prefix: &Path, out: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = prefix.join(entry.file_name());
        if path.is_dir() {
            collect_rs(&path, &rel, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.insert(rel);
        }
    }
}

#[test]
fn no_duplicate_source_files_across_extracted_crates() {
    let node_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("valori-node sits two levels below the workspace root");

    let mut node_files = BTreeSet::new();
    collect_rs(&node_src, Path::new(""), &mut node_files);
    assert!(
        node_files.len() > 20,
        "parser sanity: found only {} .rs files under valori-node/src",
        node_files.len()
    );

    let allowed: BTreeSet<PathBuf> = ALLOWED_COLLISIONS.iter().map(PathBuf::from).collect();

    let mut collisions = Vec::new();
    for krate in EXTRACTED_CRATES {
        let other_src = workspace_root.join(krate).join("src");
        assert!(
            other_src.is_dir(),
            "{krate}/src no longer exists — update EXTRACTED_CRATES"
        );
        let mut other_files = BTreeSet::new();
        collect_rs(&other_src, Path::new(""), &mut other_files);

        for path in node_files.intersection(&other_files) {
            if !allowed.contains(path) {
                collisions.push(format!(
                    "{} exists in both valori-node and {krate}",
                    path.display()
                ));
            }
        }
    }

    assert!(
        collisions.is_empty(),
        "duplicate source files across crate boundaries — the valori-node copy \
         is almost certainly dead code left behind by an extraction; delete it \
         (the extracted crate's version is the live one, re-exported via lib.rs):\n{}",
        collisions.join("\n")
    );
}
