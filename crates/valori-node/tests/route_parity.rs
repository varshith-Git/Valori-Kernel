// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Route parity — standalone vs cluster.
//!
//! CLAUDE.md documents the dual-path trap: an endpoint added to `server.rs`
//! but not `cluster_server.rs` (or vice versa) silently 404s on the other
//! path — no compile error, no test failure, only a runtime surprise.
//!
//! This test makes that class of bug a TEST FAILURE. It parses the `.route()`
//! declarations out of both source files and asserts the `/v1` route sets are
//! identical, modulo two explicit allowlists below. Adding an endpoint to one
//! router only now requires editing an allowlist here — a visible, reviewable
//! act instead of a silent gap.
//!
//! When a divergence is intentional (feature genuinely can't exist on one
//! path), add it to `STANDALONE_ONLY` / `CLUSTER_ONLY` with a comment saying
//! why. When it's a TODO, prefer wiring the missing handler instead.

use std::collections::{BTreeMap, BTreeSet};

const SERVER_SRC: &str = include_str!("../src/server.rs");
const CLUSTER_SRC: &str = include_str!("../src/cluster_server.rs");

/// Routes that exist ONLY on the standalone router, with the reason.
const STANDALONE_ONLY: &[&str] = &[
    // Raw snapshot upload writes directly into the local engine — a cluster
    // node must restore via Raft snapshot install, not an HTTP body.
    "/v1/snapshot/upload",
    // WAL streaming/replication endpoints are the standalone replication
    // primitive; cluster mode replicates through Raft instead.
    "/v1/replication/wal",
    "/v1/replication/events",
    "/v1/replication/state",
    // Execution history is served from the standalone engine's metadata DB;
    // cluster wiring is part of the planner/effect rollout (Phase A12+).
    "/v1/operations/:id/execution",
    // Object-store offload is per-node standalone ops tooling today.
    "/v1/storage/snapshots",
    "/v1/storage/snapshots/upload",
    "/v1/storage/snapshots/restore",
    "/v1/storage/wal",
    "/v1/storage/wal/archive",
];

/// Routes that exist ONLY on the cluster router, with the reason.
const CLUSTER_ONLY: &[&str] = &[
    // Cluster-wide proof aggregation only makes sense with peers.
    "/v1/cluster/proof",
];

/// (path, method) pairs where ONE side intentionally serves an extra method.
/// Key = path, value = methods to ignore when comparing. Empty since Phase R2
/// closed the `DELETE /v1/graph/node/:id` gap (cluster now commits
/// `KernelEvent::DeleteNode` via Raft).
const METHOD_GAPS: &[(&str, &str)] = &[];

/// Extract `(path, methods)` from every single-line `.route("…", …)` call.
/// Route declarations in both files are one per line by convention; if that
/// ever changes, the count sanity-checks below will catch a parser miss.
fn extract_routes(src: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in src.lines() {
        let Some(idx) = line.find(".route(\"") else { continue };
        let rest = &line[idx + ".route(\"".len()..];
        let Some(end) = rest.find('"') else { continue };
        let path = &rest[..end];
        let handler_part = &rest[end..];

        let mut methods = BTreeSet::new();
        for m in ["get", "post", "delete", "put", "patch"] {
            let needle = format!("{m}(");
            let mut search = 0;
            while let Some(pos) = handler_part[search..].find(&needle) {
                let abs = search + pos;
                // Word boundary: preceding char must not be part of an ident
                // (avoids matching `budget(` etc.).
                let boundary = abs == 0
                    || !handler_part.as_bytes()[abs - 1].is_ascii_alphanumeric()
                        && handler_part.as_bytes()[abs - 1] != b'_';
                if boundary {
                    methods.insert(m.to_string());
                    break;
                }
                search = abs + needle.len();
            }
        }
        out.entry(path.to_string()).or_default().extend(methods);
    }
    out
}

fn v1_only(routes: &BTreeMap<String, BTreeSet<String>>) -> BTreeMap<String, BTreeSet<String>> {
    routes
        .iter()
        .filter(|(p, _)| p.starts_with("/v1/"))
        .map(|(p, m)| (p.clone(), m.clone()))
        .collect()
}

#[test]
fn v1_route_sets_match_between_standalone_and_cluster() {
    let standalone = v1_only(&extract_routes(SERVER_SRC));
    let cluster = v1_only(&extract_routes(CLUSTER_SRC));

    // Parser sanity: both files declare a substantial v1 surface. If these
    // fire, the extraction regex no longer matches the source formatting.
    assert!(standalone.len() > 40, "parser found only {} standalone v1 routes", standalone.len());
    assert!(cluster.len() > 40, "parser found only {} cluster v1 routes", cluster.len());

    let standalone_only: BTreeSet<&str> = STANDALONE_ONLY.iter().copied().collect();
    let cluster_only: BTreeSet<&str> = CLUSTER_ONLY.iter().copied().collect();

    // Allowlist staleness: every entry must still exist on its own side and
    // must NOT have appeared on the other side (if it did, remove the entry).
    for p in &standalone_only {
        assert!(standalone.contains_key(*p), "STANDALONE_ONLY entry {p} no longer exists in server.rs — remove it");
        assert!(!cluster.contains_key(*p), "{p} is in STANDALONE_ONLY but cluster_server.rs now serves it — remove the allowlist entry");
    }
    for p in &cluster_only {
        assert!(cluster.contains_key(*p), "CLUSTER_ONLY entry {p} no longer exists in cluster_server.rs — remove it");
        assert!(!standalone.contains_key(*p), "{p} is in CLUSTER_ONLY but server.rs now serves it — remove the allowlist entry");
    }

    let shared_standalone: BTreeSet<&String> = standalone
        .keys()
        .filter(|p| !standalone_only.contains(p.as_str()))
        .collect();
    let shared_cluster: BTreeSet<&String> = cluster
        .keys()
        .filter(|p| !cluster_only.contains(p.as_str()))
        .collect();

    let missing_in_cluster: Vec<_> = shared_standalone.difference(&shared_cluster).collect();
    let missing_in_standalone: Vec<_> = shared_cluster.difference(&shared_standalone).collect();

    assert!(
        missing_in_cluster.is_empty() && missing_in_standalone.is_empty(),
        "v1 route divergence detected.\n\
         Routes in server.rs missing from cluster_server.rs: {missing_in_cluster:?}\n\
         Routes in cluster_server.rs missing from server.rs: {missing_in_standalone:?}\n\
         Either add the endpoint to the other router (see CLAUDE.md § single-node AND multi-node)\n\
         or add it to STANDALONE_ONLY / CLUSTER_ONLY in tests/route_parity.rs with a reason."
    );
}

#[test]
fn v1_route_methods_match_between_standalone_and_cluster() {
    let standalone = v1_only(&extract_routes(SERVER_SRC));
    let cluster = v1_only(&extract_routes(CLUSTER_SRC));

    let standalone_only: BTreeSet<&str> = STANDALONE_ONLY.iter().copied().collect();
    let cluster_only: BTreeSet<&str> = CLUSTER_ONLY.iter().copied().collect();
    let gaps: BTreeMap<&str, BTreeSet<&str>> = METHOD_GAPS.iter().fold(
        BTreeMap::new(),
        |mut acc, (p, m)| {
            acc.entry(*p).or_default().insert(*m);
            acc
        },
    );

    let mut divergences = Vec::new();
    for (path, s_methods) in &standalone {
        if standalone_only.contains(path.as_str()) || cluster_only.contains(path.as_str()) {
            continue;
        }
        let Some(c_methods) = cluster.get(path) else { continue }; // path parity covered above
        let ignore = gaps.get(path.as_str());
        let filt = |ms: &BTreeSet<String>| -> BTreeSet<String> {
            ms.iter()
                .filter(|m| ignore.is_none_or(|ig| !ig.contains(m.as_str())))
                .cloned()
                .collect()
        };
        let (s, c) = (filt(s_methods), filt(c_methods));
        if s != c {
            divergences.push(format!("{path}: standalone={s:?} cluster={c:?}"));
        }
    }
    assert!(
        divergences.is_empty(),
        "v1 method divergence detected:\n{}\nEither align the routers or add an entry to METHOD_GAPS with a reason.",
        divergences.join("\n")
    );
}
