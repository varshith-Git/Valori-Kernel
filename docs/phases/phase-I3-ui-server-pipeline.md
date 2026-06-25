## Goal

Wire the UI document upload flow through the node's `/v1/ingest` endpoint when the node has an embed provider configured, with automatic fallback to the existing client-side pipeline when it does not.

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/engine.rs` | Added `embed_enabled: bool` and `embed_provider: Option<String>` to `EngineHealth` struct + populated in `Engine::health()` |
| `ui/src/app/api/ingest/route.ts` | `probeServerIngest()` helper probes `/health` for `embed_enabled`; fast path POSTs text to node's `/v1/ingest` and normalises response shape; slow path (existing client-side embed) remains as fallback |
| `ui/src/components/ingestion/DocumentUploadTab.tsx` | `useEffect` on mount probes `/api/health` for server capability; shows "Server-side pipeline active ⚡" banner when embed is configured; success panel shows `strategy_used` and "server pipeline" badge |

### Behaviour by configuration

| Node config | UI behaviour |
|---|---|
| `VALORI_EMBED_PROVIDER` set | Fast path: text → `/v1/ingest` → done (no client-side embed) |
| Not set | Slow path: existing TypeScript chunk + embed + insert + graph wiring |

The response shape is normalised in both cases so the UI result panel and question suggester work identically regardless of which path ran.

## Findings

- The `/api/health` Next.js proxy already existed and passes through all fields including the new `embed_enabled`/`embed_provider` fields added to the Rust health response — no new proxy route needed.
- The server-side fast path returns `chunk_node_id: -1` for all chunks because the node creates graph nodes internally and does not expose per-chunk node IDs in the `IngestResponse`. The UI success panel shows `rec #{id}` but not chunk node IDs in server mode. This is cosmetic only — graph is fully wired server-side.
- The `pipeline: "server"` tag in the response lets the UI distinguish between the two paths for badge display without adding a query parameter.

## Validation

- Cargo tests: **237 passed, 0 failed** (`cargo test -p valori-kernel -p valori-node`)
- TypeScript: `npx tsc --noEmit` — no errors
- Manual flow verified: banner appears when `VALORI_EMBED_PROVIDER=ollama` is set, disappears when unset, fallback pipeline triggers correctly.

## Follow-ups

- The `probeServerIngest()` call adds one extra `/health` round-trip on mount — could be eliminated by reading server capability once at app startup and storing in context. Deferred.
- The slow-path client-side pipeline (TypeScript) and server-side pipeline (Rust) now duplicate logic (entity extraction, context enrichment, dedup). Long-term all of this moves to the node. Deferred to Phase I4+.
