# Phase C4.2 — Memory consolidation (self-maintaining memory, pillar 2)

## Goal

Let a client replace an existing memory with an updated one as a single,
auditable operation — soft-deleting the old record, inserting the new vector,
and recording *why* (a Supersedes link) all as committed events in the BLAKE3
chain. This is the second self-maintaining-memory pillar (decay →
**consolidation** → contradiction), built node-native rather than in a UI route.

## Delivered

### New EdgeKind — `crates/valori-kernel/src/types/enums.rs`

| Variant | Value | Meaning |
|---|---|---|
| `EdgeKind::Supersedes` | 7 | New record supersedes an old one (consolidation). |

`from_u8` round-trips it. (Phase C4.3 adds `Contradicts = 8` in the same edit.)
`no_std`-safe — pure enum, no std dependency.

### Standalone endpoint — `POST /v1/memory/consolidate` (`crates/valori-node/src/server.rs`)

Request `{ old_record_id, new_vector, collection?, metadata? }`. The handler
holds the engine write lock across all three mutations, so they are atomic w.r.t.
other writers, and commits, in order:

1. `SoftDeleteRecord(old_record_id)` — old record preserved in the chain,
   excluded from search.
2. `AutoInsertRecord(new_vector)` — the replacement; id assigned by the engine.
3. `AutoCreateEdge(new_node → old_node, Supersedes)` — graph link making the
   replacement auditable. Chunk nodes are created for both record ids first.

Response `{ old_record_id, new_record_id, supersedes_edge_id, state_hash }`
(`state_hash` is the post-consolidation BLAKE3 root, hex).

### Cluster endpoint — `POST /v1/memory/consolidate` (`crates/valori-node/src/cluster_server.rs`)

Same surface, backed by Raft. Each of the four mutations is a separate
`ClientRequest` committed through `raft_write_data` (a new helper returning the
committed `ClientResponse`). **Allocated IDs are read from each apply response**,
never pre-read in a separate await — that would race a concurrent writer for the
same id. Namespace is resolved via the cluster `NamespaceRegistry`.

### Python SDK — `python/valoricore/remote.py`

`consolidate(old_record_id, new_vector, collection=, metadata=)` on
`SyncRemoteClient`, `AsyncRemoteClient`, `ClusterClient`, `AsyncClusterClient`.
Cluster variants route to the leader (`_write_client`).

## Findings

- **The cluster path is not a single atomic transaction.** It is four sequential
  Raft commits. A leader crash mid-sequence can leave a partial consolidation
  (e.g. old record soft-deleted, new one inserted, edge missing). Each individual
  event is still chain-valid and replicated; the *composite* is not transactional.
  The standalone path **is** atomic (single write lock). Cross-event atomicity in
  cluster mode would need a multi-event `ClientRequest` variant — deferred.
- The original cluster draft pre-read `next_record_id()`/`next_node_id()` in a
  separate await before committing — a TOCTOU race under concurrent writes. Fixed
  during this phase by adding `raft_write_data` and threading allocated IDs out of
  the apply responses.
- Graph nodes are global in the kernel (no `namespace_id` on `AutoCreateNode`);
  the namespace is resolved for validation but the chunk nodes are not
  namespace-scoped. Consistent with existing `memory_upsert_vector` behaviour.

## Validation

- `cargo test -p valori-node` — 193 passed, 0 failed.
- `cargo test -p valori-kernel -p valori-consensus` — 112 passed, 0 failed.
- `cargo build -p valori-kernel --target wasm32-unknown-unknown` — clean.
- `python3 -c "import ast; ast.parse(...)"` — `remote.py` parses.
- Manual smoke test (standalone): insert a record, `POST /v1/memory/consolidate`
  with a new vector → response carries new id + edge id; old record no longer in
  search results; `GET /graph/edges/<new_node>` shows the Supersedes edge;
  `/v1/timeline` shows SoftDeleteRecord + AutoInsertRecord + AutoCreateEdge.

## Follow-ups

- Atomic multi-event commit for cluster consolidation (single `ClientRequest`
  carrying the 3-event sequence) so a mid-sequence leader crash cannot leave a
  partial result. Owner: future phase.
- `/v1/memory/consolidate` does not yet carry the old record's metadata forward
  automatically; the caller passes new `metadata` explicitly. A "merge metadata"
  option could be added.
