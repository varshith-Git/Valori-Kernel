## Goal

Wire `POST /v1/ingest` into cluster mode so the full chunk → embed → insert pipeline works across a 3-node Raft cluster, with every vector and graph mutation replicated to all peers.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/cluster_server.rs` | `DataPlaneState` gains `embed_config: Option<EmbedConfig>` and `metadata: Arc<MetadataStore>`; `build_cluster_router_with_keys` accepts `node_cfg: &NodeConfig`; `build_cluster_router` reads `NodeConfig::default()` and passes it through; `health` handler exposes `embed_enabled` + `embed_provider`; new `cluster_ingest()` handler; `POST /v1/ingest` route added to cluster router |

### How the cluster pipeline works

```
POST /v1/ingest  →  cluster_ingest()
  1. chunk_document()          stateless, same as standalone
  2. embed_batch()             HTTP call to embed provider (stateless)
  3. raft.client_write(        one per chunk — replicated to all 3 nodes
       AutoInsertRecord)       ValoriStateMachine::apply() → KernelState on each
  4. raft.client_write(        document graph node
       AutoCreateNode)
  5. raft.client_write(        chunk node + ParentOf edge per chunk
       AutoCreateNode + Edge)
  6. state.metadata.set()     node-local sidecar (chunk text, source, model)
```

Every `client_write` goes through Raft consensus — the leader sequences it, all peers apply it in the same order, and every node ends up with bit-identical `KernelState` and the same BLAKE3 state hash.

### What's replicated vs node-local

| Data | Replicated via Raft | Node-local |
|---|---|---|
| Vectors (FxpScalar) | ✅ | — |
| Graph nodes + edges | ✅ | — |
| Chunk text in AutoInsertRecord metadata bytes (for reranker) | ✅ | — |
| Metadata sidecar (chunk text, source, section_title, embed_model…) | — | ✅ (DataPlaneState::metadata) |

The metadata sidecar is node-local by design — it's advisory display data (same as standalone). The BLAKE3 audit chain only covers the kernel events, not the sidecar.

### `build_cluster_router` change

`build_cluster_router` now reads `NodeConfig::default()` internally and passes it to `build_cluster_router_with_keys`. This means the cluster router automatically picks up `VALORI_EMBED_*` env vars on startup without any caller change in `main.rs`.

## Findings

- `NamespaceRegistry::create()` is idempotent (returns existing id if already registered), so calling it from the ingest handler auto-creates the namespace if missing — consistent with standalone `Engine::resolve_collection()` behaviour.
- `AutoInsertRecord.tag` is `u64` in the cluster path (not `u32` as in standalone) — cast corrected.
- The metadata sidecar is not Raft-replicated. If the operator queries `/v1/memory/meta/get?target_id=record:N` on a follower that didn't run the ingest, the sidecar will be empty. Full sidecar replication requires a `KernelEvent::SetMeta` variant — deferred to a future phase.

## Validation

- Cargo tests: **237 passed, 0 failed** (`cargo test -p valori-kernel -p valori-node`)
- Build: `cargo build -p valori-node` — clean
- Checklist from CLAUDE.md:
  - [x] Added to `server.rs` (standalone, Phase I2)
  - [x] Added to `cluster_server.rs` (cluster, this phase)
  - [x] Cluster path uses `raft.client_write()` — no direct engine lock
  - [x] Embed config accessible from `DataPlaneState`
  - [x] `/health` exposes `embed_enabled` + `embed_provider` in cluster mode
  - [x] Python SDK unchanged — same endpoint, same response shape

## Follow-ups

- Metadata sidecar replication: add `KernelEvent::SetMeta { key, value }` so all nodes share the chunk text sidecar. Currently only the node that received the ingest request has it.
- `build_cluster_router_with_keys` now has a `node_cfg` parameter — callers that pass custom auth tokens (e.g. tests) may need updating if they call it directly.
- Batch Raft writes: currently one `client_write` per chunk. Grouping all chunks into a single Raft log entry would reduce round-trips for large documents.
