# Phase A10 — Receipt bridge: real receipts from live traffic

## Goal

Make `GET /v1/proof/receipt` return real per-operation receipts from actual HTTP
traffic — not just test data — by wiring `ReceiptAssembler` into the two highest-value
handlers (`insert_record` and `search`) on both standalone and cluster paths, without
restructuring all 40+ existing handlers.

## Delivered

### `crates/valori-node/src/receipt_bridge.rs` (new)

Thin shim between existing handlers and `ReceiptAssembler`:

| Function | Use |
|---|---|
| `emit_write(store, op_kind, ns_id, shard_id, height, cluster_mode, state_before, state_after)` | Mutating operations — assembles a `Receipt` with one `ReceiptFragment { mutated: true }` |
| `emit_read(store, op_kind, ns_id, shard_id, height, cluster_mode, state_hash)` | Read-only operations — assembles a `Receipt` with `ReceiptFragment::read_only()` |

Operation hash = `BLAKE3(op_kind ‖ ns_id ‖ timestamp_nanos)` — unique per operation.
Full planner-derived hash (reproducible from request content) comes in A11.

### `crates/valori-node/src/server.rs` — standalone path

**`insert_record`**:
- Extracts `Extension<Arc<ReceiptStore>>`.
- Captures `state_before = hash_state_blake3(&engine.state)` hex while holding the write lock, before the kernel write.
- Captures `state_after` immediately after `insert_record_from_f32_ns()`.
- Calls `receipt_bridge::emit_write(...)`.

**`search`**:
- Extracts `Extension<Arc<ReceiptStore>>`.
- Captures `state_hash` from the read lock at handler entry.
- Calls `receipt_bridge::emit_read(...)` before each return point (no-decay path and decay path).

### `crates/valori-node/src/cluster_server.rs` — cluster path

**`insert_record`**:
- Extracts `Extension<Arc<ReceiptStore>>`.
- Gets `state_before` from `sm.state_hash().await` before the write.
- Switches from `raft_write` (callback) to `raft_write_data` (returns `ClientResponse`) to access `resp.state_hash` and `resp.log_index` after commit.
- Calls `receipt_bridge::emit_write(...)` with the real Raft log index as `committed_height`.

**`search`**:
- Extracts `Extension<Arc<ReceiptStore>>`.
- Captures `state_hash` from `shard.state_machine.state_hash().await` after the search completes.
- Calls `receipt_bridge::emit_read(...)` before the response is returned.

### `crates/valori-node/src/lib.rs`

Added `pub mod receipt_bridge`.

## Findings

- The `Engine` in standalone mode exposes no `.committed_height()` helper — the WAL journal's height is only accessible via `wal_writer.journal()`, which does not exist on the node-local `WalWriter` struct (the journal-based one lives in `valori-storage`). Used `0` as `committed_height` for standalone; this is the correct pragmatic choice — standalone has no Raft log index. A future phase can add a monotonic counter.
- Cluster `insert_record` previously used the `raft_write` closure API which loses the `ClientResponse`. Switching to `raft_write_data` is the right pattern for any handler that needs the post-write state hash or log index.
- `search` has two exit paths (no-decay and decay); both now emit a receipt.

## Validation

```
cargo build -p valori-node   # clean
cargo test -p valori-kernel -p valori-node -p valori-effect
```

All suites pass; 0 failures.

Smoke test (node running):

```bash
curl -s -X POST localhost:3000/v1/records -H 'Content-Type: application/json' \
  -d '{"values": [0.1, 0.2, 0.3]}'
curl -s localhost:3000/v1/proof/receipt | jq '.receipt.operation_hash'
# → "blake3-hex..."  — real receipt, not null
```

## Follow-ups

- **A11 — Planner-derived operation hash**: replace `bridge-v0` graph/fp hashes with
  the real `OperationHash = BLAKE3(kind ‖ inputs ‖ policy)` from the Planner. This makes
  receipts reproducible from the request content, not just unique.
- **Wire remaining write handlers**: `batch_insert`, `delete`, `soft-delete`,
  `memory_upsert`, `consolidate`, `contradict`, `ingest`. Each needs the same
  `emit_write` call added after its existing logic.
- **Python SDK + UI**: `get_receipt()` / `get_receipt_by_id()` SDK methods and
  a receipt display panel in the UI proof page — deferred to after A11.
