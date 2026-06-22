# Phase C2 — Audited Entity Graph + Provenance Receipt

## Goal

Extend Valori Cortex with a knowledge graph layer: extract named entities from
each ingested chunk, commit them as audited Concept nodes with Mentions edges,
add a bounded BFS subgraph endpoint to the Rust server, and include the traversed
graph nodes and edges in every proof receipt.

## Delivered

### `crates/valori-node/src/server.rs` (standalone)

- `GET /graph/subgraph?root=<node_id>&depth=<d>` — bounded BFS from any graph node.
  Depth is capped at 4. Returns `{ nodes: [...], edges: [...] }`. Both outgoing
  edges and neighbour nodes are deduplicated via `HashSet`.

### `crates/valori-node/src/cluster_server.rs` (cluster)

- Same `GET /graph/subgraph` endpoint wired to the Raft state machine via
  `with_state()`. Respects the readiness gate (returns 503 during catch-up).

### `ui/src/app/api/ingest/route.ts`

- `extractEntities(chunkText, llmConfig)` — calls the same LLM as context
  enrichment; returns up to 8 named entities as strings. Strips markdown code
  fences from the response; returns `[]` on any failure (graceful degrade).
- Per-chunk entity loop (inside the existing per-batch loop, after chunk node
  creation): for each extracted entity, looks up `entityNodeMap` (session-scoped
  `Map<string, number>`) to reuse an existing Concept node or creates a new one
  (`NodeKind::Concept = 1`). Stores the entity label in the sidecar metadata
  (`target_id: node:<id>`). Creates a `Mentions` edge (`EdgeKind::Mentions = 4`)
  from the chunk node to the concept node.
- `Document→Chunk` edge kind corrected from `0` (Relation) to `6` (ParentOf).
- `insertedChunks` array now includes `entities: string[]` per chunk.
- Chunk sidecar metadata gains `entities: string[]` field.

### `ui/src/app/api/why/route.ts`

- Provenance subgraph collection block added after the graph expansion loop.
  For each of the top-5 ranked results that has a `chunk_node_id`, calls
  `GET /graph/subgraph?root=<chunk_node>&depth=1`. Fetches entity labels for
  Concept nodes (kind=1) via the sidecar. Deduplicates nodes/edges across chunks
  via `seenNodeIds`/`seenEdgeIds` sets.
- `provenance_nodes` and `provenance_edges` arrays are included in the receipt.

### `ui/src/lib/receipts.ts`

- New exported types: `ReceiptGraphNode { id, kind, label }` and
  `ReceiptGraphEdge { id, from, to, kind }`.
- `ServerReceiptPart` gains optional `provenance_nodes` and `provenance_edges`.
- `AnswerReceipt` gains required `provenance_nodes: ReceiptGraphNode[]` and
  `provenance_edges: ReceiptGraphEdge[]`.
- `finalizeReceipt` maps `provenance_nodes` and `provenance_edges` from the
  server part (defaulting to `[]` if absent for backward compatibility).

## Findings

1. **`Document→Chunk` edge kind was wrong.** The existing ingest route was
   creating this edge with kind `0` (Relation), not kind `6` (ParentOf). Fixed
   in this phase.

2. **Entity deduplication is session-scoped, not global.** Two separate ingest
   runs for the same document will create duplicate Concept nodes for the same
   entity. A global entity registry (name → node_id lookup in the kernel) would
   require a new kernel index and is deferred to C3/C4.

3. **Provenance subgraph is depth=1.** This captures chunk → concept edges. A
   depth-2 walk would also capture concept → concept (RefersTo) chains — useful
   for contract cross-references. Depth is configurable via the query param;
   the receipt always reflects what was actually traversed.

4. **Entity extraction only runs when enrichment is enabled.** The same
   `llmEnrich` guard controls both context sentence generation and entity
   extraction. This avoids LLM calls for users who haven't opted in.

## Validation

```
cargo test -p valori-kernel -p valori-node
```

**Result: 198 tests passed, 0 failed.**

TypeScript: `cd ui && npx tsc --noEmit` — 0 errors.

Manual smoke test (requires running node + ollama with entity-capable model):
1. Enable contextual enrichment in DocumentUploadTab → upload a contract PDF
2. Check the ingest response for `entities: [...]` per chunk
3. Ask a question → inspect the receipt `provenance_nodes` array
4. Verify that Concept node labels match entities extracted at ingest time
5. Call `GET /graph/subgraph?root=<chunk_node>&depth=2` directly and verify the
   BFS returns concept nodes at depth 1 and any RefersTo chains at depth 2

## Follow-ups

| Item | Phase |
|---|---|
| Global entity registry: name → concept node_id index so duplicate entities across documents share a node | C3 |
| `EdgeKind::RefersTo` chains: at ingest, detect cross-references between entities and create RefersTo edges | C3 |
| UI: show provenance graph (node/edge diagram) in the answer receipt panel | C3 UI |
| Exact-dedup auto-tombstone | C3 |
| NLI contradiction review queue | C3 |
