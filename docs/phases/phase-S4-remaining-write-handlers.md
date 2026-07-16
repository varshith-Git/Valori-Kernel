# Phase S4 — Extend shard routing to the remaining write handlers

Branch: `Node-scaleup` (S1 `6d53924`, S2 `08dd043`, S3 `0460cee` merged).

## Goal

S3's follow-ups explicitly deferred three write handlers that needed the
same namespace-correct, shard-routed pattern proven out for
`cluster_memory_upsert`: `cluster_memory_consolidate`,
`cluster_extract_entities`, and `cluster_ingest`. This phase closes that
gap — every collection-aware write handler in `cluster_server.rs` now
routes to the shard that actually owns its target namespace's data.

## Delivered

### `cluster_memory_consolidate` (soft-delete old + insert new + 2 nodes + edge)

All 4 `raft_write_data` calls now go through `state.shard_for(ns_id).raft`
with `namespace_id: ns_id` set, instead of the flat shard-0 `state.raft`
with `namespace_id: 0`. Documented assumption (matches the endpoint's
existing semantics, not a new constraint): `old_record_id` must already
live in the same collection being consolidated into — true for any record
created via a namespace-aware path, since "consolidate" replaces a record
within its own collection, never moves it across collections.

### `cluster_extract_entities` (LLM entity extraction → records + nodes + edges)

Beyond routing, this handler had a **pre-existing race condition** that
became a correctness bug once routing was added: it pre-read "next
record/node id" from `s.sm` (the flat shard-0 state machine) *before*
submitting the write, then used that guessed id to link the node to its
record. Once writes route to a non-zero shard, that guess is wrong — each
shard has its own independent id counter, so a guess taken from shard 0
would not match the id the target shard actually allocates. Fixed by
switching to `raft_write_data`'s real allocated-id-from-response pattern
(the same pattern `cluster_memory_upsert`/`cluster_memory_consolidate`
already use) — this both fixes the race and makes routing safe. Also
tightened error handling: entity/relationship writes that fail now skip
that item and count it as `skipped` rather than silently reporting a
fabricated id.

### `cluster_ingest` (chunk + embed + insert N records + doc/chunk nodes + edges + metadata)

All `client_write` calls after namespace resolution — the per-chunk
`AutoInsertRecord` loop, the document node, each chunk node + `ParentOf`
edge, and both `SetMeta` metadata-sidecar writes — now go through
`state.shard_for(ns).raft` with `namespace_id: ns`. The metadata sidecar
writes are routed to the same shard as the rest of the document
(`KernelState.meta` is per-shard, so keeping a document's data and its
metadata sidecar entries co-located on one shard is correct — a document
never needs data split across two shards).

## Validation

```
cargo build -p valori-kernel --target wasm32-unknown-unknown   # clean (untouched)
cargo build --workspace --exclude valoricore-ffi                # clean
cargo test -p valori-kernel        # 62 passed
cargo test -p valori-consensus     # 74 passed, 1 ignored (pre-existing)
cargo test -p valori-node          # 234 passed (233 + 1 new: consolidate_routes_to_the_collections_shard)
cargo test -p valori-cli           # 11 passed
```

New test:
`crates/valori-node/tests/cluster_namespaces.rs::consolidate_routes_to_the_collections_shard`
— upserts into `tenant-a` (shard 1 of 3), consolidates the resulting
record, and asserts the new (live) record ends up on tenant-a's own shard
with shard 0 untouched. Stable across repeated runs.

`cluster_ingest` was not covered by a new automated test — it requires a
mocked embed provider (`VALORI_EMBED_PROVIDER`/HTTP mock) to exercise at
all, disproportionate to add for this pass. Verified instead by code
review (identical pattern to the two tested handlers, same
`state.shard_for()`/`namespace_id` wiring) and live manual smoke test
below.

**Manual smoke test, live**, 3-shard single-node cluster: upsert into
`tenant-a`, then consolidate that record within the same collection —
`GET /v1/graph/nodes?collection=tenant-a` shows all 4 resulting nodes
(original document, original chunk, new chunk, — the consolidate call's
two `AutoCreateNode`s) at `namespace_id:1`, `default` stays empty.

## Follow-ups (unchanged from S3, still open)

- `cluster_ingest` still lacks direct automated coverage — worth adding
  once there's a lightweight embed-provider test double in this test
  suite (may already exist for other ingest tests; wasn't investigated
  this pass).
- Crypto-shredding cross-shard design
  (`cluster_insert_encrypted`/`cluster_shred_key`/`cluster_crypto_status`).
- Core CRUD collection field (`/records`, `/search`, `/v1/delete`,
  `/v1/soft-delete`, `/v1/vectors/batch-insert`).
- Graph management + community endpoints (no namespace param to route from).
- Shard-aware linearizable read-index.
- Composite external record/node/edge ids (still per-shard-unique only).
- `cluster_tree_hybrid`'s namespace-scoped query section — identified back
  in S1's original investigation as collection-aware but never revisited;
  worth confirming whether it needs the same routing treatment.
