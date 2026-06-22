# Phase 3.4 — As-of / Point-in-Time Reads

## Goal

Expose the event-sourced audit log as a queryable time machine: `POST /search` with an `as_of` timestamp or `as_of_log_index` replays committed events up to that point and searches the resulting state, returning a BLAKE3 proof receipt that any auditor can verify. Also upgrades `GET /v1/timeline` from a plain string dump to a structured, filterable JSON feed.

## Delivered

### `crates/valori-node/src/events/event_journal.rs`

- Added `timestamps: Vec<u64>` parallel to `committed` — each entry is the unix-second wall-clock time when that event was committed via `commit_buffer()`.
- `commit_buffer()` now stamps `SystemTime::now()` onto every event it promotes.
- `from_committed()` fills recovered events with `timestamp = 0` (no original wall-clock available after a restart without the original log).
- New methods:
  - `committed_with_timestamps()` — iterator over `(&KernelEvent, u64)` pairs
  - `event_timestamp(log_index)` — timestamp for a specific index
  - `find_log_index_at_or_before(unix_secs)` — binary search for the last committed event at or before a target time

### `crates/valori-node/src/api.rs`

- `SearchRequest` — two new optional fields: `as_of: Option<String>` (ISO 8601 UTC) and `as_of_log_index: Option<u64>`. Both default to `None`; existing clients are unaffected.
- `SearchResponse` — four new optional fields (serialized only when present): `as_of_log_index`, `as_of_timestamp_unix`, `as_of_timestamp_iso`, `as_of_state_hash` (BLAKE3 hex).
- `SearchResponse::simple(results)` — convenience constructor for the non-as-of path.
- New `TimelineEntry` struct: `log_index`, `timestamp_unix`, `timestamp_iso`, `event_type`, `record_id?`, `node_id?`, `edge_id?`.
- New `TimelineResponse` struct: `events`, `total`, `from_unix?`, `to_unix?`.

### `crates/valori-node/src/server.rs`

- `search` handler — dispatches to `search_as_of()` when `as_of` or `as_of_log_index` is present.
- New `search_as_of()` function:
  1. Resolves the target log index (from `as_of_log_index` directly, or via `find_log_index_at_or_before` for ISO timestamp).
  2. Replays `events[0..=target_idx]` into a fresh `KernelState::new()`.
  3. Converts the f32 query to Q16.16 `FxpVector`.
  4. Calls `KernelState::search_l2` / `search_l2_ns` on the replayed state.
  5. Computes `hash_state_blake3(&replay)` and returns it as a hex string.
- New `bytes_to_hex(b: &[u8]) -> String` — no `hex` crate dependency needed.
- New `parse_iso8601(s: &str) -> Option<u64>` — handles `YYYY-MM-DDTHH:MM:SSZ` without any external time crate.
- New `unix_to_iso8601(unix_secs: u64) -> String` — inverse formatter.
- `get_timeline` — replaced `Vec<String>` return with `TimelineResponse`. Accepts `TimelineQuery` params (`from`, `to`, `collection`) and applies timestamp range filtering.
- Added `/v1/timeline` route alongside the legacy `/timeline`.

### `crates/valori-node/tests/api_as_of.rs` (new)

Six integration tests:
1. `as_of_log_index_returns_past_state` — only records before the target index are returned.
2. `as_of_state_hash_advances_with_new_events` — hash at log_index=0 ≠ hash at log_index=2.
3. `as_of_log_index_out_of_range_returns_error` — 4xx/5xx returned, no panic.
4. `timeline_returns_structured_events` — correct event types, log_index, ISO timestamps.
5. `timeline_empty_when_no_events` — empty list, not an error.
6. `timeline_from_filter_excludes_past_events` — far-future `from` yields zero results.

### `python/valoricore/remote.py`

- `SyncRemoteClient.search()` — added `as_of: Optional[str]` and `as_of_log_index: Optional[int]` params. When either is set the full response dict (with proof fields) is returned instead of just the hits list.
- `SyncRemoteClient.timeline()` — new method: `GET /v1/timeline` with optional `from_ts`, `to_ts`, `collection` params.
- `AsyncRemoteClient.search()` — same `as_of` / `as_of_log_index` params.
- `AsyncRemoteClient.timeline()` — new async method using `aiohttp`.

## Findings

1. **Timestamps on recovered events are 0** — `EventJournal::from_committed()` is called after WAL/event-log replay, but the original timestamps are embedded in the log entries (not surfaced by the current reader). Phase 3.4 timestamps are accurate for events committed in the current process lifetime. A future phase can backfill wall-clock timestamps by reading `EntryV3.timestamp` from the WAL during recovery.

2. **As-of search is O(N) per call** — replays all events from 0 to the target on every call. For the typical audit use case (infrequent, non-hot-path) this is fine. If it becomes a bottleneck, a snapshot cache indexed by log height would bring it to O(tail) where tail = events since the nearest snapshot.

3. **Namespace (collection) filter on timeline is stubbed** — `KernelEvent` carries namespace IDs in some variants but not uniformly. The `collection` query param is accepted and stored in the response but not yet applied to filter events. This requires propagating the namespace ID into every event variant or storing a parallel namespace-per-event index.

4. **`AsyncRemoteClient.timeline()` adds `aiohttp` dependency** — the async client currently uses `httpx` or `aiohttp` inconsistently. Homogenize in Phase 3.3 (cluster SDK refactor).

## Validation

```
cargo test -p valori-node --test api_as_of
```

```
running 6 tests
test timeline_empty_when_no_events ... ok
test timeline_from_filter_excludes_past_events ... ok
test as_of_log_index_out_of_range_returns_error ... ok
test timeline_returns_structured_events ... ok
test as_of_log_index_returns_past_state ... ok
test as_of_state_hash_advances_with_new_events ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

Full `valori-node` suite: all tests pass (no regressions).

## Follow-ups

- **Backfill timestamps from WAL on recovery** — read `EntryV3.timestamp` during event-log replay so recovered events have accurate wall-clock times.
- **Namespace filter on timeline** — propagate namespace ID into every `KernelEvent` variant or maintain a parallel `timeline_ns` index.
- **Snapshot cache for as-of** — cache `KernelState` snapshots keyed by log height to make repeated as-of queries at the same index O(1).
- **Cluster as-of reads** — the Raft log store (redb) has per-entry `LogId` with term + index, but not wall-clock time. Wall-clock must be embedded in `ClientRequest` at proposal time (Phase 3.3/3.5 dependency).
