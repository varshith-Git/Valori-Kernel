# Phase R2 — Dual-path unification: graph, records, meta, version domains

## Goal

Continue Phase R1's shared-handler migration end to end: move every domain
where the standalone/cluster duplication was real and mechanical into
`routes/`, closing the behavioral divergences found along the way.

## Delivered

| File | What |
|---|---|
| `crates/valori-node/src/routes/graph.rs` (new) | `GraphOps` trait + shared bodies for all 7 graph endpoints (`create_node`, `get_node`, `delete_node`, `list_nodes`, `create_edge`, `get_edges`, `get_subgraph`) incl. the shared `CollectionQuery` / `ListNodesQuery` / `SubgraphQuery` types. |
| `crates/valori-node/src/routes/records.rs` (new) | `RecordOps` trait + shared body for `POST /v1/delete` and `POST /v1/soft-delete` (soft/hard as a flag); receipt emission (`receipt_bridge`) lives in the shared body so both paths always emit. |
| `crates/valori-node/src/routes/meta.rs` (new) | `MetaOps` trait + shared bodies for `POST /v1/memory/meta/set` and `GET /v1/memory/meta/get`. |
| `crates/valori-node/src/routes/mod.rs` | Shared stateless `version()` handler used by both routers. |
| `crates/valori-node/src/api.rs` | `CreateNodeResponse` / `CreateEdgeResponse` / `DeleteNodeResponse` / `DeleteRecordResponse` gain `log_index: Option<u64>` (`skip_serializing_if` — the standalone wire format is byte-identical to before; the cluster keeps its log_index field). |
| `crates/valori-node/src/server.rs` | 11 handler bodies replaced by 3-line wrappers + `GraphOps`/`RecordOps`/`MetaOps` impls on `SharedEngine`. **New endpoint: `POST /v1/soft-delete`** (the engine had `soft_delete_record` all along — only the route was missing). |
| `crates/valori-node/src/cluster_server.rs` | 11 handler bodies replaced by wrappers + impls on `DataPlaneState`. **New endpoint: `DELETE /v1/graph/node/:id`** (commits `KernelEvent::DeleteNode` via Raft on the owning shard — closes the last METHOD_GAPS entry). 5 private duplicate DTO structs deleted (`CreateNodeRequest`, `CreateEdgeRequest`, `CollectionQuery`, `SubgraphQuery`, `ClusterListNodesQuery`, `DeleteRequest`, `MetaSetPayload`, `MetaGetQuery`). |
| `crates/valori-node/tests/route_parity.rs` | `METHOD_GAPS` now empty; `/v1/soft-delete` removed from `CLUSTER_ONLY` — both closures verified by the parity tests themselves. |

## Findings — divergences fixed by construction

1. **Tenant-isolation leak in cluster `GET /v1/graph/nodes`**: with no
   `collection` param the old cluster handler listed EVERY namespace's nodes;
   standalone scoped to the default namespace. Canonical: absent collection =
   default namespace (matches every other collection-aware endpoint).
2. **Invalid node/edge `kind` silently coerced on standalone**
   (`from_u8(..).unwrap_or_default()` turned garbage kinds into `Document`/
   default) while cluster 400'd. Canonical: 400 on both.
3. **`GET /v1/graph/edges/:id` for a missing node**: standalone answered
   200 + `{"edges":[]}`, cluster 404'd. Canonical: 404.
4. **`{"ok":true}` vs `{"success":true}`**: cluster `meta/set` used a
   different response envelope than standalone. Canonical:
   `MetadataSetResponse {success}`.
5. **Unknown `collection` on graph/delete endpoints**: standalone 400,
   cluster 404. Canonical: 404 (consistent with Phase R1's drop-collection
   decision).
6. **Missing routes now filled**: standalone `POST /v1/soft-delete`; cluster
   `DELETE /v1/graph/node/:id` (v1 + legacy).

Intentionally NOT migrated (documented, not forgotten):
* `insert`/`search`/`memory upsert-search`/`consolidate`/`contradict` — the
  write mechanics genuinely differ (standalone planner/effect graph vs Raft
  events; read-index consistency on cluster). Next candidates, but each needs
  its own design pass.
* `index/config` + `index/rebuild` — cluster mode intentionally runs kernel
  brute-force only (linearizable consistency); standalone has switchable
  indexes. Different semantics, not duplication.
* `proof`/`timeline`/`operations` — cluster merges per-shard audit logs
  (S16); standalone reads one log. Response shapes match; mechanics differ.

## Validation

- `cargo test -p valori-node` — **225 passed, 0 failed** (route_parity now
  verifies zero method gaps and the tightened allowlists).
- `cargo test -p valori-kernel` — **66 passed, 0 failed**.

## Follow-ups

- Migrate the memory domain (`upsert`, `search`, `consolidate`, `contradict`)
  — the largest remaining duplicated surface (~600 lines across both files).
- Migrate `search` once the read-consistency abstraction (local vs
  read-index) is designed into the trait.
- Python SDK: no changes required — existing methods work identically on both
  paths; `soft_delete` against standalone and graph-node delete against
  cluster now work where they previously 404/405'd.
