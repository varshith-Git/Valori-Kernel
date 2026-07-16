# Phase I8 — Document Update with Chunk-Level Diffing

## Goal

Add a `POST /v1/ingest/update` endpoint that lets callers update a
previously ingested document without re-embedding unchanged chunks.
Uses BLAKE3 content hashing to diff old vs new chunks at the text
level — only genuinely new or modified chunks hit the embedding
provider.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/ingest.rs` | `IngestUpdateRequest`, `IngestUpdateResponse`, `ingest_update()` standalone handler, `chunk_content_hash()` (BLAKE3 of chunk text), `collect_old_chunks()` (walks Document→Chunk graph edges + reads metadata) |
| `crates/valori-node/src/server.rs` | Route registration: `POST /v1/ingest/update` → `crate::ingest::ingest_update` |
| `crates/valori-node/src/cluster_server.rs` | `cluster_ingest_update()` — same contract, all writes via `raft.client_write()`, shard-routed; route registration |
| `python/valoricore/remote.py` | `ingest_update()` on both `SyncRemoteClient` and `AsyncRemoteClient` |

### Endpoint contract

```
POST /v1/ingest/update
{
  "document_node_id": 42,       // from original /v1/ingest response
  "text": "...",                 // new full text
  "collection": "default",      // optional
  "strategy": "auto",           // optional
  "source": "report-v2.pdf",    // optional
  "chunk_size": 1000,           // optional
  "chunk_overlap": 200          // optional
}

→ 200
{
  "ok": true,
  "document_node_id": 42,
  "strategy_used": "tree",
  "new_chunk_count": 35,
  "kept_count": 28,             // unchanged chunks (not re-embedded)
  "removed_count": 3,           // old chunks soft-deleted
  "added_count": 7,             // new chunks embedded + inserted
  "record_ids": [1,2,...35],    // all live chunks after update, in order
  "collection": "default"
}
```

### Algorithm

1. Chunk the new text using the same strategy dispatcher as `/v1/ingest`
2. BLAKE3 content-hash every new chunk (`blake3::hash(text.as_bytes())`)
3. Walk the Document node's outgoing `ParentOf` edges to find all existing
   Chunk graph nodes → read each chunk's text from the metadata sidecar →
   content-hash it
4. Diff by hash:
   - **Kept** — old hash matches a new hash: chunk stays in place, no re-embed
   - **Removed** — old hash not in the new set: `soft_delete_record` (standalone)
     or `SoftDeleteRecord` via Raft (cluster), which auto-cascades the graph node
   - **Added** — new hash not matched by any old chunk: embed → insert record →
     create Chunk graph node → ParentOf edge → metadata sidecar
5. Update document-level metadata (`updated_at`, new `total_chunks`, etc.)
6. Return the diff summary + ordered list of all live record IDs

### Design decisions

- **Reuses the Document node** — the same `document_node_id` gains new
  ParentOf edges for added chunks. External edges (e.g. community, entity)
  pointing to the Document node remain valid.
- **Content hash stored in metadata** — each chunk's `content_hash` (hex
  BLAKE3) is persisted in the metadata sidecar so future updates can diff
  without re-reading chunk text from the store.
- **Cluster path is shard-routed** — `shard_for_namespace(ns, shard_count)`
  routes all reads and writes to the correct shard, consistent with S3-S9.

## Findings

- No issues found. The existing `soft_delete_record()` already
  auto-cascades the associated graph node (via `record_to_node` map),
  so removed chunks don't leave orphaned graph nodes.
- The metadata sidecar stores chunk text under `record:{rid}` — this is
  the same key the reranker and Ask UI read from, so no additional
  lookups are needed.

## Validation

- `cargo build -p valori-node` — clean (zero errors, zero warnings in node)
- `cargo test -p valori-kernel -p valori-node` — 282 tests passing, 0 failures
- `cargo build -p valori-kernel --target wasm32-unknown-unknown` — clean
  (kernel `no_std` invariant preserved; no kernel changes)

## Follow-ups

- **Content-hash on initial ingest** — store the BLAKE3 content hash
  in metadata during `/v1/ingest` so the first update doesn't need to
  re-hash from the text field. Low priority (re-hashing text is fast).
- **Partial update API** — accept a list of changed sections instead
  of the full document text, for very large documents where the caller
  already knows what changed.
- **UI integration** — surface the update flow in the Documents tab
  (re-upload button that calls `ingest_update` with the existing
  `document_node_id`).
