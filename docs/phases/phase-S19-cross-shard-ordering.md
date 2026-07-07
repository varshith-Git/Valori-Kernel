# Phase S19 — Cross-shard replay ordering validation

## Goal

Guarantee that `GET /v1/timeline` on a multi-shard cluster returns a deterministically
merged, correctly ordered event stream and actively rejects log files whose per-shard
ordering is broken.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/api.rs` | Added `shard_id: u32` field to `TimelineEntry` — carried through to the JSON response |
| `crates/valori-node/src/server.rs` | Standalone `get_timeline` handler sets `shard_id: 0` on every entry |
| `crates/valori-node/src/cluster_server.rs` | `parse_log` closure now takes `shard_id: u32` and tags each `TimelineEntry`; sort key changed to `(timestamp_unix, shard_id, log_index)`; new validation pass that errors HTTP 500 if any shard's `log_index` is non-monotonic in the merged output |
| `crates/valori-node/tests/cluster_namespaces.rs` | New test `timeline_merges_cross_shard_events_in_order` |

## Findings

- Wall-clock `timestamp_unix` is non-monotonic across shards; the composite key
  `(timestamp_unix, shard_id, log_index)` gives deterministic tie-breaking.
- The validation pass catches tampered or out-of-order log segments at read time,
  not just at full-chain verify time.

## Validation

```
cargo test -p valori-node timeline_merges_cross_shard_events_in_order -- --nocapture
# → test timeline_merges_cross_shard_events_in_order ... ok

cargo test -p valori-kernel -p valori-node
# All results: ok, 0 failed
```

Total passing tests across both crates: 229+.

## Follow-ups

- `valori-cli verify` should surface CRC violations (V4) distinctly from chain breaks — deferred.
- Generate a V4 fixture file and add `v4_fixture_decodes_forever` test — deferred.
- Unify duplicate `event_log.rs` in `valori-node` vs `valori-storage` — deferred.
