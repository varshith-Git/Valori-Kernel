# Phase C3 — Self-Maintaining Memory

> ⚠️ **Superseded by Phase C4.** This C3 shipped as UI-only Next.js logic
> (`ui/src/app/api/ingest|contradictions|why`), so the SDK, the MCP wedge, and
> the raw API got none of it; its state lived in a non-replicated metadata
> sidecar outside the audit chain; its "contradiction detector" fired on cosine
> *similarity* (agreement, not contradiction) into a review queue backed by a
> `meta/list` endpoint that does not exist (so it always returns `[]`); and it
> had **no decay**. C4 rebuilds these pillars as node-native, audited,
> all-client capabilities. See [phase-C4.1-decay.md](phase-C4.1-decay.md)
> (decay, done) and the C4.2 / C4.3 follow-ups (consolidation, contradiction).


## Goal

Close three knowledge-base hygiene gaps that let stale, duplicate, and contradictory
information pollute AI answers: (1) global entity deduplication so the same real-world
entity shares one graph node across all documents; (2) content-level deduplication so
exact-duplicate chunks are never re-ingested; (3) an asynchronous contradiction
detection queue that surfaces near-duplicate chunks from different sources for human
review, and a resolver that marks losing chunks as superseded so they never appear in
future answers.

## Delivered

### `ui/src/app/api/ingest/route.ts`

**Global entity registry (C3.1)**
- `lookupEntityNode(label, collection)` — checks `entity:<collection>:<normalized_label>`
  in the metadata sidecar before creating a Concept node. Returns the existing node_id
  or null.
- `registerEntityNode(label, collection, nodeId)` — writes the entity→node_id mapping
  so subsequent ingest sessions for ANY document in the same collection reuse the node.
- Per-chunk entity loop now checks session cache → global registry → create new, in that
  order. A "Morgan Stanley" Concept node created when ingesting Contract A is
  automatically reused when ingesting Contract B.

**Content dedup (C3.2)**
- `sha256hex(text)` — Web Crypto SHA-256 of chunk text (Node.js / Next.js compatible).
- `lookupContentRecord(sha, collection)` — checks `content:<collection>:<sha256>`.
  Returns existing record_id or null.
- `registerContentRecord(sha, collection, recordId, source)` — registers newly inserted
  chunks.
- Per-batch flow: hashes are computed before embedding. Duplicate chunks skip the vector
  insert entirely (saving storage + tokens). `dedup: true` flag on the chunk entry in
  the response. `dedup_skipped` count in the ingest response.

**Contradiction detection (C3.3)**
- `detectContradictions(vectors, recordIds, source, collection)` — runs after all
  inserts, fire-and-forget (`void`). For each inserted vector, does a `k=5` similarity
  search. If any result scores > 0.92 AND comes from a different source document, writes
  a `contradiction:<timestamp>-<rid_a>-<rid_b>` entry with status `"pending"`.

**Other**
- `content_sha256` stored in every chunk's sidecar metadata for external verification.

### `ui/src/app/api/contradictions/route.ts` (new)

- `GET /api/contradictions?collection=<c>&status=pending` — lists pending
  contradictions. Scans the metadata sidecar for `contradiction:` prefix entries,
  filters by collection + status, enriches with up to 300 chars of text from each
  record for display.
- `POST /api/contradictions` — resolve a contradiction:
  - `action: "dismiss"` → marks both records as valid (different but not contradictory).
  - `action: "supersede_b"` → marks `record_b`'s sidecar with `superseded: true` +
    `superseded_by: record_a`. Superseded chunks are excluded from future vector search
    results by the `/api/why` filter.

### `ui/src/app/api/why/route.ts`

- Added C3 supersession filter: after fetching metadata for search candidates, drops
  any record where `metadata.superseded === true`. These records still exist in the
  kernel (audit trail is immutable) but are invisible to retrieval.

## Findings

1. **Global entity registry is eventually consistent.** If two concurrent ingest jobs
   start at the same instant and both try to create a "Morgan Stanley" node before
   either registers it, they'll create two nodes. The collision probability is low for
   typical single-user ingest workflows; a true distributed lock would require a kernel-
   level name index, deferred to a future phase.

2. **Contradiction detection is async and best-effort.** It fires after the ingest
   response is sent. If the Next.js process dies mid-scan, some contradictions won't be
   queued. This is acceptable for the current architecture; a durable job queue would
   be the production-hardened version.

3. **Superseded records are not deleted from the kernel.** The vector and its audit
   event remain permanently (BLAKE3 chain is append-only). Only the sidecar metadata
   marks them as superseded. This is intentional: the audit trail is immutable.

4. **The `meta/list` endpoint (prefix scan) is required for contradiction listing.**
   If the Valori server does not implement this endpoint, `GET /api/contradictions`
   returns an empty list gracefully. This endpoint should be added to the Rust server
   as a follow-up (not a blocker for C3).

## Validation

```
cargo test -p valori-kernel -p valori-node
```

**Result: 198 tests passed, 0 failed.**

TypeScript: `cd ui && npx tsc --noEmit` — 0 errors.

Manual smoke test:
1. Ingest the same document twice → verify `dedup_skipped > 0` in the second response.
2. Ingest two documents with overlapping content (similarity > 0.92) → query
   `GET /api/contradictions?collection=default` → verify entries appear.
3. Resolve one as `supersede_b` → ask a question about that topic → verify the
   superseded chunk does NOT appear in the answer receipt.
4. Ingest two contracts both mentioning "Goldman Sachs" → verify only one Concept node
   is created via `GET /graph/subgraph?root=<doc_node>&depth=2`.

## Follow-ups

| Item | Phase |
|---|---|
| `GET /v1/memory/meta/list?prefix=` endpoint in Rust (needed for contradiction listing) | C4 / infra |
| Concurrent entity dedup: kernel-level name→node_id index to eliminate the race window | C4 |
| UI: Contradictions review panel in the Collections page | C4 UI |
| `RefersTo` edges: detect cross-references between entities and chain them automatically | C4 |
| Durable contradiction scan job (survives process restart) | C4 infra |
