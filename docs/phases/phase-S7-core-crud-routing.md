# Phase S7 — Core CRUD `collection` field + shard routing

Branch: `Node-scaleup` (S1 `6d53924`, S2 `08dd043`, S3 `0460cee`, S4 `809b87a`, S5, S6 merged).

## Goal

The cluster path's core CRUD surface — `/v1/records` (insert),
`/v1/search`, `/v1/delete`, `/v1/soft-delete`, `/v1/vectors/batch-insert` —
had **no `collection` field at all**, unlike every `/v1/memory/*` handler
already routed in S3b/S4. Every write and read through this surface always
targeted the default namespace on shard 0, regardless of what a caller
intended, and silently ignored a `collection` value the SDK was already
capable of sending (it uses the same field name against the standalone
server, which has always honored it). This phase closes that gap.

## Delivered

All five handlers in `crates/valori-node/src/cluster_server.rs` gained a
`collection: Option<String>` field (`#[serde(default)]`, matching the
standalone `api.rs` convention exactly — same field name, same "absent or
`\"default\"` means the default namespace" semantics) and now resolve it
via `state.sm.resolve_namespace(...)` before routing:

- **`insert_record`** (`/v1/records`) — routes the `AutoInsertRecord` write
  through `state.shard_for(ns_id).raft` with `namespace_id: ns_id`.
- **`search`** (`/v1/search`) — resolves `ns_id`, then routes every state
  read (`locked_dim`, `with_state` L2 scan, `with_text_corpus` rerank,
  `with_state_and_timestamps` decay path) through
  `state.shard_for(ns_id).state_machine`, and the linearizable read-index
  check through that shard's `raft` (this is the call site S6's
  `ensure_read_consistency(shard_id, ...)` signature was built for).
- **`delete_record` / `soft_delete_record`** (`/v1/delete`,
  `/v1/soft-delete`) — `DeleteRequest` gained `collection`. Record ids are
  only unique within their own shard's kernel state (each shard runs an
  independent id counter), so there is no way to locate a record from its
  bare `id` alone — the caller must name the collection, the same
  constraint `cluster_memory_consolidate` already had for `old_record_id`.
- **`batch_insert`** (`/v1/vectors/batch-insert`) — resolves `ns_id` once
  before the loop, then every `AutoInsertRecord` in the batch routes to
  that single shard (a batch insert always targets one collection, so one
  shard resolution up front is correct, not a per-item resolve).

`api.rs`'s standalone `DeleteRecordRequest`/`InsertRecordRequest`/etc. were
already the reference shape this phase matched — no standalone changes
needed, only the cluster path catching up.

### Python SDK — two real bugs found and fixed in the same area

While wiring the field, `SyncRemoteClient.soft_delete()` and
`AsyncRemoteClient.soft_delete()` were found posting to `/v1/delete` (hard
delete) instead of `/v1/soft-delete` — the SDK's "soft delete" call has
always permanently removed the record, on both standalone and cluster
targets, since before this phase existed. Fixed both methods to hit the
correct endpoint. `crates/valori-node/README.md`'s API table had the same
mislabeling (`/v1/delete` documented as "soft-delete a record by ID") —
corrected, and `/v1/soft-delete` (previously undocumented) added as its own
row.

All four SDK delete methods (`SyncRemoteClient`/`AsyncRemoteClient` ×
`delete`/`soft_delete`) gained a `collection: str = "default"` parameter,
plus their `ClusterClient`/`AsyncClusterClient` wrapper forwarding
(`AsyncClusterClient` was also missing a `soft_delete` forwarding method
entirely — added for parity with `ClusterClient`).

## Findings

- The `/v1/delete` = soft-delete documentation/SDK bug (above) predates
  this phase and affects **both** standalone and cluster targets — it was
  not introduced by sharding work, but was found while touching this exact
  code region for the `collection` field, and was cheap enough to fix in
  the same pass rather than deferring.
- Confirms the S3a finding pattern continues to apply to whatever region of
  `cluster_server.rs` is touched next: `record_count()` intentionally
  excludes soft-deleted records (already known from S4), used again here
  to make the S7 test's assertions self-checking without a separate
  "is this the soft-deleted one" query.

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean (untouched)
cargo test -p valori-kernel        # 62 passed
cargo test -p valori-node          # 233 passed
python3 -c "import ast; ast.parse(open('python/valoricore/remote.py').read())"  # syntax OK
```

No existing Python test references `soft_delete`/`delete` directly (grepped
`python/tests/`), so the endpoint-fix carries no test-breakage risk; not
run against a live node this pass (would require building + starting the
binary, out of scope for a Python-syntax-level fix — flagged here rather
than silently assumed correct).

New test:
`crates/valori-node/tests/cluster_namespaces.rs::core_crud_routes_to_the_collections_shard`
— exercises `/v1/records`, `/v1/search`, `/v1/vectors/batch-insert`, and
`/v1/soft-delete` against a `tenant-a` collection resolved to a non-zero
shard, asserting every write lands there and shard 0 stays untouched.

## Follow-ups

- Graph management (`/v1/graph/*`) and community endpoints still lacked
  shard routing at the start of this phase — delivered in the same working
  session, see `phase-S8-graph-community-routing.md`.
- The Python SDK fix was not exercised against a live cluster in this pass
  (syntax-checked only) — worth a smoke test next time a node binary is
  built for another reason.
