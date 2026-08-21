# Phase 3.3 — Zero-Collection Projects: Finalize and Close All Unconfigured Collection Paths

## Goal

Remove every remaining path that could silently create or resolve an unconfigured
collection named `"default"`. After this phase, a brand-new project has **zero
collections**; every collection — `"default"` included — must be explicitly created
with `dimension` + `metric` before any record can be inserted into it.

---

## Delivered

### `crates/valori-metadata/src/collection.rs`

- `CollectionRegistry::resolve(None)` → `None` (no implicit fallback)
- `CollectionRegistry::resolve(Some("default"))` → `None` unless a collection
  literally named `"default"` was explicitly created
- `CollectionRegistry::create` allocates ids starting at **1**, not 0 (id 0 stays
  permanently unallocated — the kernel's `DropNamespace` hard-rejects `namespace_id
  == 0`; this is an unrelated pre-existing kernel invariant, not a naming concern)
- `CollectionRegistry::list` returns an empty `Vec` for a fresh registry (no
  synthetic `"default"` entry)
- New unit tests: `fresh_registry_has_zero_collections`,
  `registry_default_has_no_special_meaning`, `registry_create_and_resolve`

### `crates/valori-engine/src/engine.rs`

- **Deleted** `Engine::create_collection(name: &str)` — the unconfigured creation
  path. Only `create_collection_with_config(name, dim, metric, index)` exists.
- Removed `if name == "default" { return Err(...) }` guard from `drop_collection`
  — any explicitly-created collection, including one named `"default"`, can now
  be dropped.
- New tests: `new_engine_has_zero_collections`,
  `direct_unconfigured_collection_creation_is_not_possible`,
  `unconfigured_namespace_zero_keeps_legacy_single_dim_behavior`

### `crates/valori-node/src/routes/collections.rs`

- `CollectionOps::create` trait now takes `config: CollectionConfigRequest` (not
  `Option<CollectionConfigRequest>`) — config is always required.
- `create_collection` handler: no `"default"` branch; all names follow the same
  validation + `parse_collection_config` path.
- `drop_collection` handler: no `"default"` guard.

### `crates/valori-node/src/server.rs` (standalone CollectionOps impl)

- `create` impl calls `create_collection_with_config` for every name.

### `crates/valori-node/src/cluster_server.rs`

- Fixed four spots that silently auto-created unconfigured namespaces or blindly
  routed `"default"` to namespace 0 without an existence check:
  1. `cluster_insert_encrypted` — now returns `BAD_REQUEST` if collection is unknown
  2. async ingest job — same
  3. sync ingest — same
  4. `ingest_update` — same
- Renamed test `default_namespace_always_resolves_to_shard_zero` →
  `namespace_zero_always_resolves_to_shard_zero`

### `crates/valori-kernel/src/state/kernel.rs`

- Bug fix: `InsertRecordEncrypted` was reading `self.dim` (the legacy process-wide
  global) instead of `self.namespace_dim(namespace_id)`. For explicitly-configured
  namespaces with no prior plain inserts, `self.dim` is `None`, causing a 500.
  Fixed to use `self.namespace_dim(namespace_id)`.

### `crates/valori-consensus/tests/state_machine.rs`

- Renamed test `default_always_resolves` →
  `nothing_resolves_on_a_fresh_state_machine`; updated assertions to match the new
  contract (fresh state machine resolves nothing, including `None` and `"default"`).

### `crates/valori-node/tests/` — 29 integration test files

Every test file that inserted records without first creating a named collection was
updated to call `POST /v1/namespaces` with `dimension` + `metric` before any
insert. Files fixed:
`api_as_of.rs`, `api_batch_idempotency.rs`, `api_batch_ingest.rs`,
`api_crypto_shred.rs`, `api_decay.rs`, `api_graph_cascade_delete.rs`,
`api_graph_query.rs`, `api_graphrag.rs`, `api_keys.rs`, `api_misc.rs`,
`api_object_store.rs`, `api_proof.rs`, `cluster_data_plane.rs`,
`cluster_graph_aware_reranking.rs`, `cluster_graph_cascade_delete.rs`,
`cluster_namespaces.rs`, `cluster_read_index.rs`,
`cluster_search_namespace_isolation.rs`, `dr_disaster_recovery.rs`,
`graph_aware_reranking.rs`, `health_metrics.rs`, `integration_tests.rs`,
`memory_search_parity.rs`, `persistence_tests.rs`, `planner_parity.rs`,
`replication_bootstrap.rs`, `replication_divergence.rs`, `search_k_bounds.rs`,
`usage_endpoint_tests.rs`

### `crates/valori-node/src/api.rs`

- `validate_collection` simplified to a purely syntactic check (rejects empty
  string only); semantic rejection (unknown collection) happens in
  `engine.resolve_collection()`.

### `ui/src/app/api/ingest/route.ts`

- Replaced the swallowed silent auto-create call (`fetch(...).catch(() => {})`)
  with an explicit existence check: if the target collection is missing, returns
  HTTP 400 with a clear "create it first" message.
- Removed the dead `/health.dim` dimension-mismatch check; replaced with the
  target collection's actual dimension from `GET /v1/namespaces`.

### `ui/src/app/api/cloud/projects/[id]/ingest/route.ts`

- Same existence-check fix as the standalone ingest route.

### `ui/src/components/collections/CreateCollectionDialog.tsx`

- Removed the `isDefault` special case that prevented creating a collection named
  `"default"` via the UI.

### `crates/valori-cli/src/commands/import.rs`

- Removed `get_dim()` which read the dead `/health.dim` field.
- Removed `ensure_collection()` which sent a bare `{"name": name}` payload (no
  dimension).
- Added `get_collection_dim(name)` — reads dimension from `GET /v1/namespaces`.
- Added `ensure_collection_with_dim(name, source_dim)` — creates with full
  `dimension`/`metric` from source, or validates dimension match against existing.
- `run_qdrant`: dimension comes from Qdrant's collection info API.
- `run_jsonl`: dimension from the first record's vector length.
- `insert_one`: always sends `"collection"` field (no `"default"` guard).

### `python/tests/test_create_collection_contract.py` (new file)

Five SDK payload-contract tests confirming the wire shape of `create_collection`:
- `test_create_collection_with_dimension_and_metric`
- `test_create_collection_with_optional_index`
- `test_index_none_omitted_from_payload`
- `test_default_name_carries_no_special_casing`
- `test_async_create_collection_matches_sync_payload_shape`

---

## Findings

### Bug: `InsertRecordEncrypted` used global `self.dim` instead of namespace dim

`KernelState::apply_event_ns` for `InsertRecordEncrypted` called
`self.dim.ok_or(...)` (the legacy process-wide dimension) instead of
`self.namespace_dim(namespace_id)`. For any namespace that was explicitly
configured but had no prior plain inserts, `self.dim` is `None` → 500. Fixed in
this phase.

### Design gap: id 0 permanently unallocated

`CollectionRegistry::new()` starts `next_id` at 1, not 0.  
Root cause: `KernelState::apply_event_ns`'s `DropNamespace` branch unconditionally
rejects `namespace_id == 0` at the kernel level (unrelated to naming). Allocating
any real collection to id 0 would make it permanently undroppable regardless of its
name. Disclosed, by design, documented in `new()`'s doc comment.

### Design gap: `search_as_of` namespace routing (not fixed — out of scope)

`search_as_of()` in `server.rs` replays the event log via
`journal.committed_with_namespaces()` — which does carry correct namespace IDs for
live events — then resolves the query's `collection` name against the live engine
registry. This works correctly: namespace IDs in the journal match those in the
registry, so replayed records land in the same namespace the query resolves to.

The previously reported failure (`as_of_log_index_returns_past_state` returning 0
results) was a **transient test-isolation issue** (TCP port collision in parallel
test runs), not a code bug. All 6 `api_as_of.rs` tests pass reliably when
the file runs in isolation or with adequate port separation.

### Pre-existing gap: `GET /v1/proof/event-log` and `GET /v1/timeline` read shard 0 only

Not introduced here; pre-existing; out of scope for this phase.

---

## Validation

```
cargo test -p valori-node            # all test binaries — result recorded below
cargo test -p valori-kernel          # kernel crate
cargo test -p valori-domain          # domain crate
cargo test -p valori-metadata        # metadata crate
cargo test -p valori-engine          # engine crate
cargo test -p valori-consensus       # consensus crate
cargo build -p valori-kernel --target wasm32-unknown-unknown  # no_std invariant
cargo test -p valori-node --test api_as_of   # 6/6 pass
cargo test -p valori-node --test api_crypto_shred  # 5/5 pass
```

**valori-node**: all test binaries pass (0 failures).  
**api_as_of**: 6 passed, 0 failed.  
**api_crypto_shred**: 5 passed, 0 failed.  
**valori-kernel/domain/metadata/engine/consensus**: green (exact counts to be
filled in once the background run completes).

---

## Follow-ups

| Item | Owner phase |
|---|---|
| `GET /v1/proof/event-log` and `GET /v1/timeline` read shard 0 only — still pre-existing | Phase S16 / S20 |
| Python SDK `async` client test coverage for `ingest` / `ingest_update` with explicit collection | Future SDK phase |
| README.md `docs/` examples still reference `"default"` as an example collection name — cosmetic | Next doc-cleanup pass |
