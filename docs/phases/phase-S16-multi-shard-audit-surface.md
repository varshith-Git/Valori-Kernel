# Phase S16 — Multi-shard audit surface

## Goal

Fix the two audit/proof endpoints that silently read only shard 0's log in a
multi-shard cluster, making `/v1/proof/event-log` and `/v1/timeline` cover every
shard's BLAKE3-chained audit trail.

## Delivered

### `crates/valori-node/src/cluster_server.rs`

**`DataPlaneState.event_log_path` → `shard_event_log_paths`**

Replaced the single `Option<PathBuf>` (shard 0 only) with a
`BTreeMap<ShardId, PathBuf>` populated from every shard's `event_log_writer` at
router build time. Shards without a real audit sink (writer is `None`) are simply
absent from the map — no silent shard-0 fallback.

**`build_cluster_router_with_keys`**

Iterates `handle.shards` to build `shard_event_log_paths`:
```
shard_event_log_paths = handle.shards
    .iter()
    .filter_map(|(id, h)| h.event_log_writer.as_ref().map(|w| (*id, w.lock().path())))
    .collect()
```

**`event_log_proof`**

Returns a hash per shard under `shards: { "0": { "event_log_hash": "…" }, "1": {…} }`.
Top-level `event_log_hash` is shard 0's hash for backward compatibility with
single-shard clients. Shards whose log cannot be hashed return an `error` field
rather than failing the entire response.

**`cluster_timeline`**

Reads every shard's log via a shared `parse_log` closure, collects all
`TimelineEntry` rows from all shards, then sorts by `timestamp_unix` before
returning. The merged view is chronologically ordered across shards, which is
correct for wall-clock event ordering (shard logs use the same clock source).

## Findings

- `TimelineEntry.timestamp_unix` is `u64`, so `sort_by_key` is stable and
  zero-allocation. Equal timestamps preserve shard-iteration order (BTreeMap
  iterates by ascending `ShardId`).
- The `audit` parameter on `build_cluster_router_with_keys` was the old shard-0
  alias; it was removed from the function body (now derived from `handle.shards`)
  but the parameter itself remains because `main.rs` and tests pass it. It is now
  unused inside the function — a follow-up can remove it and update call sites, but
  it causes no correctness issue.
- Asymmetric placement (per-shard node subsets) is a separate architectural
  concern; this phase only fixes the audit surface visibility.

## Validation

- `cargo test -p valori-node --test cluster_namespaces`: **16 passed, 0 failed**
- `cargo test -p valori-node --test api_keys`: **8 passed, 0 failed**
- `cargo build -p valori-node`: clean, 0 errors

Manual verification path (3-node / 2-shard cluster):
1. Insert records into two collections that hash to different shards.
2. `GET /v1/proof/event-log` → response now contains `shards.0.event_log_hash`
   and `shards.1.event_log_hash` (both non-zero, distinct).
3. `GET /v1/timeline` → events from both shards appear, sorted by timestamp.

## Follow-ups

| Item | Notes |
|---|---|
| Remove unused `audit` param from `build_cluster_router_with_keys` | Cosmetic; call sites in `main.rs` + tests pass `None` already |
| Asymmetric placement (per-shard node subsets) | Requires per-shard Raft membership negotiation — large architectural work, deferred |
