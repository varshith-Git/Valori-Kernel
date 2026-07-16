# Phase A11 — Real OperationHash + write-handler coverage

## Goal

Replace the placeholder `op_hash` (timestamp-based, not reproducible) from Phase A10
with the canonical RFC-0003 `OperationHash = BLAKE3(kind_discriminant ‖ bincode(inputs)
‖ bincode(policy))`, and extend receipt emission to all remaining write handlers:
`batch_insert`, `delete_record`, and `soft_delete_record` on both standalone and cluster
paths.

## Delivered

### `crates/valori-planner/src/operation.rs`

- Added `OperationKind::Delete` and `OperationKind::BatchInsert` variants.
- Added matching `OperationInputs::Delete { collection, shard_id, mode }` and
  `OperationInputs::BatchInsert { count, collection, shard_id }` variants.

### `crates/valori-node/src/receipt_bridge.rs`

- Rewrote `make_assembler` to call `compute_operation_hash(kind, inputs, &ExecutionPolicy::default())`
  — the canonical RFC-0003 hash, reproducible from planning parameters alone.
- `emit_write` / `emit_read` signatures now take `(OperationKind, &OperationInputs, …)`
  instead of bare strings.

### `crates/valori-node/src/server.rs` (standalone)

| Handler | Change |
|---|---|
| `insert_record` | Updated `emit_write` call to pass `OperationKind::Ingest` + `OperationInputs::Ingest` |
| `batch_insert` | Added `Extension<Arc<ReceiptStore>>`; `emit_write` with `OperationKind::BatchInsert` |
| `delete_record` | Added `Extension<Arc<ReceiptStore>>`; `emit_write` with `OperationKind::Delete { mode: "hard" }` |
| `search` (both exit paths) | Updated both `emit_read` calls to pass `OperationKind::Search` + `OperationInputs::Search` with real flags |

### `crates/valori-node/src/cluster_server.rs` (cluster)

| Handler | Change |
|---|---|
| `insert_record` | Updated `emit_write` call; already on `raft_write_data` from A10 |
| `batch_insert` | Added `Extension<Arc<ReceiptStore>>`; `state_before` from `sm.state_hash().await`, `state_after` from shard SM after all inserts; `emit_write` with `OperationKind::BatchInsert` |
| `delete_record` | Switched to `raft_write_data`; added `Extension<Arc<ReceiptStore>>`; `emit_write` with `mode: "hard"` |
| `soft_delete_record` | Switched to `raft_write_data`; added `Extension<Arc<ReceiptStore>>`; `emit_write` with `mode: "soft"` |
| `search` (both exit paths) | Updated `emit_read` calls with real `OperationInputs::Search`; `ConsistencyLevel` mapped from request |

## Findings

- The `OperationKind` discriminant is cast to `u8` for hashing; adding new variants
  must not reorder existing ones (the discriminant is their ordinal). Appending is safe.
- `batch_insert` cluster `state_after` requires a second `sm.state_hash().await` call
  after the insert loop — unavoidable because Raft applies on all nodes and the SM hash
  reflects the committed state.
- `soft_delete_record` on the cluster path previously used `raft_write` (no return
  value); switched to `raft_write_data` to obtain `log_index` for `committed_height`.
- The third `emit_read` call in the standalone `search` handler (decay path, ~line 655)
  was still using the A10 string API after the bulk replacement; fixed manually.

## Validation

- `cargo test -p valori-node --test api_keys`: **8 passed, 0 failed**
- `cargo test -p valori-node --test cluster_namespaces`: **16 passed, 0 failed**
- `cargo test -p valori-planner`: **3 passed, 0 failed** (hash determinism, distinct params, operation_new)
- `cargo test -p valori-effect`: **16 passed, 0 failed**
- `cargo build -p valori-kernel --target wasm32-unknown-unknown`: passes (kernel untouched)
- Manual: `GET /v1/proof/receipt` after an insert returns a receipt with a non-zero,
  deterministic `op_hash` (same parameters → same hash across restarts).

## Follow-ups

| Item | Phase |
|---|---|
| Wire receipt emission into `memory_upsert`, `consolidate`, `contradict`, `ingest` | A12+ |
| Replace `"bridge-v0"` `graph_hash` / `fp_hash` with real planner-derived values | A12 (full planner integration) |
| Python SDK: `get_receipt()` + `get_receipt_by_id(id)` methods | B1 (SDK update sprint) |
| UI: receipt display panel on the proof/audit page | B1 (SDK update sprint) |
