# Phase S6 — Shard-aware linearizable read-index

Branch: `Node-scaleup` (S1 `6d53924`, S2 `08dd043`, S3 `0460cee`, S4 `809b87a`, S5 merged).

## Goal

`ensure_read_consistency()` (the follower→leader read-index protocol behind
linearizable reads) was hardcoded to shard 0 in every call site, regardless
of which shard the read actually needed to be consistent against. Once
S3b/S4 started routing real reads to non-zero shards
(`cluster_memory_search`, `cluster_graphrag`), a linearizable read against
`tenant-a`'s shard 1 was silently establishing a read index against shard
0's Raft group instead — consistent with the *wrong* group. This phase
makes the read-index protocol shard-aware end to end.

## Delivered

### `ensure_read_consistency` — shard parameter added

Signature changed from `(raft, http)` to `(shard_id: ShardId, raft, http)`.
The leader fast path (`raft.ensure_linearizable()`) already operated on
whichever `raft` handle was passed in, so it needed no change — only the
follower path, which calls out to the *leader's* `/v1/cluster/read-index`
endpoint, needed to say which shard it's asking about. The URL gained a
query parameter: `GET /v1/cluster/read-index?shard={id}`.

### `/v1/cluster/read-index` — shard query param

`cluster_api.rs`'s `read_index` handler now takes
`Query<ReadIndexQuery { shard: u32 }>`, looks up `ShardId(q.shard)` in a new
`ClusterApiState.shards: Arc<BTreeMap<ShardId, Raft>>` map, and 404s on an
unknown shard id instead of silently answering for the wrong group. The
response body gained a `"shard"` field so callers can confirm which
shard's read index they received.

`cluster_router()`'s signature changed to accept this map:
`cluster_router(raft: Arc<Raft>, shards: Arc<BTreeMap<ShardId, Raft>>, audit)`.
`build_cluster_router_with_keys` builds `api_shards` from
`handle.shards` **before** `state` (which owns the same shard handles) is
moved into `.with_state(state)` — the map has to be built from the
`ClusterHandle` directly, not re-derived from `DataPlaneState` after the
move.

### Call sites updated

- `search` (core CRUD, `cluster_server.rs`): now resolves `ns_id` from
  `collection` (S7 landed in the same working session, so this call site
  reflects the final shard-aware form) and passes
  `shard_for_namespace(ns_id, state.shard_count)` + that shard's `raft`.
- `cluster_graphrag`: same treatment — resolves `ns_id`, passes its shard.
- `cluster_memory_search`: gained a linearizable-consistency check it never
  had before (previously read straight from local state with no read-index
  step at all) — see Findings below.

## Findings

- `cluster_memory_search` had **no read-index call whatsoever** before this
  phase, despite accepting a `consistency` field on `MemorySearchVectorRequest`
  (added earlier but never wired to anything). Every `/v1/memory/search`
  read was silently eventually-consistent regardless of what the caller
  requested. Fixed as part of this phase: `if payload.consistency != Some("local")`
  now calls `ensure_read_consistency` before reading local state, using the
  correct shard.
- The `cluster_router()` signature change broke 8 call sites in
  `crates/valori-node/tests/cluster_api.rs` that only surfaced under
  `cargo test -p valori-node --tests` — `cargo build -p valori-node` (lib
  only) does not compile test files, so a lib-only build after a shared-type
  signature change is not sufficient to catch every consumer. This is the
  same lesson S1 learned about `-p valori-node -p valori-consensus` missing
  `valori-cli` regressions; the corollary here is lib-vs-test-vs-full-build
  scope, not just crate scope.

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean (untouched)
cargo test -p valori-kernel        # 62 passed
cargo test -p valori-consensus     # 74 passed, 1 ignored (pre-existing)
cargo test -p valori-node          # 233 passed (full workspace, this session's cumulative S5-S9 total)
cargo test -p valori-cli           # 11 passed
```

New test:
`crates/valori-node/tests/cluster_namespaces.rs::memory_search_is_linearizable_on_a_non_zero_shard`
— proves both the default linearizable path and the explicit `"local"`
opt-out work for a collection resolved to a non-zero shard, and that
`GET /v1/cluster/read-index?shard=N` returns `{"shard": N}` for the
requested shard specifically (not always shard 0).

## Follow-ups

- The read-index protocol is now shard-aware everywhere it's called from
  this phase's edited handlers, but any *future* handler that reads
  namespace-scoped state must remember to pass its own shard's `ShardId` —
  there's no compile-time enforcement that a new call site gets this right,
  only convention (matching `state.shard_for(ns_id)`).
- Core CRUD's `collection` field + shard routing was still pending at the
  start of this phase — delivered in the same working session as S7, see
  `phase-S7-core-crud-routing.md`.
