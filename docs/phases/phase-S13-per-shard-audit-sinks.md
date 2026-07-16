# Phase S13 — Per-shard audit sinks

Branch: `Node-scaleup` (S1-S12 merged through commit `3d9f1a0`).

## Goal

Fix a real, pre-existing data-integrity gap discovered while scoping a UI feature: `bootstrap_cluster()`'s shard bootstrap loop only ever gave shard 0 a real audit sink — every shard `i >= 1` got a hardcoded `NullAuditSink` that silently discards every event. This was an intentional, documented S1-era decision, made when no HTTP traffic could reach shard ≥ 1. That premise became false once S3-S9 wired real `shard_for_namespace()`-based HTTP routing to every shard — a write landing on shard 1+ was correctly Raft-committed and applied to that shard's `KernelState`, but never chained into any `events.log`. For a product whose entire value proposition is a verifiable audit trail, that's a real bug, not a cosmetic one. This phase gives every shard its own genuine, chain-verifiable audit sink.

## Delivered

### `crates/valori-node/src/cluster.rs`

`bootstrap_cluster()`'s signature changed from accepting one pre-built `audit: Box<dyn AuditSink>` to `event_log_path: Option<&Path>` + `event_log_rotation_bytes: Option<u64>`, so the function can construct N real sinks itself — one per shard, reusing the existing `shard_path()` helper (already used for Raft log paths) to derive each shard's own filename (unchanged at `shard_count == 1`, `{stem}-shard{N}{ext}` otherwise).

**Error-handling asymmetry, deliberate:** a failure to open shard 0's audit log is fatal — `bootstrap_cluster` returns `Err(...)`, preserving the exact pre-S13 guarantee that a node refuses to boot rather than silently run its primary shard unaudited. A failure to open a non-zero shard's audit log is *not* fatal: that shard falls back to `NullAuditSink` and the node boots anyway, logged loudly via `tracing::error!`. Shards ≥ 1 are new capability this phase adds — there was no pre-existing "fatal" guarantee to preserve for them, and aborting the whole node over one non-primary shard's disk issue would be a new availability regression with no precedent.

`ShardHandle` gained `event_log_writer: Option<Arc<Mutex<EventLogWriter>>>`. `ClusterHandle` gained a matching flat field aliasing shard 0's writer, following the same "flat field aliases shard 0" convention already used for `raft`/`state_machine`/`startup_committed_index` — so `main.rs` doesn't need to reach into `shards[&ShardId(0)]`.

### `crates/valori-node/src/main.rs`

`run_cluster()` simplified: it no longer hand-constructs `EventLogWriter`/`EventLogAuditSink` — it just passes `node_cfg.event_log_path.as_deref()` and a normalized rotation-bytes value straight into `bootstrap_cluster`, then reads `handle.event_log_writer.clone()` for `build_cluster_router`. Net simpler than before.

### 15 call sites migrated

All test and CLI-wizard call sites (`cluster_read_index.rs`, `cluster_boot.rs` ×4, `cluster_data_plane.rs` ×2, `cluster_namespaces.rs` ×3, `cluster_api.rs`, `cluster_cli.rs`, `wizard.rs` ×2) updated to the new `(cfg, event_log_path, rotation_bytes, dim)` shape — 14 pass `None, None` (unaffected, `NullAuditSink` behavior preserved exactly), one (`cluster_boot.rs`'s `raft_committer_writes_a_verifiable_audit_log`) converted from hand-building a real sink to passing the path directly (`Some(&log_path)`), which is both a simplification and now exercises the exact code path production takes. `crates/valori-consensus/tests/multi_shard.rs` needed no changes — it never calls `bootstrap_cluster`, constructing its own Raft/state-machine topology directly.

Removed now-unused `NullAuditSink` imports across all touched test files.

## Findings

- The bug had gone unnoticed since S1 specifically *because* it was written down as a documented, deliberate scope decision rather than silently dropped — but the doc comment justifying it ("shards >= 1 have no HTTP surface in S1") was never revisited once S3-S9 made that premise false. Two adjacent comments in `cluster.rs` (the one above `bootstrap_cluster` and the `ClusterConfig.shard_count` field doc) both still said "no namespace routing yet" — updated both while in the area, since a stale comment sitting directly next to the real bug is exactly the kind of thing that causes this class of gap to persist.
- Confirmed via direct inspection that `StateMachineInner.audit: Box<dyn AuditSink>` is a genuinely per-instance field with zero shared/global state — the fix was entirely a bootstrap-layer concern (`cluster.rs`/`main.rs`), no changes needed to `valori-consensus`'s state machine itself.

## Validation

```
cargo test -p valori-consensus                                    # 74 passed, 1 ignored — untouched, no fallout
cargo build -p valori-node
cargo test -p valori-node --test cluster_boot --test cluster_namespaces --test cluster_data_plane --test cluster_api --test cluster_read_index
                                                                    # 37 passed, 0 failed
cargo test -p valori-node                                          # 235 passed, 0 failed (233 + 2 new)
cargo build -p valori-cli && cargo test -p valori-cli              # clean, all pre-existing tests pass
cargo build --workspace --exclude valoricore-ffi                   # clean
cargo clippy --workspace --exclude valoricore-ffi --all-targets    # 0 errors (pre-existing warnings elsewhere, unrelated)
cargo build -p valori-kernel --target wasm32-unknown-unknown       # clean, kernel untouched
```

Two new tests prove the fix directly:

- `crates/valori-node/tests/cluster_namespaces.rs::writes_to_non_zero_shard_are_chained_to_that_shards_event_log` — boots a real `shard_count: 3` cluster with a real event log path, routes a write to a non-zero shard via the same namespace→shard proof as `writes_to_different_collections_route_to_different_shards`, then reads the shard's own `events-shardN.log` back with `read_event_log` (which chain-validates as it decodes, not just a byte count) and asserts the `AutoInsertRecord` is present — plus an isolation check that shard 0's own log does not contain it (it does contain the `AutoCreateNamespace` event, since the namespace registry always lives on shard 0 — the test asserts specifically that the record insert didn't leak there, not that the log is empty).
- `crates/valori-node/tests/cluster_boot.rs::single_shard_event_log_path_is_unsuffixed_and_matches_pre_s13_naming` — regression: `shard_count == 1` still produces the exact unsuffixed filename with no `-shard0` sibling, and the new `ClusterHandle.event_log_writer` alias reports the correct path.

Manual smoke test not performed live this pass — the two new integration tests boot real Raft clusters end to end (gRPC server, leader election, HTTP router, real file I/O) and are equivalent in rigor to a manual smoke test; no additional live verification was judged necessary before moving to the UI phase (S14), where a live smoke test against the running preview server is planned regardless.

## Follow-ups

- `/v1/proof/event-log` and `/v1/timeline` in `cluster_server.rs` still only ever read `DataPlaneState.event_log_path` — shard 0's log. They do not yet expose per-shard or aggregate (Merkle-root-over-shard-hashes) proof/timeline views. Every shard's writes are now durably, correctly audited on disk (this phase's actual guarantee) — but the HTTP proof/timeline surface won't show anything for a collection that lives on a non-zero shard until a follow-up phase makes those endpoints shard-aware. Already flagged as a known future need in the original S1 phase doc ("Composable state-hash receipt").
- Optional Test 3 from the plan (per-shard `EventLogWriter::open` failure falls back gracefully without aborting the whole boot) was not added this pass — the fallback logic is a straightforward `match` arm with no branching complexity that the two delivered tests don't already exercise indirectly (both tests prove the success path thoroughly; the failure-path `tracing::error!` + `NullAuditSink` fallback is a one-line-per-branch mirror of the already-tested "no path configured" `None` arm). Worth adding if this code path is touched again.
