# Phase B13 — Snapshot Cadence & Startup Readiness Gate

## Goal

Fix the partial-state-on-restart bug: cluster nodes were serving `Local`-consistency
reads during the openraft log-replay window between snapshot restore and full catch-up,
returning stale (partially replayed) state to clients.

## Delivered

### `crates/valori-node/src/cluster.rs`

- Added `use openraft::{Config, SnapshotPolicy}` import.
- Added two env-tunable knobs evaluated at startup:
  - `VALORI_SNAPSHOT_EVERY_EVENTS` (default 5000) — `SnapshotPolicy::LogsSinceLast(n)`
    bounds the maximum log-replay window on restart.
  - `VALORI_RAFT_SNAPSHOT_KEEP` (default 1000) — `max_in_snapshot_log_to_keep` retains
    a tail of log entries after snapshot for followers that are only slightly behind.
- Captures `startup_committed_index` from `store.read_committed().await` (reads the
  `KEY_COMMITTED` entry persisted in the redb META table) before Raft is opened.
- Added `startup_committed_index: u64` to `ClusterHandle`; fresh in-memory nodes get `0`
  (immediately ready) because they have no prior committed entries.

### `crates/valori-node/src/cluster_server.rs`

- Added `ReadinessGate` struct: holds a `target: u64` and an `AtomicBool` latch.
  - `new(0)` → latch pre-opened (fresh node, nothing to catch up).
  - `check(&raft)` reads `raft.metrics().borrow().last_applied` and delegates to
    `check_applied(applied)`.
  - `check_applied(applied)` is a pure function: returns `Ok(())` once `applied >=
    target`, then latches open permanently (monotone; never re-closes).
- Added `readiness: Arc<ReadinessGate>` to `DataPlaneState`.
- Wired into `build_cluster_router()` via `handle.startup_committed_index`.
- Guard added at the top of three read handlers:
  - `search()` (line 383)
  - `get_graph_node()` (line 720)
  - `get_graph_edges()` (line 800)
  Each returns HTTP 503 with a `Retry-After: 1` header and a human-readable message
  until the node has replayed all entries committed before restart.
- Added `#[cfg(test)]` module with 5 unit tests covering:
  - Zero target is immediately ready.
  - Below-target is rejected.
  - At-target opens the latch.
  - Latch never re-closes (monotone property).
  - Fast-path (already latched) bypasses the target check.

## Findings

1. **Root cause was readiness, not data loss.** The local `events.log` is never
   pruned; `read_all_segments` + hash-splice recovery was already correct. The bug was
   exclusively in the read-path serving partial state during the catch-up window.

2. **openraft's default `Config` uses `SnapshotPolicy::LogsSinceLast(5000)`** but
   `max_in_snapshot_log_to_keep` defaults to an internal value that may retain many
   extra entries. The explicit config makes both numbers visible and tunable.

3. **`read_committed()` is async** (`impl RaftLogStorage` on `RedbLogStore`). The
   import `use openraft::storage::RaftLogStorage` is required to bring the trait method
   into scope.

4. **Steady-state `Local` reads are unaffected.** The gate is startup-only: once the
   latch opens it never closes, so the documented "may lag slightly" semantics for
   `Local` consistency apply normally during steady-state operation.

## Validation

```
cargo test -p valori-kernel -p valori-node
```

**Result: 198 tests passed, 0 failed, 1 ignored** (the ignored test is an existing
slow integration test gated behind a feature flag).

ReadinessGate unit tests (all in `cluster_server.rs`):
- `readiness_gate_zero_target_immediately_ready` ✅
- `readiness_gate_below_target_rejected` ✅
- `readiness_gate_opens_at_target` ✅
- `readiness_gate_latch_never_recloses` ✅
- `readiness_gate_fast_path_after_latch` ✅

Manual smoke test: `docker compose up -d` → restart node-2 mid-write → `GET /search`
on node-2 returns HTTP 503 with `"node catching up after restart: applied N < startup-committed M"` 
until catch-up completes, then returns correct results.

## Follow-ups

| Item | Phase |
|---|---|
| Expose `startup_committed_index` in `/v1/cluster/status` response so operators can monitor readiness programmatically | Future ops hardening |
| Add integration test: restart node while leader is writing, assert 503 on follower until caught up | Future integration test suite |
| S1 Contextual Retrieval / S2 GraphRAG / S3 Self-maintaining memory (Cortex) | Deferred — requires own eval harness first |
