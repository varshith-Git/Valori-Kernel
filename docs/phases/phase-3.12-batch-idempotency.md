# Phase 3.12 — Batch insert per-item idempotency

## Goal

Allow callers to supply a per-item idempotency key (`request_id`) in a batch insert. Duplicate keys are detected server-side and the previously assigned record ID is returned instead of creating a new record, enabling safe at-least-once delivery without double-inserts.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/api.rs` | `BatchInsertRequest` gains `request_ids: Option<Vec<Option<String>>>` (32-hex per item, `null` = no dedup for this slot) |
| `crates/valori-node/src/engine.rs` | `Engine` gains `batch_seen: FxHashMap<[u8;16], u32>` in-memory dedup map; `insert_batch_ns()` updated to accept `request_ids: Option<&[Option<[u8;16]>]>` with O(1) lookup per item; `insert_batch()` updated to pass `None` |
| `crates/valori-node/Cargo.toml` | Added `rustc-hash = "2.1.1"` |
| `crates/valori-node/src/server.rs` | `batch_insert` handler parses hex `request_ids` and passes parsed bytes to `insert_batch_ns()` |
| `crates/valori-node/tests/api_batch_ingest.rs` | Updated `BatchInsertRequest` construction to include `request_ids: None` |
| `crates/valori-node/tests/api_batch_idempotency.rs` | **NEW** — 4 integration tests covering: same ID returned for duplicate key, mixed dedup+new in one batch, backward compat without keys, fully deduped batch does not grow record count |
| `python/valoricore/remote.py` | `insert_batch()` on `SyncRemoteClient`, `AsyncRemoteClient`, `ClusterClient`, and `AsyncClusterClient` gains `request_ids: Optional[List[Optional[str]]] = None` |

**Wire format:**
```json
{
  "batch": [[0.1, 0.2, 0.3, 0.4]],
  "request_ids": ["aabbccddeeff00112233445566778899"]
}
```
A `null` entry in `request_ids` opts that slot out of dedup. Omitting `request_ids` entirely is unchanged behavior.

**Dedup scope:** Within a single process lifetime. `batch_seen` is not persisted — restarts clear the cache. For cross-restart idempotency, the Raft cluster dedup (`request_id` in `ClientRequest`) remains the authoritative mechanism.

## Findings

- Capacity guard is now based on `insert_indices.len()` (new items only), not `batch.len()`, so a fully deduped batch never trips the capacity limit.
- The `id_map` approach interleaves deduped and new IDs correctly even for batches where deduped and new items are interleaved at arbitrary positions.

## Validation

```
cargo test -p valori-node --test api_batch_idempotency
4 passing, 0 failing

cargo test -p valori-node -p valori-kernel
224 passing, 0 failing
```

## Follow-ups

- Persistence: if cross-restart dedup is needed for the standalone path (no Raft), `batch_seen` could be written to a sidecar file alongside the WAL. Deferred.
- Phase 3.13 — HNSW parameter exposure
