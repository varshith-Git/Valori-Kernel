# Phase C1 — Contextual Retrieval + Audited Enrichment

## Goal

Store LLM-generated context sentences inside the committed `AutoInsertRecord.metadata`
bytes — audited, replicated through Raft, and hashed into the BLAKE3 chain — closing
the gap between "RAG with text metadata" and "provably-grounded embeddings."
Add an optional Tier-2 reranker whose scores are logged in the proof receipt.

## Delivered

### `crates/valori-node/src/api.rs`

- `BatchInsertRequest` gains `metadata: Option<Vec<Option<String>>>` — one UTF-8 string
  per vector. `#[serde(default)]` keeps the field optional for existing callers.

### `crates/valori-node/src/engine.rs`

- `insert_batch_ns` signature extended: `metadata: Option<&[Option<Vec<u8>>]>`.
- Each `InsertRecord` event is now constructed with the per-index metadata blob when
  present. The blob is committed to the WAL and included in the BLAKE3 audit chain.
- `insert_batch` wrapper updated: passes `None` (no metadata, backward-compatible).

### `crates/valori-node/src/server.rs`

- Standalone `batch_insert` handler: decodes `payload.metadata` strings to UTF-8 bytes
  and passes them to `insert_batch_ns`.

### `crates/valori-node/src/cluster_server.rs`

- Local `BatchInsertRequest` gains `metadata: Option<Vec<Option<String>>>`.
- `batch_insert` handler: extracts the per-index metadata string and encodes as UTF-8
  bytes into `AutoInsertRecord { metadata: meta_bytes, .. }`.
- **This was the blocking C1 dependency** — previously always `metadata: None`.

### `crates/valori-node/tests/api_batch_ingest.rs`

- Two test struct literals updated: `BatchInsertRequest { .., metadata: None }`.

### `ui/src/app/api/ingest/route.ts`

- `generateContextSentence()` — calls the configured LLM (ollama, openai, groq,
  together, custom) with a one-sentence context prompt. Returns `null` on failure
  (degraded mode: ingest continues unenhanced).
- `generateContextBatch()` — runs up to 6 concurrent context calls using
  `Promise.allSettled`; failures for individual chunks don't abort the batch.
- Context sentence format: `{"doc":"<filename>","n":<idx>,"total":<total>,"ctx":"<sentence>"}`
  stored as UTF-8 in the committed event `metadata` field.
- Sidecar metadata gains `context_sentence` and `enriched: true` fields for display.
- New form fields accepted: `enrichEnabled`, `llmProvider`, `llmModel`, `llmApiKey`,
  `llmEndpoint` — passed from `DocumentUploadTab`.

### `ui/src/app/api/why/route.ts`

- `rerankChunks()` — calls Cohere `/v2/rerank` or a custom endpoint after vector search.
  Failure is silent (falls through, all `rerank_score: null`).
- `WhyRequest` gains `reranker?: RerankerConfig`.
- Answer path uses `rankedResults` (reranked if available, otherwise original order).
- LLM synthesis prompt includes `context_sentence` from sidecar when present.
- Receipt chunks gain `rerank_score: number | null` and `enriched: boolean`.
- Receipt response gains `reranked: boolean` flag.

### `ui/src/lib/receipts.ts`

- `ReceiptChunkRef` gains `rerank_score: number | null` and `enriched: boolean`.
  Both are additive (non-breaking) within schema version `"1.0"`.

### `ui/src/components/ingestion/DocumentUploadTab.tsx`

- `enrichEnabled` toggle state added.
- Form data submission appends `enrichEnabled` and (when true) LLM params.
- Contextual enrichment toggle UI added between dim-mismatch warning and drop zone.

### `ui/src/components/collections/AskTab.tsx`

- Loads reranker config from `localStorage["valori:reranker_config"]` on mount.
- Passes `reranker` config to `/api/why` when a provider is configured.

### `ui/src/app/settings/page.tsx`

- New "Tier-2 Reranker" section (disabled / Cohere / custom).
- Config persisted in `localStorage["valori:reranker_config"]`.

## Findings

1. **Degraded mode is safe.** If the LLM is unavailable at ingest time, chunks are
   committed with `metadata: None` (unchanged from pre-C1 behavior). The ingest
   never blocks or fails due to enrichment. The `enriched` flag in the receipt
   lets auditors know which chunks were enriched.

2. **Reranker non-determinism is documented, not hidden.** The `reranked: true` flag
   on the receipt and `rerank_score` per chunk make it explicit that chunk ordering
   was changed by a non-deterministic external call.

3. **Context format is JSON, not raw text.** `{"doc","n","total","ctx"}` is more
   machine-readable than a plain string, and keeps the blob compact.

4. **Cluster path previously discarded all metadata.** Fixed: `cluster_server.rs`
   now indexes into `req.metadata` by the current `ids.len()` position (pre-push)
   to pick the correct per-vector blob.

## Validation

```
cargo test -p valori-kernel -p valori-node
```

**Result: 198 tests passed, 0 failed** (same count as C0 — two test struct literals
updated, no logic regressions).

TypeScript: `cd ui && npx tsc --noEmit` — 0 errors.

End-to-end smoke test (requires running node + ollama):
1. Enable "Contextual enrichment" in DocumentUploadTab → upload a PDF
2. Verify each chunk's receipt shows `enriched: true`
3. Ask a question → verify `reranked: false` (no reranker configured)
4. Set Cohere reranker in Settings → ask again → verify `reranked: true` and
   non-null `rerank_score` per chunk in the receipt JSON

## Follow-ups

| Item | Phase |
|---|---|
| Eval C1 improvement: run `seed-eval` after a real corpus ingest with enrichment enabled, compare recall@5 to C0 baseline | C1 measurement |
| `/v1/namespaces/:ns/graph/subgraph` bounded BFS endpoint | C2 |
| Entity extraction at ingest → `AutoCreateNode { kind: Concept }` + `AutoCreateEdge { kind: Mentions }` | C2 |
| Provenance subgraph in answer receipt | C2 |
| Exact-dedup auto-tombstone | C3 |
