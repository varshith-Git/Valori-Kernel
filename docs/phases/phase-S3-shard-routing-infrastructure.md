# Phase S3 — Shard-routing infrastructure + namespace-scoping fix

Branch: `Node-scaleup` (S1 `6d53924`, S2 `08dd043` merged).

## Goal

Build the "front desk" that routes a namespace's data to the correct
shard's Raft group. This phase shipped in two parts within the same
session: an initial infra-only slice (routing math + `DataPlaneState`
awareness, nothing wired in) followed immediately by **S3a** (fixing a
separate, pre-existing bug that blocked wiring it up) and **S3b** (actually
wiring representative handlers to real, shard-routed, namespace-correct
writes and reads). All three landed together after the bug was confirmed
fixable within a bounded, verifiable blast radius.

## Delivered

### Routing infrastructure (`crates/valori-node/src/cluster_server.rs`)

- **`shard_for_namespace(namespace_id: u16, shard_count: u32) -> ShardId`**
  — pure, deterministic `namespace_id % shard_count`. No placement table:
  S1 keeps every shard symmetric (every cluster member is a voter in every
  shard), so every node computes the identical answer independently, no
  coordination needed. `shard_count=1` always resolves to `ShardId(0)`.
- **`DataPlaneState`** gained `shards: Arc<BTreeMap<ShardId, ShardHandle>>`
  + `shard_count: u32`, populated from `ClusterHandle.shards` (S1). The
  existing `raft`/`sm` fields stay untouched (shard 0), so every handler
  that doesn't resolve a namespace is byte-identical to before this phase.
- **`DataPlaneState::shard_for(&self, namespace_id) -> &ShardHandle`** —
  the routing accessor, now actually used (see S3b below).
- 4 unit tests for `shard_for_namespace` (shard_count=1 always shard 0,
  namespace 0 always shard 0, deterministic distribution, shard_count=0
  defensive non-panic).

### S3a — the namespace-scoping bug fix

**Root cause, confirmed by reading the code directly:**
`ValoriStateMachine::apply()`'s generic dispatch branch called
`inner.state.apply_event(&req.event)` — which is `apply_event_ns(evt,
DEFAULT_NS.0)` — for every event type except the two S2 added
(`AutoCreateNamespace`/`DropNamespace`). `KernelEvent::AutoInsertRecord`,
`AutoCreateNode`, and `AutoCreateEdge` carry no `namespace_id` field at
all. Confirmed directly in the handler code: `cluster_memory_upsert` and
`cluster_memory_consolidate` both resolved a `ns_id` from the requested
collection and then discarded it (`let _ = ns_id;`). **Every record and
graph node written through these paths landed in namespace 0 regardless of
which collection the client specified** — collections/namespaces for
graph and vector data were non-functional in cluster mode, independent of
sharding, predating both S1 and S2.

**Fix:**
- `ClientRequest` (`crates/valori-consensus/src/types.rs`) gained
  `namespace_id: u16` (`#[serde(default)]` — old callers decode this as 0,
  byte-identical to prior behavior).
- `apply()`'s generic branch (`crates/valori-consensus/src/state_machine.rs`)
  now calls `apply_event_ns(&req.event, req.namespace_id)` instead of the
  `DEFAULT_NS`-hardcoded `apply_event(&req.event)`. Variants carrying their
  own internal `namespace_id` (`InsertRecordEncrypted`/
  `AutoInsertRecordEncrypted`) are unaffected — they never consulted this
  parameter.
- **Blast radius, handled mechanically then verified by full workspace
  build**: adding a required field to a plain (non-`Default`) struct broke
  every existing `ClientRequest { ... }` construction site — **63 sites**
  across `valori-consensus` (lib + 12 test files) and `valori-node` (`cluster_server.rs`'s
  33 handler-level literals + `commit/raft.rs` + 1 test file). Fixed via a
  scripted pass (regex-insert `namespace_id: 0,` after every
  `schema_version: ...,` occurrence, verified against the compiler's exact
  error list, hand-fixed the handful of single-line/reordered-field
  literals the script's pattern didn't match) followed by full-workspace
  `cargo build --tests` until clean. Every site defaults to `namespace_id:
  0` (today's behavior) except the specific handlers upgraded in S3b.

### S3b — wiring into representative handlers

Three handlers now genuinely namespace-scope their data AND route to the
correct shard:

- **`cluster_memory_upsert`** (write) — resolves `ns_id`, gets
  `state.shard_for(ns_id)`, and every one of its 4 Raft writes (insert
  record, create/reuse document node, create chunk node, connect
  document→chunk edge) now goes to that shard's `raft` with
  `namespace_id: ns_id` set on the event. All 4 writes route to the *same*
  shard, so the node-id cross-references between them (the edge referencing
  the doc/chunk node ids) stay valid — no cross-shard dangling references.
- **`cluster_list_nodes`** (read) — reads from `state.shard_for(ns_id).state_machine`
  instead of the flat shard-0 `state.sm`.
- **`cluster_memory_search`** (read) — same, both the plain and
  decay-reranked search branches route through the resolved shard's state
  machine. (`search_l2_ns` already existed and correctly filters by
  namespace at the kernel level — confirms reads were always structurally
  ready, only the write side was broken.)

**Deliberately not touched in this phase** (same reasoning as the original
S3 scope-out, now narrower since S3a fixed the root blocker):
`cluster_memory_consolidate`, `cluster_extract_entities`, `cluster_ingest`
still write to shard 0/whatever `ns_id` they resolve is silently unused
for routing purposes (mechanical extension of the exact same pattern once
picked back up — not a design question, just repetition); crypto-shredding
(`cluster_insert_encrypted`/`cluster_shred_key`/`cluster_crypto_status`)
deliberately excluded — shredding must fan out to every shard that might
hold a matching `key_id`, a real design question, not mechanical routing;
core CRUD (`/records`, `/search`, `/v1/delete`, `/v1/soft-delete`,
`/v1/vectors/batch-insert`) has no `collection` field in its wire format at
all; graph management (`/v1/graph/*`) and community endpoints have
get-by-id shapes with no namespace parameter to route from.

## Validation

**Full regression, zero failures:**
```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean (untouched)
cargo build --workspace --exclude valoricore-ffi                # clean
cargo test -p valori-kernel        # 62 passed
cargo test -p valori-consensus     # 74 passed, 1 ignored (pre-existing)
cargo test -p valori-node          # 233 passed (225 + 8 new: 4 shard_for_namespace + 4 already counted in cluster_namespaces.rs, see below)
cargo test -p valori-cli           # 11 passed
```

**New test — the flagship proof**
(`crates/valori-node/tests/cluster_namespaces.rs::writes_to_different_collections_route_to_different_shards`,
stable across repeated runs): a 3-shard single-node cluster, two
collections created (landing on shards 1 and 2 by construction — namespace
ids assigned sequentially, `1 % 3 = 1`, `2 % 3 = 2`), one record upserted
into each via real HTTP. Asserts each shard's own `KernelState` holds
exactly the record it should, and shard 0 (the namespace-registry shard)
holds zero data records — proving both the S3a correctness fix and the
S3b routing together, not just one or the other.

**Manual smoke test, live**, 3-shard single-node cluster:
```
POST /v1/namespaces {"name":"tenant-a"} → id 1
POST /v1/namespaces {"name":"tenant-b"} → id 2
POST /v1/memory/upsert {"vector":[...],"collection":"tenant-a"}
POST /v1/memory/upsert {"vector":[...],"collection":"tenant-b"}

GET /v1/graph/nodes?collection=tenant-a → 2 nodes, namespace_id:1
GET /v1/graph/nodes?collection=tenant-b → 2 nodes, namespace_id:2
GET /v1/graph/nodes?collection=default  → 0 nodes
```
Before S3a, all of these would have shown up under `default`/`namespace_id:0`
regardless of collection requested — confirmed by re-reading the pre-fix
code, not just inferred.

## Follow-ups

- **Extend S3b's pattern** to `cluster_memory_consolidate`,
  `cluster_extract_entities`, `cluster_ingest` — mechanical repetition of
  the exact pattern used for `cluster_memory_upsert`, once picked up.
- **Crypto-shredding cross-shard design** — `cluster_shred_key`/
  `cluster_crypto_status` need to fan out across every shard that might
  hold a matching `key_id` before `cluster_insert_encrypted` (already
  internally namespace-correct) can safely route non-zero-shard inserts.
  A GDPR-compliance-sensitive design question, not mechanical work.
- **Core CRUD collection field** — `/records`, `/search`, `/v1/delete`,
  `/v1/soft-delete`, `/v1/vectors/batch-insert` need an additive
  `collection: Option<String>` field before they can be namespace/shard
  aware at all.
- **Graph management + community endpoints** — need either a query-param
  namespace hint or a different id scheme before shard-routing is possible
  (get-by-id has no namespace to resolve a shard from).
- **Shard-aware linearizable read-index** — `/v1/cluster/read-index` in
  `cluster_api.rs` is shard-0-only; reads routed to non-zero shards
  currently get local (non-linearizable) consistency implicitly, since
  neither routed read handler calls `ensure_read_consistency` at all today
  (worth confirming/fixing explicitly in the next pass touching these
  handlers).
- **Composite external record/node/edge ids** — confirmed live in the
  manual smoke test above (`record_id: 0` appeared independently in both
  `tenant-a` and `tenant-b`'s data — correct per-shard, but ids are only
  unique within a shard, not globally). Unchanged from the S1 follow-up
  list; still deferred.
