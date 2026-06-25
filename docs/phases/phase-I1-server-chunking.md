## Goal

Add server-side document chunking to valori-node so any SDK or UI call gets identical, deterministic chunk boundaries without client-side text splitting logic.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/ingest.rs` | New module — `IngestDocumentRequest/Response`, `chunk_document()`, `ingest_document()` handler, plus `detect_strategy()`, `chunk_tree()`, `chunk_conversation()`, `chunk_sentence_window()`, `chunk_fixed()` |
| `crates/valori-node/src/lib.rs` | `pub mod ingest;` |
| `crates/valori-node/src/server.rs` | `POST /v1/ingest/document` route (stateless, no engine state) |
| `crates/valori-node/src/cluster_server.rs` | Same route added — works in cluster mode because handler carries no `State` |
| `python/valoricore/remote.py` | `SyncRemoteClient.chunk_document()` and `AsyncRemoteClient.chunk_document()` |
| `ui/src/app/api/ingest/route.ts` | `chunkTextTree()` TypeScript implementation + `chunkMode` parameter ("tree" | "fixed") defaulting to "tree" |
| `ui/src/components/ingestion/DocumentUploadTab.tsx` | Tree/fixed toggle UI, `chunkMode` state, sends `chunkMode` in FormData |

### Chunking strategies

| Strategy | Trigger | Logic |
|---|---|---|
| `auto` | default | >10% header lines → tree; >20% timestamps OR (q>3 AND ts>3) → conversation; else fixed |
| `tree` | forced | Split on numbered sections (`3.1 Title`) and markdown headers (`## Header`); title prepended to body |
| `conversation` | forced | Split on question-boundary lines (ending `?`); groups each Q+A block as one chunk |
| `sentence` | forced | Each sentence = one retrieval unit; stored text includes ±2 surrounding sentences |
| `fixed` | forced | Overlapping char windows (default 1000/200); sentence-boundary snapping |

### Endpoint

```
POST /v1/ingest/document
{ "text": "...", "strategy": "auto|tree|conversation|sentence|fixed",
  "collection": "default", "source": "filename.pdf",
  "chunk_size": 1000, "chunk_overlap": 200 }
→ { "strategy_used": "tree", "chunk_count": 31,
    "chunks": [{"index": 0, "title": "3.1 Training", "text": "..."}, ...] }
```

## Findings

- The `ingest_document` handler must be **stateless** (no `State<>` parameter) to compile in both standalone (`State<SharedEngine>`) and cluster (`State<DataPlaneState>`) servers simultaneously — confirmed by the type-mismatch error fixed during implementation.
- `chunk_conversation` strips leading timestamps (`HH:MM:SS`, `[MM:SS]`) before splitting on `?` — otherwise Zoom/Teams transcripts produce zero question boundaries.
- Tree fallback threshold is 2 headers; below that the document has no detectable structure and fixed-size chunking is used without user-visible error.

## Validation

- Cargo tests: **237 passed, 0 failed** (`cargo test -p valori-kernel -p valori-node`)
- TypeScript: `npx tsc --noEmit` — no errors
- Manual: uploaded `firstone--final` collection (84 → 32 tree chunks); "AdamW optimizer" question answered correctly after switching from fixed to tree

## Follow-ups

- `chunk_sentence_window` sentence splitter is a naive `split_terminator(['.','!','?'])` — a proper sentence segmenter (e.g. via `lindera` or NLTK-on-server) would handle abbreviations. Deferred to a future NLP phase.
- Conversation chunker does not handle multi-speaker interleaving (e.g. `Alice: … Bob: …`). Speaker-aware splitting is deferred.
