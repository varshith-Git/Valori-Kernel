// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Architecture tripwire — the crate dependency graph is mechanically enforced.
//!
//! `architecture.rs` guards against duplicate *source files* across crates.
//! This file guards the *dependency edges* between them. The two failure modes
//! are unrelated: a clean file layout with a reversed dependency is still a
//! broken architecture.
//!
//! ## Why this exists
//!
//! `docs/architecture/layers.md` and `rfcs/0005-crate-boundaries.md` describe
//! the allowed dependency directions in prose. Prose does not fail CI. The
//! `ARCHITECTURE_AUDIT.md` (Stage 1) found that nothing in the repository
//! prevented a reversed edge from shipping — `deny.toml` has no layer rule and
//! `architecture.rs` only compares file paths.
//!
//! This test is Stage-2 step M0, and it runs **before** `valori-domain` is
//! introduced on purpose: the guard has to predate the thing it guards, or the
//! first violation ships unnoticed.
//!
//! ## What is enforced
//!
//! 1. **Acyclic** — the shipped dependency graph has no cycles.
//! 2. **Sealed crates** — `valori-core`, `valori-kernel` and `valori-domain`
//!    may depend on an explicit allowlist and nothing else.
//! 3. **Domain firewall** — the determinism-critical crates (kernel, wire,
//!    storage, state, index, rag, verify) must not reach `valori-domain`, even
//!    transitively. `valori-domain` is std-only platform vocabulary; letting it
//!    into the snapshot/WAL/event-log crates would couple wire compatibility
//!    (`COMPATIBILITY.md`) to product-level types.
//! 4. **No Cloud in OSS** — no crate in this workspace may depend on a
//!    `valori-cloud-*` crate, and Cloud-only identity concepts may not be
//!    *defined* in the OSS platform core.
//!
//! ## Dev-dependencies are deliberately excluded
//!
//! Dev-dependencies do not ship, and two back-edges exist on purpose:
//! `valori-state → valori-verify` and `valori-verify → valori-node` (the
//! cross-crate wire-compat test must link both sides to prove node-written
//! bytes decode with the verifier's mirror). Including dev-deps would report a
//! cycle that has no runtime meaning.
//!
//! ## Changing the rules
//!
//! These constants are an architectural contract, not configuration. Widening
//! an allowlist or removing a firewall entry requires a written reason here and
//! a corresponding update to `docs/architecture/layers.md`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

// ── The contract ──────────────────────────────────────────────────────────────

/// Crates whose dependency list is closed: they may depend on exactly these
/// crates and no other workspace crate.
///
/// - `valori-core` is the zero-dependency foundation. Anything added here is
///   inherited by every crate in the workspace, including the `no_std` kernel.
/// - `valori-kernel` is the portability moat (invariant #7 in `CLAUDE.md`). It
///   must stay buildable for `wasm32-unknown-unknown` and embedded targets.
/// - `valori-domain` is platform vocabulary shared by daemon / node / models /
///   Cloud. It must stay leaf-ward: if it ever depends on `valori-node` or
///   `valori-daemon`, it stops being a contract and becomes an application.
/// - `valori-studio-storage` (Studio S1, `docs/architecture/studio-storage.md`)
///   is `desktop/src-tauri`'s local metadata store (`studio.redb`). It must
///   stay leaf-ward for the same reason `valori-domain` does: depending on
///   `valori-daemon`, `valori-node`, `valori-metadata` or `valori-consensus`
///   would either create a cycle (those crates never depend on a
///   desktop-only concern) or silently couple Studio's local bookkeeping to
///   control-plane/consensus internals it has no business touching. It may
///   depend on `valori-domain` for shared identity types (`ProjectId`,
///   `SessionId`, `InstallationId`) and nothing else.
const SEALED_CRATES: &[(&str, &[&str])] = &[
    ("valori-core", &[]),
    ("valori-kernel", &["valori-core"]),
    ("valori-domain", &["valori-core"]),
    ("valori-studio-storage", &["valori-domain"]),
];

/// Crates that must not reach `valori-domain`, transitively included.
///
/// These are the crates that define or consume the on-disk and on-wire formats
/// whose compatibility is frozen by `COMPATIBILITY.md`: snapshot V6, event log
/// V4, and the BLAKE3 audit chain. `valori-domain` carries product concepts
/// (Project, Model, Runtime) that must never influence those bytes.
///
/// `valori-metadata`, `valori-planner` and `valori-effect` are intentionally
/// **absent** — the control plane is a legitimate future consumer of the
/// canonical domain model (Stage-2 step M3).
const DOMAIN_FIREWALL: &[&str] = &[
    "valori-core",
    "valori-kernel",
    "valori-wire",
    "valori-storage",
    "valori-state",
    "valori-index",
    "valori-rag",
    "valori-verify",
];

/// Identity concepts that belong to the private Cloud control plane and must
/// not be *defined* in the OSS platform core.
///
/// A local Studio project has no organization, no user, no billing account and
/// no hosted deployment. Defining these here would make the open-source kernel
/// carry commercial vocabulary for zero open-source benefit (hard rule 8 of the
/// platform brief; §10 and §17 of `ARCHITECTURE_AUDIT.md`).
const CLOUD_ONLY_CONCEPTS: &[&str] = &[
    "OrganizationId",
    "UserId",
    "BillingAccountId",
    "SubscriptionId",
    "DeploymentId",
    "WorkerId",
];

/// Crates the Cloud-concept ban applies to.
const OSS_PLATFORM_CORE: &[&str] = &[
    "valori-core",
    "valori-kernel",
    "valori-domain",
    "valori-studio-storage",
];

/// Edges that must exist. A parser that silently stops matching would make
/// every other assertion in this file vacuously true; these keep it honest.
const EXPECTED_EDGES: &[(&str, &str)] = &[
    ("valori-kernel", "valori-core"),
    ("valori-wire", "valori-kernel"),
    ("valori-storage", "valori-kernel"),
    ("valori-state", "valori-storage"),
    ("valori-planner", "valori-metadata"),
    ("valori-effect", "valori-planner"),
    ("valori-node", "valori-effect"),
    ("valori-studio-storage", "valori-domain"),
];

// ── Manifest parsing ──────────────────────────────────────────────────────────

/// The workspace-dependency edges of one crate, split by whether they ship.
struct Manifest {
    /// `[dependencies]`, `[target.'cfg(..)'.dependencies]`, `[build-dependencies]`.
    shipped: BTreeSet<String>,
    /// `[dev-dependencies]` — excluded from every graph assertion. See module docs.
    #[allow(dead_code)]
    dev: BTreeSet<String>,
}

/// Which kind of dependency table a `[section]` header introduces, if any.
fn dependency_table_kind(header: &str) -> Option<&'static str> {
    // Handles `[dependencies]` and `[target.'cfg(target_os = "macos")'.dependencies]`.
    if header.ends_with("dev-dependencies") {
        Some("dev")
    } else if header.ends_with("build-dependencies") || header.ends_with("dependencies") {
        Some("shipped")
    } else {
        None
    }
}

/// Extract the workspace-crate key from a dependency line, e.g.
/// `valori-kernel = { workspace = true }` → `valori-kernel`.
fn workspace_dep_key(line: &str) -> Option<String> {
    let (key, _) = line.split_once('=')?;
    let key = key.trim().trim_matches('"');
    if key.starts_with("valori-") && key.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        Some(key.to_string())
    } else {
        None
    }
}

fn parse_manifest(path: &Path) -> Manifest {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut shipped = BTreeSet::new();
    let mut dev = BTreeSet::new();
    let mut table: Option<&'static str> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            // `[[bin]]` leaves a stray bracket; it is not a dependency table either way.
            table = dependency_table_kind(header.trim_matches('[').trim_matches(']'));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(kind) = table else { continue };
        if let Some(dep) = workspace_dep_key(line) {
            match kind {
                "dev" => dev.insert(dep),
                _ => shipped.insert(dep),
            };
        }
    }

    Manifest { shipped, dev }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("valori-node sits two levels below the workspace root")
        .to_path_buf()
}

/// Every `crates/*/Cargo.toml`, keyed by crate directory name.
fn workspace_graph() -> BTreeMap<String, Manifest> {
    let crates_dir = workspace_root().join("crates");
    let mut graph = BTreeMap::new();
    for entry in std::fs::read_dir(&crates_dir)
        .expect("crates/ must exist")
        .flatten()
    {
        let manifest = entry.path().join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        graph.insert(name, parse_manifest(&manifest));
    }
    graph
}

/// Crates reachable from `start` through shipped edges, excluding `start`.
fn reachable(graph: &BTreeMap<String, Manifest>, start: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    if let Some(m) = graph.get(start) {
        queue.extend(m.shipped.iter().cloned());
    }
    while let Some(node) = queue.pop_front() {
        if !seen.insert(node.clone()) {
            continue;
        }
        if let Some(m) = graph.get(&node) {
            queue.extend(m.shipped.iter().cloned());
        }
    }
    seen
}

/// The shipped path from `start` to `target`, for a readable failure message.
fn path_to(graph: &BTreeMap<String, Manifest>, start: &str, target: &str) -> Option<Vec<String>> {
    let mut queue = VecDeque::from([vec![start.to_string()]]);
    let mut seen = BTreeSet::from([start.to_string()]);
    while let Some(path) = queue.pop_front() {
        let tail = path.last().expect("paths are never empty");
        if tail == target {
            return Some(path);
        }
        let Some(m) = graph.get(tail) else { continue };
        for dep in &m.shipped {
            if seen.insert(dep.clone()) {
                let mut next = path.clone();
                next.push(dep.clone());
                queue.push_back(next);
            }
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn parser_sees_the_workspace() {
    let graph = workspace_graph();
    assert!(
        graph.len() >= 20,
        "parser sanity: found only {} crates under crates/ — the manifest parser \
         is probably broken, which would make every other assertion in this file \
         vacuously pass",
        graph.len()
    );

    for (from, to) in EXPECTED_EDGES {
        let manifest = graph
            .get(*from)
            .unwrap_or_else(|| panic!("{from} not found under crates/"));
        assert!(
            manifest.shipped.contains(*to),
            "parser sanity: expected shipped edge {from} → {to} was not found. \
             Either the dependency was genuinely removed (update EXPECTED_EDGES \
             with a reason) or the manifest parser stopped matching."
        );
    }

    // The two intentional dev-only back-edges must stay dev-only. If either is
    // ever promoted to a shipped dependency, the graph gains a real cycle.
    for (krate, dep) in [
        ("valori-state", "valori-verify"),
        ("valori-verify", "valori-node"),
    ] {
        if let Some(m) = graph.get(krate) {
            assert!(
                !m.shipped.contains(dep),
                "{krate} → {dep} must remain a dev-dependency. Promoting it to a \
                 shipped dependency creates a cycle (see module docs)."
            );
        }
    }
}

#[test]
fn shipped_dependency_graph_is_acyclic() {
    let graph = workspace_graph();
    let mut cycles = Vec::new();

    for name in graph.keys() {
        if reachable(&graph, name).contains(name) {
            let via = graph
                .get(name)
                .map(|m| {
                    m.shipped
                        .iter()
                        .filter(|d| *d == name || reachable(&graph, d).contains(name))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            cycles.push(format!("{name} is reachable from itself via: {via}"));
        }
    }

    assert!(
        cycles.is_empty(),
        "the shipped dependency graph must stay acyclic (hard rule 14):\n{}",
        cycles.join("\n")
    );
}

#[test]
fn sealed_crates_depend_only_on_their_allowlist() {
    let graph = workspace_graph();
    let mut violations = Vec::new();

    for (krate, allowed) in SEALED_CRATES {
        // Sealed crates that do not exist yet (valori-domain before M1) are
        // skipped, not failed — the guard is allowed to predate the crate.
        let Some(manifest) = graph.get(*krate) else {
            continue;
        };
        let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
        for dep in &manifest.shipped {
            if !allowed.contains(dep.as_str()) {
                violations.push(format!(
                    "{krate} → {dep} is not on {krate}'s allowlist ({:?})",
                    allowed
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a sealed crate gained a dependency outside its allowlist. These \
         boundaries are load-bearing: valori-core is inherited by everything, \
         valori-kernel must stay no_std/wasm-buildable, and valori-domain must \
         stay leaf-ward. Widening an allowlist requires a written reason in \
         dependency_direction.rs and docs/architecture/layers.md:\n{}",
        violations.join("\n")
    );
}

#[test]
fn determinism_crates_cannot_reach_valori_domain() {
    let graph = workspace_graph();
    if !graph.contains_key("valori-domain") {
        // M0 ships before M1; nothing to check until the crate exists.
        return;
    }

    let mut violations = Vec::new();
    for krate in DOMAIN_FIREWALL {
        if !graph.contains_key(*krate) {
            continue;
        }
        if reachable(&graph, krate).contains("valori-domain") {
            let path = path_to(&graph, krate, "valori-domain")
                .map(|p| p.join(" → "))
                .unwrap_or_else(|| format!("{krate} → … → valori-domain"));
            violations.push(path);
        }
    }

    assert!(
        violations.is_empty(),
        "a determinism-critical crate reached valori-domain. These crates own \
         the snapshot, WAL, event-log and audit-chain formats whose \
         compatibility is frozen by COMPATIBILITY.md; product vocabulary must \
         never influence those bytes:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_oss_crate_depends_on_cloud() {
    let graph = workspace_graph();
    let mut violations = Vec::new();

    for (name, manifest) in &graph {
        for dep in &manifest.shipped {
            if dep.starts_with("valori-cloud") {
                violations.push(format!("{name} → {dep}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "an open-source crate depends on a Cloud crate. The Cloud control \
         plane (provisioning, billing, scheduler, hosted inference) lives in a \
         private repository and depends on OSS contracts, never the reverse:\n{}",
        violations.join("\n")
    );
}

#[test]
fn cloud_only_concepts_are_not_defined_in_oss_platform_core() {
    let root = workspace_root();
    let mut violations = Vec::new();

    for krate in OSS_PLATFORM_CORE {
        let src = root.join("crates").join(krate).join("src");
        if !src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect_rs_files(&src, &mut files);

        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            for (lineno, line) in text.lines().enumerate() {
                let line = line.trim();
                // Only definitions count. Mentioning a Cloud concept in a doc
                // comment (as this crate's own docs do) is fine and useful.
                let defines = line.starts_with("pub struct ")
                    || line.starts_with("struct ")
                    || line.starts_with("pub enum ")
                    || line.starts_with("enum ")
                    || line.starts_with("pub type ")
                    || line.starts_with("type ");
                if !defines {
                    continue;
                }
                for concept in CLOUD_ONLY_CONCEPTS {
                    if line.contains(concept) {
                        violations.push(format!(
                            "{}:{} defines `{concept}`",
                            file.strip_prefix(&root).unwrap_or(&file).display(),
                            lineno + 1
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a Cloud-only identity concept is defined in the OSS platform core. \
         Organizations, users, billing accounts, subscriptions, deployments and \
         workers belong to the private Cloud control plane — a local Studio \
         project has none of them (ARCHITECTURE_AUDIT.md §10, §17):\n{}",
        violations.join("\n")
    );
}

/// S7 (`docs/phases/phase-studio-S7-persistence-boundary.md`): `metadata.redb`
/// is deliberately dormant — see `valori-metadata`'s own crate-level doc
/// comment for the full decision. Reactivating it in a real binary must be a
/// conscious architecture decision (updating/removing this test), never an
/// accidental side effect of some unrelated change wiring `MetadataDb::open`
/// in without reconciling it against `valori-daemon`'s `project.json` and
/// `valori-studio-storage`'s `projects` table, which remain the sole
/// authorities for project identity today.
#[test]
fn metadata_db_open_stays_out_of_production_binaries() {
    let root = workspace_root();
    let mut violations = Vec::new();

    for krate_src in [
        "crates/valori-node/src",
        "crates/valori-daemon/src",
        "desktop/src-tauri/src",
    ] {
        let src = root.join(krate_src);
        if !src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect_rs_files(&src, &mut files);

        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            // Strip test modules — a test exercising MetadataDb directly
            // (e.g. valori-metadata's own crate) is fine; this check is
            // about production call sites only.
            let production = match text.find("#[cfg(test)]") {
                Some(idx) => &text[..idx],
                None => &text[..],
            };
            for (lineno, line) in production.lines().enumerate() {
                if line.contains("MetadataDb::open(") {
                    violations.push(format!(
                        "{}:{}",
                        file.strip_prefix(&root).unwrap_or(&file).display(),
                        lineno + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "MetadataDb::open() now appears in a production binary — this is a \
         deliberate architecture decision (see valori-metadata's crate doc \
         and docs/phases/phase-studio-S7-persistence-boundary.md), not \
         something that should happen without reconciling Project/Collection \
         identity against valori-daemon's project.json and \
         valori-studio-storage's projects table first. Update this test \
         (and the crate doc) once that reconciliation is actually done:\n{}",
        violations.join("\n")
    );
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
