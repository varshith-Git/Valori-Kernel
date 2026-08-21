# valori-node

HTTP API server and orchestration layer for Valori. Runs in standalone mode
or as a member of a Raft cluster (`VALORI_CLUSTER_MEMBERS`).

## Base URL

- **Local**: `http://localhost:3000`
- **Production**: `https://<your-app>.koyeb.app`

The published contract for every route below is
[`api/openapi/valori-v1.yaml`](../../api/openapi/valori-v1.yaml). Where the
contract and this crate disagree, the contract wins and the code is the bug —
current status in [`docs/api/contract-conformance.md`](../../docs/api/contract-conformance.md).

> **Contract status (Phase API-3.1).** `api/openapi/valori-v1.yaml` is the
> generator's byte-exact output. All **74** public routes carry a real
> `#[utoipa::path]` and are registered on `ValoriApi`; the document declares
> **OpenAPI 3.1.0** (see
> [`docs/api/openapi-version-decision.md`](../../docs/api/openapi-version-decision.md)).
> `scripts/verify-api-route-contract.py` proves Rust == utoipa == OpenAPI on
> every gate run and fails on drift in either direction, and
> `tests/openapi_generated.rs` asserts the committed file is byte-identical to
> what the generator emits. Never hand-edit the YAML — regenerate it.
>
> The other 26 registered routes (7 admin, 5 operator-internal, 14 deprecated
> aliases) are **served but deliberately absent** from the public contract. A
> server route is not the same thing as a public SDK route. See
> [`docs/phases/phase-api-3.1-utoipa-coverage.md`](../../docs/phases/phase-api-3.1-utoipa-coverage.md).

> **Operation completeness (Phase API-3.3).** Route coverage answers *"are the
> right operations present?"*. `scripts/audit-public-api-operations.py` answers
> the next question — *"are their HTTP contracts complete?"* — by cross-checking
> each operation against the **Rust handler signature** it dispatches to.
> Completeness is never inferred from path existence: a `requestBody` counts
> only when the contract declares one **and** the handler has a body extractor,
> and a `query?: never` only when the handler has no `Query<..>`. Both
> directions fail the gate. Current state: **74 / 74 complete**, 0 untyped
> parameters, 0 parameter mismatches, 0 untyped schema properties, and 0
> unexpected `unknown`/`any` in the generated TypeScript. `sdk_ready` is
> `true` — see
> [`docs/api/sdk-readiness.md`](../../docs/api/sdk-readiness.md) and
> [`docs/api/typescript-contract-quality.md`](../../docs/api/typescript-contract-quality.md).

> **Open-ended JSON objects — use `serde_json::Value`, not `Object`.**
> Phase API-4D. A field whose Rust type is `serde_json::Value` and which is
> documented as "arbitrary caller-supplied JSON" must be annotated
> `schema(value_type = std::collections::HashMap<String, serde_json::Value>)`.
> Five fields used `HashMap<String, Object>`, which utoipa renders as
> `additionalProperties: {type: object}` — that means *every value must itself
> be an object*, so `{"page": 4}` is not a valid instance. The Python generator
> honoured it faithfully and emitted a per-value wrapper model, making scalar
> metadata values unrepresentable in the SDK. `serde_json::Value` renders
> `additionalProperties: {}` ("any JSON value"), which is what the field
> descriptions already claimed.
>
> **Known server bug:** `metadata_filter` on `/v1/search`, `/v1/search/multi`
> and `/v1/memory/search*` resolves metadata from the sidecar store only
> (`rec:{id}`), so it never matches metadata written by `POST /v1/records` or
> `PATCH /v1/records/{id}/metadata` and returns zero hits for a predicate that
> exactly matches. Reproduction, root cause and suggested fix in
> [`docs/api/known-server-issues.md`](../../docs/api/known-server-issues.md) #1.
> The SDKs deliberately do not work around it.

## OpenAPI generation

The **only** sanctioned pipeline. Reconstructing `paths` from a route manifest
or from the previous YAML is forbidden — a prior phase did exactly that and
produced a contract in which every operation shared two placeholder responses.

```bash
# Emit the code-first document (stdout, or atomically to a file)
cargo run -p valori-node --features utoipa --bin valori-openapi -- \
    --output api/openapi/valori-v1.yaml

# What Rust actually registers (never reads OpenAPI; fails loudly if unsure)
python3 scripts/generate-route-manifest.py

# Rust public routes == utoipa operations == OpenAPI operations
python3 scripts/verify-api-route-contract.py

# Everything, plus computed docs/api/sdk-readiness.json
./scripts/api-contract-gate.sh
```

Adding an endpoint means annotating its **registered** handler with
`#[cfg_attr(feature = "utoipa", utoipa::path(...))]` and listing it in the
`paths(...)` block of `src/openapi.rs`. Both links are required — an annotation
that is not registered generates nothing, and the route manifest reports
`utoipa_annotated` and `utoipa_registered_in_api` separately so a half-wired
handler is visible rather than counted as covered.

Public request/response types must be deliberate DTOs. Where the wire model
already lives in another workspace crate (tree-RAG, community, ingest, index
lifecycle, object store, model health), that crate carries an optional,
default-off `utoipa` feature which `valori-node/utoipa` turns on — so the
contract references the same Rust type the handler serialises instead of a
hand-copied mirror that can drift. `valori-kernel` is deliberately excluded: it
gains no dependency and stays `no_std`.

Vendor extensions (`x-required-scope`, `x-sdk`) are stamped by the
`VendorExtensionAddon` `Modify` pass in `src/openapi.rs`. `x-required-scope` is
read from `api_keys::required_scope` — the same function the auth middleware
calls — so the contract cannot document a scope the server does not enforce.
The pass adds metadata only; it can never create a path, body, or response.

Phase API-3.3: `x-required-scope` is emitted **only for authenticated
operations**. `GET /health` declares `security: []`, so the auth middleware
never runs on it and `required_scope` is never consulted — the pass used to
stamp it with the function's default anyway, telling every SDK that the one
deliberately open endpoint required a key.

Two further `Modify` passes own responses the handlers do not produce:

| Pass | Owns |
|---|---|
| `AuthResponsesAddon` | `401` / `403` on every authenticated operation, with `body = ApiError`. They come from `auth_guard_v2`, not from any handler. |
| `ErrorBodyAddon` | Fills `ApiError` into any `>= 400` response left bodyless, never overriding one already declared. |

`ErrorBodyAddon` is the **contract-side mirror of `error_codes::attach_error_code`**
— the middleware described below. Because that middleware guarantees the
`ApiError` shape structurally at runtime, describing it structurally here is the
only way the two cannot drift: a new endpoint that forgets `body = ApiError`
gets the correct body anyway, exactly as it gets the correct `code` without
asking.

---

## Error responses (Phase API-2)

**Every** error, on every route, on both the standalone and cluster routers:

```json
{ "error": "unknown collection 'ghost' — create it first with POST /v1/namespaces",
  "code": "collection_not_found" }
```

`code` is the stable machine-readable field — **branch on it, never on
`error`**. It is drawn from a closed set of 16 values: `validation_error`,
`unauthorized`, `forbidden`, `not_found`, `collection_not_found`,
`record_not_found`, `dimension_mismatch`, `invalid_metric`, `invalid_index`,
`index_build_failed`, `conflict`, `capacity_exceeded`, `not_leader`,
`unavailable`, `not_implemented`, `internal_error`
(`valori_engine::ErrorCode`).

`src/error_codes.rs` installs `attach_error_code` as a response-layer
middleware on both routers, so even error bodies produced by axum itself
(previously a bare bodiless `401`/`403`) come back as parseable JSON. Build
errors with `errors::error_response(status, code, message)`; use
`error_codes::collection_not_found(name)` for the one canonical
"Collection does not exist" answer rather than hand-rolling a message.

It is the **outermost** layer on both routers, which is what makes the
guarantee total: a handler that builds a bare `json!({"error": ...})` still
leaves with a `code`. The one thing it deliberately does **not** rewrite is a
non-empty **non-JSON** body — Phase API-3.3 converged
`GET /v1/crypto/status/{key_id}`, which returned `text/plain` and was the last
error in the public surface escaping the canonical shape.

### Status reports are not errors (Phase API-3.3)

`STATUS_REPORT_PATHS` in `src/error_codes.rs` exempts `GET /health` and
`GET /v1/cluster/health`. Both answer `503` with their **full typed health
document** when a pool is at 100 %: the status code is a signal to the load
balancer, not a failure to describe. Without the exemption the middleware saw
"503, JSON object, no `code`" and spliced `error` and `code` into a documented
DTO, so the bytes on the wire did not match the schema the contract advertises
and a strictly-deserialising SDK would reject its own health probe. These are
the only two `>= 400` responses in the surface with a typed non-`ApiError`
body; `scripts/audit-public-api-operations.py` reports any third one rather
than letting it pass.

## Idempotency (Phase API-2)

`POST /v1/records` and `POST /v1/vectors/batch-insert` accept `request_id` on
**both** paths — standalone dedup is real, not a no-op. Two wire spellings are
accepted and normalised by `api::RequestId`: a 16-byte array, or a 32-char hex
string with optional UUID dashes. Replaying a token inside the dedup window
returns the record the first request created and writes nothing. A malformed
token is a 4xx, never a silently dropped field.

## Key scopes

`api_keys::required_scope()` derives the minimum scope from `(method, path)`.
Two special cases exist because prefix derivation got them wrong:
`/v1/cluster/add-node`, `/remove-node` and `/snapshot` require **`admin`**
(they reconfigure the deployment), and `/v1/search/multi` and `/v1/graphrag`
require **`read_only`** (they are pure reads that carry their query in a
body). `tests/api_contract.rs` pins both against the contract's
`x-required-scope`.

---

## Core & System

| Endpoint | Method | Description |
|---|---|---|
| `/health` | `GET` | Liveness probe. |
| `/version` | `GET` | Server version string. |
| `/metrics` | `GET` | Prometheus metrics. |

```bash
curl http://localhost:3000/health
curl http://localhost:3000/version
```

### `/metrics` — what's exported

| Metric | Type | Notes |
|---|---|---|
| `valori_records_live` / `_capacity` / `valori_record_fill_ratio` | gauge | Vector count vs. the `VALORI_MAX_RECORDS` slab. |
| `valori_nodes_live` / `valori_edges_live` (+ `_capacity`, `_fill_ratio`) | gauge | Graph pools. |
| `valori_dim` | gauge | Vector dimension. |
| `valori_collections_total` | gauge | Namespaces, including the implicit `default`. |
| `valori_event_log_height` | gauge | Committed audit-chain height. |
| `valori_process_memory_rss_bytes` / `_virtual_bytes` | gauge | This process's memory. Sampled every 10s by a background task (`process_metrics.rs`). |
| `valori_process_cpu_percent` | gauge | Percent of ONE core — >100 under multi-threaded load is expected. Needs two samples to be non-zero, which is why it's a task, not a handler. |
| `valori_index_size_bytes` | gauge | Serialized index size. Updated at snapshot time only (measuring it means serializing the index). **`0` is correct for `VALORI_INDEX=brute`** — brute force keeps no structure of its own. |
| `valori_snapshot_size_bytes` / `valori_snapshot_duration_seconds` | gauge / histogram | Per `POST /v1/storage/snapshots/upload`. Duration covers encode + upload + prune. |
| `valori_restore_size_bytes` / `valori_restore_duration_seconds` | gauge / histogram | Per `POST /v1/storage/snapshots/restore`. Duration covers download + apply + rehash — the real project-unavailable window, i.e. what an RTO claim should be based on. |
| `valori_graph_node_create_total` | counter | Incremented on each successful `POST /v1/graph/node`. Fires on both standalone and cluster paths (via shared handler). |
| `valori_graph_edge_create_total` | counter | Incremented on each successful `POST /v1/graph/edge`. Fires on both paths. |
| `valori_graph_query_total` | counter | Incremented when `GET /v1/graph/query` resolves the start node and returns hits. |
| `valori_graph_traversal_nodes` | histogram | Node count returned by `GET /v1/graph/query` and `GET /v1/graph/subgraph`. |
| `valori_graph_traversal_edges` | histogram | Edge count returned by `GET /v1/graph/subgraph`. |
| `valori_graphrag_total` | counter | Incremented after each successful `POST /v1/graphrag` (standalone and cluster independently). |
| `valori_graph_rerank_total` | counter | Incremented at the start of each graph-aware reranking pass (`graph_rerank` field in `POST /search`). |
| `valori_graphrag_seed_count` | histogram | Number of vector hits that resolved to a graph node (seed count) per `POST /v1/graphrag` call. A value of 0 while hits exist means no records in the top-k have a graph node. |
| `valori_graphrag_expanded_nodes` | histogram | Number of nodes in the BFS-expanded subgraph per `POST /v1/graphrag` call. |
| `valori_graphrag_expanded_edges` | histogram | Number of edges in the BFS-expanded subgraph per `POST /v1/graphrag` call. |
| `valori_graphrag_no_graph_seed` | counter | Fires when a `POST /v1/graphrag` call returns vector hits but none of them map to a graph node — the collection has vectors but no linked graph. Useful for detecting unlinked collections. |

---

## Collections (Multi-tenancy)

Valori supports up to **1 024 named collections** (namespaces). A brand-new
project starts with **zero collections** — there is no automatically
created "default" and no implicit vector namespace (Phase 3.3). Every data
endpoint's `"collection"` field must name a collection that was already
explicitly created; there is nothing to fall back to if it's omitted.

Records in different collections are **fully isolated** — they never appear
in each other's searches.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/namespaces` | `POST` | Create a collection (idempotent). |
| `/v1/namespaces` | `GET` | List all collections and their numeric IDs. |
| `/v1/namespaces/:name` | `DELETE` | Drop a collection and all its records. |
| `/v1/namespaces/:name/index` | `POST` | Create, replace, or drop (`type: null`) the collection's ANN index. Returns `202 Accepted` — build runs in background. Both standalone and cluster (Phase 4.3). Cluster: desired spec + generation replicated via Raft; each node builds its local index independently. |
| `/v1/namespaces/:name/index` | `GET` | Poll the collection's index lifecycle state (`status`: none/building/active/failed; `active_type`: hnsw/ivf/bq/none; `building_generation`; `base_lsn`; `error`). Both standalone and cluster. Cluster: reports node-local state on the responding node. |

### Create a collection

`dimension` and `metric` are **required** for every collection, no
exceptions — `"default"` included, if you choose to create one by that
literal name. There is no zero-config name; a collection can no longer be
created bare and silently lock onto whatever dimension its first insert
happens to use. `index` remains optional.

```bash
curl -X POST http://localhost:3000/v1/namespaces \
  -H "Content-Type: application/json" \
  -d '{"name": "images", "dimension": 768, "metric": "squared_l2", "index": "ivf"}'
# → {"name":"images","id":0,"created":true}
# Second call with the same name → {"created":false,"id":0} (idempotent)

curl -X POST http://localhost:3000/v1/namespaces \
  -H "Content-Type: application/json" \
  -d '{"name": "images"}'
# → 400: collection 'images' must be created with an explicit 'dimension'

curl -X POST http://localhost:3000/v1/namespaces \
  -H "Content-Type: application/json" \
  -d '{"name": "default"}'
# → 400: same as any other name — "default" carries no special meaning
```

- `metric` currently only accepts `"squared_l2"` — Valori's determinism
  guarantee depends on avoiding a square root, so this is not yet a
  configurable choice, only a representable one.
- `index` defaults to `brute` (no dedicated ANN index — exact
  namespace-scoped search) when omitted.
- Once set, a collection's dimension is immutable — a later request with a
  different `dimension` for the same collection is rejected (`409 Conflict`),
  not silently applied.
- **`"default"`** has no architectural meaning any more. A collection
  literally named `"default"` is created exactly like any other name, with
  the same required `dimension`/`metric` — see the example above.
- **Cluster mode**: dimension is enforced identically and replicated to
  every node (kernel-level, via `KernelEvent::ConfigureNamespace`). As of
  Phase 4.3, per-collection ANN indexes (HNSW, IVF, BQ) are fully supported
  in cluster mode via `POST /v1/namespaces/{name}/index`. The desired spec and
  generation are committed through Raft; each node builds its own local index
  and activates it independently. Search uses the node-local active ANN index
  and falls back to exact brute-force while a build is in progress or if it
  fails on that node. Phase 4.4 added stale-build detection (re-reads desired
  generation before activating a completed build), FAILED retry debounce (60 s
  minimum between retries), watcher drop-path hardening, and 7 Prometheus metrics
  for the full ANN build lifecycle.

### List collections

```bash
curl http://localhost:3000/v1/namespaces
# A brand-new project → {"collections":[]}
# After creating "tenant-acme" and "images":
# → {"collections":[{"name":"tenant-acme","id":0,"dimension":384,"metric":"squared_l2"},
#                    {"name":"images","id":1,"dimension":768,"metric":"squared_l2","index":"ivf"}]}
```

### Drop a collection

```bash
curl -X DELETE http://localhost:3000/v1/namespaces/tenant-acme
# → 204 No Content
# Dropping an unknown collection → 404 Not Found
# ("default" is not a special case — 404 if it was never created, 204 if it was)
```

Collection names must be non-empty, at most 64 characters, and contain only
`[a-zA-Z0-9_-]`. These rules — and the status codes above — are identical on
the standalone and cluster paths: since Phase R1 the handler bodies are
shared (`src/routes/collections.rs`), and `tests/route_parity.rs` asserts the
two routers expose the same `/v1` surface.

### Cluster mode (Phase S2)

Collection create/drop go through Raft, exactly like every other write — the
name → id mapping is replicated and identical on every node, durable across
snapshots and leader failover. A follower correctly answers `307 Temporary
Redirect` to these two endpoints (same as `/records`), pointing at the
leader. `GET /v1/namespaces` is a local, eventually-consistent read (matches
every other list-style cluster endpoint) — a node still catching up on
replication may briefly lag behind the leader's list.

---

## Built-in Ingest Pipeline (Phase I1/I2/I3/I8)

Three endpoints that handle chunking, embedding, and document lifecycle entirely on the node.

### Chunking only — `POST /v1/ingest/document`

Splits raw text into chunks using server-side intelligence. No vectors are inserted.

```bash
curl -X POST http://localhost:3000/v1/ingest/document \
  -H "Content-Type: application/json" \
  -d '{
    "text": "1 Introduction\nThis paper explores...\n2 Methods\n...",
    "strategy": "auto",
    "collection": "default",
    "source": "paper.pdf",
    "chunk_size": 1000,
    "chunk_overlap": 200
  }'
# → {"strategy_used":"tree","chunk_count":12,
#    "chunks":[{"index":0,"title":"1 Introduction","text":"..."},...]}
```

Strategies: `auto` (sniffs text), `tree` (section headers), `conversation` (Q&A boundaries), `sentence` (±2 sentence window), `fixed` (overlapping windows).

### Full pipeline — `POST /v1/ingest`

Requires `VALORI_EMBED_PROVIDER` (ollama / openai / custom). Chunks + embeds + inserts + creates graph nodes + stores metadata sidecar. One call replaces the entire client-side pipeline.

```bash
# Start node with embedding configured:
VALORI_EMBED_PROVIDER=ollama VALORI_EMBED_MODEL=nomic-embed-text \
  cargo run -p valori-node

curl -X POST http://localhost:3000/v1/ingest \
  -H "Content-Type: application/json" \
  -d '{"text":"...","source":"annual_report.pdf","strategy":"tree","collection":"finance"}'
# → {"ok":true,"document_node_id":1,"strategy_used":"tree",
#    "chunk_count":31,"record_ids":[1,2,...31],"collection":"finance"}
# → 422 if VALORI_EMBED_PROVIDER not set
```

### Env vars for on-node embedding

| Var | Default | Purpose |
|---|---|---|
| `VALORI_EMBED_PROVIDER` | — | `ollama` / `openai` / `custom`; absent = embedding disabled |
| `VALORI_EMBED_MODEL` | provider default | e.g. `nomic-embed-text`, `text-embedding-3-small` |
| `VALORI_EMBED_URL` | provider default | Base URL; Ollama: `http://localhost:11434`, OpenAI: `https://api.openai.com` |
| `VALORI_EMBED_API_KEY` | — | Required for OpenAI / custom if auth needed |

### Health probe

`/health` now includes `"embed_enabled": true` and `"embed_provider": "ollama"` when embedding is configured. The UI uses this to auto-detect which pipeline to use.

### Document update — `POST /v1/ingest/update` (Phase I8)

Updates a previously ingested document without re-embedding unchanged chunks.
Uses BLAKE3 content hashing to diff old vs new chunks at the text level.

```bash
curl -X POST http://localhost:3000/v1/ingest/update \
  -H "Content-Type: application/json" \
  -d '{
    "document_node_id": 42,
    "text": "1 Introduction\nUpdated content...\n2 Methods\n...",
    "source": "paper-v2.pdf",
    "collection": "default"
  }'
# → {"ok":true,"document_node_id":42,"strategy_used":"tree",
#    "new_chunk_count":35,"kept_count":28,"removed_count":3,
#    "added_count":7,"record_ids":[1,2,...35],"collection":"default"}
```

**Diff algorithm:**
- Unchanged chunks (same BLAKE3 hash) → kept as-is, not re-embedded
- Removed chunks (old hash not in new set) → soft-deleted + graph node removed
- New/changed chunks → embedded, inserted, new Chunk node + ParentOf edge

The Document graph node is reused — external edges pointing to it remain valid.

### Cluster mode (Phase I4)

`POST /v1/ingest` and `POST /v1/ingest/update` work identically in standalone and 3/5-node cluster mode. In cluster mode every vector insert and graph mutation goes through `raft.client_write()` and is replicated to all peers — same BLAKE3 state hash on every node after ingest. As of Phase I4.1 the metadata sidecar (chunk text, source, …) is **also** replicated, via `KernelEvent::SetMeta`, so any node can serve `/v1/memory/meta/get`.

### Async ingest — fire-and-forget large documents

Pass `"async": true` in the request body to `POST /v1/ingest`. The server returns immediately with a `job_id`; poll `GET /v1/ingest/status/:job_id` for progress.

```bash
# Start an async ingest job
JOB=$(curl -s -X POST http://localhost:3000/v1/ingest \
  -H "Content-Type: application/json" \
  -d '{"text": "...", "source": "paper.pdf", "async": true}' | jq -r .job_id)

# Poll for completion
curl http://localhost:3000/v1/ingest/status/$JOB
# → {"job_id": "...", "status": "completed", "chunk_count": 31, "record_ids": [...]}
```

### Graph node management

```bash
# GET a node by ID (returns node data + adjacency list)
curl "http://localhost:3000/v1/graph/node/42?collection=default"

# DELETE a node (soft-deletes associated record; edges removed)
curl -X DELETE "http://localhost:3000/v1/graph/node/42?collection=default"
```

Both routes are available on standalone (`/v1/graph/node/:id`) and via the legacy path (`/graph/node/:id`). On clusters the DELETE goes through `raft.client_write()`.

---

## Tree-RAG — hierarchical retrieval with provable receipts (Phase I5)

PageIndex-style retrieval that navigates a document's table-of-contents to the
*right section* instead of returning vector-similar text — plus a BLAKE3
receipt that makes every retrieval replayable and tamper-evident. Deterministic:
no embeddings, no LLM. All three handlers are stateless, so they behave
identically in standalone and cluster mode.

| Endpoint | Method | Body | Returns |
|---|---|---|---|
| `/v1/tree/build` | `POST` | `{text, doc_name?}` | `{cache_key, doc_name, node_count, structure_map, tree}` |
| `/v1/tree/query` | `POST` | `{tree?, cache_key?, query, k?, prev_hash?}` | `{answer, citations, visited_node_ids, reasoning, receipt}` |
| `/v1/tree/hybrid` | `POST` | `{text?, tree?, cache_key?, query, namespace?, k?, tree_weight?, prev_hash?, doc_name?}` | `{query, hits, tree_hit_count, vector_hit_count, tree_answer?, reasoning}` |
| `/v1/tree/verify` | `POST` | `{tree, receipt}` | `{valid}` |
| `/v1/community/detect` | `POST` | `{namespace?, max_iter?}` | `{community_count, node_count, communities, receipt}` |
| `/v1/community/search` | `POST` | `{vector, k?, namespace?, depth?, drill_in?}` | `{communities, total_communities_searched}` |
| `/v1/ingest/extract-entities` | `POST` | `{text, namespace?, entity_types?, model?}` | `{entities, relationships, entity_count, relationship_count, skipped_relationships}` |

`/v1/tree/build` stores the parsed tree in a server-side cache and returns `cache_key` (BLAKE3 of the input text). Pass `cache_key` to subsequent `/v1/tree/query` or `/v1/tree/hybrid` calls instead of re-transmitting the full `tree` object. The full `tree` is still accepted for backward compatibility.

`/v1/tree/hybrid` fuses tree-navigation scores with vector similarity scores (requires `VALORI_EMBED_PROVIDER`). `tree_weight` (default 0.6) controls the blend. Results include a `source` tag (`"tree"` or `"vector"`) per hit.

Each `query` (or `hybrid`) returns a `receipt`; pass its `receipt_hash` as the next call's `prev_hash` to chain receipts. `verify` returns `valid: false` if the stored section was altered.

```bash
# Build once, cache server-side
RESP=$(curl -s localhost:3000/v1/tree/build \
  -H 'Content-Type: application/json' \
  -d '{"text":"# Handbook\n## Annual Leave\n25 days.\n## Sick Leave\n10 days.\n","doc_name":"hb"}')
CACHE_KEY=$(echo "$RESP" | jq -r '.cache_key')

# Query by cache_key (no re-transmit)
curl -s localhost:3000/v1/tree/query \
  -H 'Content-Type: application/json' \
  -d "{\"cache_key\":\"$CACHE_KEY\",\"query\":\"how many sick days?\"}"
# → answer cites "Handbook > Sick Leave", lines [N, M], with a receipt

# Hybrid: tree + vector (needs embed provider)
curl -s localhost:3000/v1/tree/hybrid \
  -H 'Content-Type: application/json' \
  -d "{\"cache_key\":\"$CACHE_KEY\",\"query\":\"sick leave policy\",\"k\":5,\"tree_weight\":0.6}"
```

---

## Vector Operations

All endpoints accept an optional `"collection"` field. If the named collection
does not exist, delete and graph endpoints answer `404 Not Found` (Phase R2,
both paths); insert and search answer `400 Bad Request`.

| Endpoint | Method | Description |
|---|---|---|
| `/records` | `POST` | Insert a single vector. Optional `text` field indexes the record for hybrid retrieval (Phase C5). |
| `/v1/vectors/batch_insert` | `POST` | Insert multiple vectors. Optional `texts` array indexes each record for hybrid retrieval (Phase C5). |
| `/search` | `POST` | K-nearest-neighbour search. `rerank=true` (default) + `query_text` enables the Valori Reranker (Phase C5). Supports `as_of` / `as_of_log_index` for point-in-time reads, `decay_half_life_secs` for recency-aware ranking (Phase C4.1), `metadata_filter` for JSON predicate post-filtering (Phase I7), and `graph_rerank` for graph-aware reranking (Phase G1.4.1, not supported on `as_of` queries). Namespace-scoped on both paths — cluster mode was fixed in Phase G1.4.2 (previously leaked across namespaces sharing a shard, e.g. any `VALORI_SHARD_COUNT=1` deployment). |
| `/v1/search/multi` | `POST` | **Phase 5 — Cross-collection search.** Body: `{query, k, collections: [name, ...], decay_half_life_secs?, metadata_filter?}`. All collections must share the same `dim` and `metric`; different index types are allowed. Fans-out to each collection in parallel, merges by Squared L2 (smaller = better). Each hit carries a `collection` field. Partial per-collection failures are reported in `partial_failures` without suppressing other results. BM25 reranking and graph reranking are excluded (scores from different Collection corpora are not comparable). Available on both standalone and cluster paths. |
| `/v1/delete` | `POST` | Permanently remove a record by ID (accepts an optional `"collection"` field, S7). **Cascades**: also deletes every graph node still referencing the record (and each such node's incident edges), in ascending `NodeId` order (G1.3.1) — a record can have zero, one, or many nodes (`/v1/memory/contradict`/`consolidate` create one per call), and a surviving node whose `record` points at a hard-deleted record makes the state's own snapshot undecodable, so the cascade is mandatory, not optional. Rejects (404) a `collection` that doesn't own the record. |
| `/v1/soft-delete` | `POST` | Mark a record inactive without removing it — searchable-off but still present for audit (accepts an optional `"collection"` field, S7). Does **not** touch the graph: the record row survives (flagged, not freed), so any referencing node stays valid. Rejects (404) a `collection` that doesn't own the record. |
| `/v1/timeline` | `GET` | Structured event timeline. Accepts `from=<ISO8601>` and `to=<ISO8601>` filters. |

### Insert into a collection

```bash
curl -X POST http://localhost:3000/records \
  -H "Content-Type: application/json" \
  -d '{"values": [0.1, 0.2, 0.3, 0.4], "collection": "tenant-acme"}'
# → {"id": 0}
```

### Batch insert

```bash
curl -X POST http://localhost:3000/v1/vectors/batch_insert \
  -H "Content-Type: application/json" \
  -d '{"batch": [[0.1,0.2,0.3,0.4],[0.5,0.6,0.7,0.8]], "collection": "tenant-acme"}'
# → {"ids": [0, 1]}
```

### Batch insert with per-item idempotency (Phase 3.12)

Supply a 32-hex string per slot in `request_ids`. Duplicate keys are detected
server-side and the previously assigned record ID is returned — safe for
at-least-once delivery.

```bash
curl -X POST http://localhost:3000/v1/vectors/batch_insert \
  -H "Content-Type: application/json" \
  -d '{
    "batch": [[0.1,0.2,0.3,0.4],[0.5,0.6,0.7,0.8]],
    "request_ids": ["aabbccddeeff00112233445566778899", null]
  }'
# → {"ids": [0, 1]}
# Retrying with the same request_id returns the same IDs without double-insert.
```

A `null` entry in `request_ids` opts that slot out of dedup. Omitting
`request_ids` entirely is fully backward-compatible.

### Search within a collection

```bash
# Scoped to tenant-acme — default-namespace records are excluded.
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5, "collection": "tenant-acme"}'
# → {"results":[{"id":0,"score":0.0}]}

# "collection" is required — there is no implicit collection to search
# even when it's omitted (Phase 3.3).
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5, "collection": "tenant-acme"}'
```

### Point-in-time (as-of) search — Phase 3.4

Requires `VALORI_EVENT_LOG_PATH` to be set. Replays the event log up to the
target point and searches the resulting state.

```bash
# Search as the state was after the 5th committed event (log_index 4).
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5, "as_of_log_index": 4}'
# → {
#     "results": [...],
#     "as_of_log_index": 4,
#     "as_of_timestamp_iso": "2026-03-03T10:00:00Z",
#     "as_of_state_hash": "a3f...bc9"   ← BLAKE3 of the replayed state
#   }

# Search the state as it existed on March 3, 2026 (UTC).
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5, "as_of": "2026-03-03T00:00:00Z"}'
```

```bash
# Python SDK
from valoricore.remote import SyncRemoteClient
c = SyncRemoteClient("http://localhost:3000")
resp = c.search([0.1, 0.2, 0.3, 0.4], k=5, as_of="2026-03-03T00:00:00Z")
print(resp["results"], resp["as_of_state_hash"])
```

### Event timeline

```bash
# All events (structured JSON).
curl http://localhost:3000/v1/timeline

# Events in a specific time window.
curl "http://localhost:3000/v1/timeline?from=2026-03-01T00:00:00Z&to=2026-03-31T23:59:59Z"
```

---

## Memory Protocol (Recommended for AI agents)

High-level endpoints that combine vector storage with graph metadata.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/memory/upsert_vector` | `POST` | Insert vector + metadata + graph nodes. |
| `/v1/memory/search_vector` | `POST` | Search for similar vectors. |
| `/v1/memory/consolidate` | `POST` | Replace a memory: soft-delete old + insert new + `Supersedes` edge (Phase C4.2). |
| `/v1/memory/contradict` | `POST` | If two records' cosine similarity ≥ threshold, commit a `Contradicts` edge (Phase C4.3). |
| `/v1/memory/meta/get` | `GET` | Retrieve metadata by ID. |
| `/v1/memory/meta/set` | `POST` | Update metadata for an existing ID. |

```bash
curl -X POST http://localhost:3000/v1/memory/upsert_vector \
  -H "Content-Type: application/json" \
  -d '{"vector": [0.1, 0.2, 0.3, 0.4], "metadata": {"role": "assistant-memory"}}'

curl -X POST http://localhost:3000/v1/memory/search_vector \
  -H "Content-Type: application/json" \
  -d '{"query_vector": [0.1, 0.2, 0.3, 0.4], "k": 3}'

# Consolidate: replace record 7 with a new vector (commits 3 events to the chain)
curl -X POST http://localhost:3000/v1/memory/consolidate \
  -H "Content-Type: application/json" \
  -d '{"old_record_id": 7, "new_vector": [0.2, 0.3, 0.4, 0.5]}'

# Contradiction: link two records if cosine similarity ≥ threshold (default 0.85)
curl -X POST http://localhost:3000/v1/memory/contradict \
  -H "Content-Type: application/json" \
  -d '{"record_a": 3, "record_b": 9, "threshold": 0.9}'
```

### GraphRAG — `POST /v1/graphrag` (Phase 3.15, hardened Phase 5.3/5.4)

Retrieve the K nearest vectors **and** the connected knowledge subgraph around
them in a single call, from one consistent kernel snapshot — no second store, no
cross-system drift. Vectors and graph live in the same kernel, so the KNN, the
record→node resolution, and the subgraph BFS all run under one read lock.

**Request** (Phase 5.4 contract):

```bash
curl -X POST http://localhost:3000/v1/graphrag \
  -H "Content-Type: application/json" \
  -d '{
    "query_vector": [0.1, 0.2, 0.3, 0.4],
    "retrieval_k": 20,
    "final_k": 10,
    "depth": 2,
    "max_graph_candidates": 100,
    "max_nodes": 500,
    "max_edges": 2000,
    "graph_weight": 0.3,
    "collection": "knowledge"
  }'
```

`k` is accepted as a backward-compat alias for `retrieval_k`.

| Field | Default | Meaning |
|---|---|---|
| `retrieval_k` / `k` | 5 | Vector seed count (ANN candidates used as graph expansion seeds) |
| `final_k` | `retrieval_k` | Maximum hits returned after reranking. Absent = same as retrieval_k. |
| `depth` | 2 | BFS hop depth (clamped to MAX_DEPTH=4) |
| `max_graph_candidates` | 100 | Budget on graph-only hits before `final_k` |
| `max_nodes` | unlimited | Halt BFS once this many nodes are visited |
| `max_edges` | unlimited | Halt edge emission once this many edges are emitted |
| `graph_weight` | 0.3 | β in `final_score = (1-β)×vector_rel + β×graph_rel`; range [0,1] |

**Response hit shape** (Phase 5.4):

```jsonc
// Vector hit with graph node (seed):
{ "record_id": 15, "source": "vector_and_graph",
  "score": 0.05, "vector_score": 0.05,
  "graph_score": 1.0, "final_score": 0.966,
  "graph_distance": 0, "node_id": 3, "memory_id": "rec:15", "metadata": null }

// Vector hit without graph node:
{ "record_id": 42, "source": "vector",
  "score": 0.12, "vector_score": 0.12,
  "graph_score": 0.0, "final_score": 0.614,
  "graph_distance": null, "node_id": null, "memory_id": "rec:42", "metadata": null }

// Graph-only hit (not in top-k vector results, reached via expansion):
{ "record_id": 57, "source": "graph",
  "score": null, "vector_score": null,
  "graph_score": 0.5, "final_score": 0.15,
  "graph_distance": 1, "node_id": 7, "memory_id": "rec:57", "metadata": null }
```

`score` is a backward-compat deprecated alias for `vector_score`. All hits are
merged into one list sorted by `final_score` descending (higher = better),
with `record_id` ascending as tie-breaker. `graph_score` = `1/(1+hop_distance)`,
always in [0, 1]. `graph_distance` is the minimum hop count from any seed
(guaranteed shortest path). At `graph_weight=1.0` the ranking is purely
graph-based — graph-only candidates can outrank pure vector hits with no graph
node. On a cluster the request also honours `consistency` (linearizable by
default). For agents, prefer the `memory_graph_recall` MCP tool.

### Recency-aware search — `decay_half_life_secs` (Phase C4.1)

Add `decay_half_life_secs` to `/search` (or `/v1/memory/search_vector`) to fade
older memories in ranking. A record one half-life old has its L2 distance
doubled, so a fresh near-match can overtake a stale better one.

```bash
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5, "decay_half_life_secs": 86400}'
```

Each hit gains `decay_factor` (∈ (0,1]) and `age_secs`; `score` stays the true,
undecayed distance. Decay is a **read-time re-rank**: it never mutates kernel
state and never changes the BLAKE3 state hash. Set `VALORI_DECAY_HALF_LIFE_SECS`
for a server default (a per-request value, including `0` to disable, wins).
Not applied to `as_of` queries. Standalone only in v1 (cluster accepts the field
but treats it as neutral — see `docs/phases/phase-C4.1-decay.md`).

### Graph-aware reranking — `graph_rerank` (Phase G1.4.1)

Add `graph_rerank` to `/search` to nudge vector ranking by graph proximity to
the query's own best hits — a candidate structurally close to your top match
ranks higher than one that's equally distant in vector space but graph-isolated:

```bash
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5,
       "graph_rerank": {"weight": 0.15, "seed_count": 1, "direction": "outgoing", "max_depth": 2}}'
```

All fields are optional (defaults shown above). Seeds are the resolved graph
nodes of the top `seed_count` hits in the *same* search's own candidate pool —
no separate seed-node lookup needed. Each candidate's graph distance is the
minimum hop count (bounded BFS, `direction`/`max_depth`-scoped) across all
graph nodes referencing its record (a record may have several — see
[docs/reviews/graph-g1.3.1-record-graph-cascade-semantics.md](../../docs/reviews/graph-g1.3.1-record-graph-cascade-semantics.md)).
Reranking formula: `adjusted = score × (1 + weight × distance)` — a
multiplicative penalty, same shape as decay's `distance / factor`. Each hit
gains `graph_distance` (absent when `graph_rerank` isn't requested); missing
or unreachable graph data is **neutral** (no penalty, never drops a
candidate). Composes with either `rerank` (BM25) or `decay_half_life_secs` —
it runs as an independent final pass over whichever score they already
produced. Read-time only: never mutates canonical state, never affects the
BLAKE3 state hash. Not applied to `as_of` queries. Standalone and cluster
both supported — see
[docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md](../../docs/reviews/graph-g1.4.1-graph-aware-reranking-design.md)
for the full design and the reachability-pre-filter / independent-signal-fusion
modes deliberately deferred out of this version.

### Valori Reranker — hybrid retrieval (Phase C5)

The Valori Reranker runs inside the node after vector search. When a record is
inserted with a `text` field, the server tokenises and indexes it. At query
time, passing `rerank=true` (the default) and a `query_text` string triggers a
two-stage retrieval:

1. Kernel returns `k × POOL_FACTOR` candidates by vector similarity.
2. The reranker scores each candidate by term frequency against `query_text`
   and blends the two scores (50 % vector + 50 % term score).
3. The top-k from the blended ranking are returned.

No external process, no LLM call, no network hop — the reranker is pure Rust
inside the same binary.

```bash
# Insert with text for hybrid indexing
curl -X POST http://localhost:3000/records \
  -H "Content-Type: application/json" \
  -d '{"values": [0.1, 0.2, 0.3], "text": "Section 3.1 Training — AdamW optimizer"}'

# Search with hybrid reranking (rerank=true is the default)
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3], "k": 5, "query_text": "what optimizer is used?"}'
```

Python SDK:

```python
c.insert(vector, text="Section 3.1 Training — AdamW optimizer")
c.insert_batch(vectors, texts=["Section 3.1 ...", "Section 4.2 ...", ...])
hits = c.search(query_vec, k=5, query_text="what optimizer is used?")
```

Set `rerank=false` (or omit `query_text`) to fall back to pure vector ranking.
The reranker state is in-memory and rebuilt from inserts — it does not persist
across restarts today (see Phase C6 follow-ups).

---

## Snapshots & Recovery

| Endpoint | Method | Description |
|---|---|---|
| `/v1/snapshot/save` | `POST` | Persist in-memory state to disk. |
| `/v1/snapshot/restore` | `POST` | Restore state from a disk file. |
| `/v1/snapshot/download` | `GET` | Download the snapshot as raw bytes. |
| `/v1/snapshot/upload` | `POST` | Upload a snapshot binary to restore state. |

Snapshots include the full namespace registry — collection names, IDs, and all
records survive a round-trip. The snapshot encoder writes into a growable buffer
(Phase P1), so there is no record-count or dimension ceiling — verified at 1M
records (515 MB snapshot in ~1.2 s).

**WAL durability on teardown (Phase P1).** Inserts are buffered and fsynced in
batches for throughput. `Engine` and `EventCommitter` now flush the tail buffer
on `Drop`, so a clean teardown never loses buffered events. For explicit
durability mid-run without a full snapshot, call `flush()`.

```bash
curl -X POST http://localhost:3000/v1/snapshot/save \
  -H "Content-Type: application/json" \
  -d '{"path": "./backup.snap"}'
```

**Snapshot on shutdown.** In standalone mode the server runs with a graceful-shutdown
handler: on `SIGTERM` or `Ctrl-C` it writes a final snapshot to `VALORI_SNAPSHOT_PATH`
(when set) before exiting, so the next start is instant. The event log already guarantees
durability — this only avoids a full replay. No configuration required.

**Periodic autosave (Phase 6.2).** Set `VALORI_SNAPSHOT_INTERVAL=<secs>` (with
`VALORI_SNAPSHOT_PATH`) to also write the snapshot on a fixed cadence, so an
ungraceful kill (`SIGKILL`, power loss) still leaves a recent snapshot behind.
UI-launched project nodes set 60. Standalone only — cluster durability rides on
the persisted Raft log instead. Cluster mode has its own graceful-shutdown
handler (drains HTTP, lets redb close cleanly); it does not write snapshot files.

---

## Proofs & Audit

| Endpoint | Method | Description |
|---|---|---|
| `/v1/proof/state` | `GET` | BLAKE3 hash of the current engine state (hex). |
| `/v1/proof/event-log` | `GET` | BLAKE3 hash of the immutable event log (hex). |
| `/v1/proof/receipt` | `GET` | Most recently assembled `Receipt` (RFC-0003); `404` if none. |
| `/v1/proof/receipt/:id` | `GET` | Receipt by `receipt_id`; `404` if not found. |

```bash
curl http://localhost:3000/v1/proof/state
# → {"final_state_hash":"a3f2..."}
```

---

## Usage Accounting (Phase P2)

Read-only records/collections/storage-byte counts for Cloud's plan/quota
system. `valori-node` remains completely plan-agnostic — this endpoint
returns raw numbers only, never a plan name, never a quota decision.
Never mutates canonical state (read lock only, no audit-log write) and
never appears in the BLAKE3 state hash.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/usage` | `GET` | `{records, collections, storage: {event_log_bytes, snapshot_bytes, total_bytes}}`. In cluster mode, `records`/`storage` are summed across every shard this node runs; `collections` is shard-0's namespace registry (not duplicated per shard, so no sum needed). |

```bash
curl -H "Authorization: Bearer $VALORI_AUTH_TOKEN" http://localhost:3000/v1/usage
# → {"records":17,"collections":11,"storage":{"event_log_bytes":2466,"snapshot_bytes":9257,"total_bytes":11723}}
```

`storage_bytes` sums the live event-log segment **plus every rotated
archive segment** (`events.log`, `events.log.000001`, ...) — archived
segments are never deleted on rotation, so a naive stat of only the live
file undercounts after any rotation has ever happened.

### Billable storage definition

**What counts**: the live event-log segment, every rotated archive
segment, and the snapshot file (`state.snap`). This is the actual
on-disk footprint of a project's real data — everything a restore needs.

**What does not count**: `metadata.json`/`namespaces.json` (trivial
sidecars, a few hundred bytes), `.tmp` atomic-write files (transient,
present only mid-crash), remote object-store copies (Cloud's own
`infra.project_backups`/`storage_usage` tracks that allocation
separately — including it here would double-count against that system),
and the `~/.valori/metadata.redb` control-plane DB (not project data at
all).

**Why `snapshot_bytes` can fluctuate by a small amount across restarts,
even with identical logical content** (observed directly: 9359 → 9257
bytes across a real restart with the exact same 17 records / 11
collections both before and after): the snapshot format's canonical
section (records, graph, index heads — the part that determines the
BLAKE3 state hash) is followed by three **non-canonical** trailing
sections written on every save — `NSRG` (namespace registry, JSON),
`CRTS` (per-record creation timestamps, used only for read-time decay
ranking), and `BCRP` (BM25 reranker term-frequency corpus) — see
`Engine::write_snapshot_to_writer` in `crates/valori-engine/src/engine.rs`.
These sections are real, intentionally excluded from the state hash
(hashing a wall-clock timestamp would make the hash non-reproducible on
replay, which is the opposite of the point), and their serialized size
can vary slightly run to run without any change to canonical data. **A
few hundred bytes of drift on `snapshot_bytes` between two snapshots of
logically identical data is expected and does not indicate data loss,
corruption, or a billing discrepancy** — verified directly: the same
real restart that produced this drift left `records`/`collections`
(the two fields the trailing sections cannot affect) and the BLAKE3
state hash itself completely unchanged. Anyone treating `storage_bytes`
as an exact, restart-stable number for billing purposes should be aware
of this before relying on byte-for-byte precision — the record/
collection counts are exact and stable; the storage figure has this one
documented, bounded source of natural variance.

---

## API Key Management (Phase 3.5)

Per-tenant scoped credentials. Three scope tiers: `read_only < read_write < admin`.

| Endpoint | Method | Scope required | Description |
|---|---|---|---|
| `/v1/keys` | `POST` | admin | Create a new API key. |
| `/v1/keys` | `GET` | admin | List all keys (masked — no raw token). |
| `/v1/keys/:id` | `DELETE` | admin | Revoke a key immediately. |

```bash
# Create a read-write key (using the legacy admin token or an existing admin key).
curl -X POST http://localhost:3000/v1/keys \
  -H "Authorization: Bearer <admin-token>" \
  -H "Content-Type: application/json" \
  -d '{"scope": "read_write", "description": "tenant-acme write key"}'
# → {"id":"key_a3f2...","token":"vk_...","scope":"read_write","created_at":1719000000}
# Token is shown ONCE — store it now.

# List keys (token masked after creation).
curl http://localhost:3000/v1/keys \
  -H "Authorization: Bearer <admin-token>"

# Revoke a key.
curl -X DELETE http://localhost:3000/v1/keys/key_a3f2... \
  -H "Authorization: Bearer <admin-token>"
```

Env var: `VALORI_KEYS_PATH=./keys.json` — persist across restarts.
`VALORI_AUTH_TOKEN` continues to work as a legacy admin credential.

---

## Crypto-shredding / GDPR Erasure (Phase 3.6)

AES-256-GCM per-record encryption with cryptographic erasure. Destroying a
Data Encryption Key (DEK) makes all data encrypted under it permanently
unrecoverable — GDPR Article 17 compliance without truncating the audit log.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/records/encrypted` | `POST` | Encrypt payload and store as a non-searchable record. Returns `{"id", "key_id"}`. |
| `/v1/crypto/shred/:key_id` | `DELETE` | Destroy the DEK. All records under this key become permanently unrecoverable. |
| `/v1/crypto/status/:key_id` | `GET` | Returns `{"exists": bool}` — false after shredding. |

**Request body for `POST /v1/records/encrypted`:**
```json
{
  "payload": "<base64-encoded plaintext>",
  "tag": 0,
  "collection": "default",
  "key_id": "<optional 32-hex key — omit for auto-generated>"
}
```

**Durability:** Set `VALORI_SHRED_LOG_PATH=./shred.log` to persist shredded key_ids across restarts.

**Grouping:** Multiple records can share one `key_id` and be shredded atomically with a single `DELETE`.

**Multi-shard clusters (Phase S5):** `DELETE /v1/crypto/shred/:key_id` fans out to
every shard this node runs, since ciphertext for one `key_id` can legitimately
land on different shards (one per collection it was used to encrypt into).
The response is `{"key_id", "shredded": bool, "shards": {"shard_0": {"status": "shredded"|"not-leader"|"error", ...}, ...}}`
— `shredded` is `true` only when every shard reports `"shredded"`. A shard
reporting `"not-leader"` means retry the call (it's a leader-redirect
condition, not a failure); the per-node DEK is destroyed unconditionally on
the very first call regardless of per-shard status, so retrying is always
safe — it can only re-confirm already-shredded records, never re-encrypt
them.

---

## Cluster Management

Available when the node boots in cluster mode (`VALORI_CLUSTER_MEMBERS` set).
Write requests are leader-only; a follower answers **403** with the leader's
API address.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/cluster/status` | `GET` | Leader, term, log indices, membership. |
| `/v1/cluster/health` | `GET` | `200` when a leader is visible; `503` otherwise. |
| `/v1/cluster/role` | `GET` | This node's current Raft role (`Leader`/`Follower`/`Candidate`). |
| `/v1/cluster/add-node` | `POST` | Join a node (learner catch-up → voter promotion). |
| `/v1/cluster/remove-node` | `POST` | Remove a voter (last-voter removal refused with `422`). |

```bash
curl http://localhost:3000/v1/cluster/status

curl -X POST http://localhost:3000/v1/cluster/add-node \
  -H "Content-Type: application/json" \
  -d '{"node_id": 2, "raft_addr": "10.0.0.2:3100", "api_addr": "10.0.0.2:3000"}'

curl -X POST http://localhost:3000/v1/cluster/remove-node \
  -H "Content-Type: application/json" \
  -d '{"node_id": 2}'
```

### Cluster environment variables

| Variable | Description |
|---|---|
| `VALORI_CLUSTER_MEMBERS` | `id=raft_addr/api_addr,…` — presence activates cluster mode. |
| `VALORI_NODE_ID` | This node's ID (must appear in `VALORI_CLUSTER_MEMBERS`). |
| `VALORI_RAFT_BIND` | gRPC consensus listener (default `0.0.0.0:3100`). |
| `VALORI_CLUSTER_INIT` | Set to `1` on exactly one node of a brand-new cluster. |
| `VALORI_RAFT_LOG_PATH` | Path to the `redb` file for the persistent Raft log. When set, the state machine shares this database so `last_applied` and snapshots survive restarts without replaying audit events. |
| `VALORI_SNAPSHOT_INTERVAL` | Standalone only. Periodic autosave interval in seconds (`VALORI_SNAPSHOT_PATH` must also be set). Omit = snapshot on graceful shutdown only. |
| `VALORI_STATE_HASH_CHECK_SECS` | Hash-convergence poll interval in seconds (default `30`, `0` = off). |
| `VALORI_SHARD_COUNT` | **Phase S1 — multi-Raft skeleton.** Number of independent Raft groups this process runs, sharing one gRPC listener (default `1`, byte-identical to pre-S1 behavior). Every configured member is a voter in every shard (symmetric placement) — namespace→shard routing and asymmetric placement do not exist yet, so shards beyond 0 currently have no HTTP surface. See [`docs/phases/phase-S1-multi-raft-skeleton.md`](../../docs/phases/phase-S1-multi-raft-skeleton.md). |

---

## Python SDK

### Single-node client

```python
from valoricore.remote import SyncRemoteClient

client = SyncRemoteClient("http://localhost:3000")

# Create a collection
client.create_collection("tenant-acme")

# Insert into a named collection
record_id = client.insert([0.1, 0.2, 0.3, 0.4], collection="tenant-acme")

# Batch insert
ids = client.insert_batch([[0.1, 0.2, 0.3, 0.4], [0.5, 0.6, 0.7, 0.8]],
                          collection="tenant-acme")

# Scoped search
results = client.search([0.1, 0.2, 0.3, 0.4], k=5, collection="tenant-acme")

# Agent-memory primitives — return memory_id + graph nodes + decay fields
m = client.memory_upsert([0.1, 0.2, 0.3, 0.4], metadata={"role": "note"})
hits = client.memory_search([0.1, 0.2, 0.3, 0.4], k=5, decay_half_life_secs=86400)

# Self-maintaining memory (audited — commits edges to the BLAKE3 chain)
client.consolidate(old_record_id=m["record_id"], new_vector=[0.2, 0.3, 0.4, 0.5])
client.contradict(record_a=3, record_b=9, threshold=0.9)

# Proof / provenance receipt
proof = client.event_log_proof()   # {"event_log_hash", "final_state_hash", "committed_height", ...}

# List and drop
collections = client.list_collections()   # [{"name": "tenant-acme", "id": 0}, ...] — [] on a brand-new project
client.drop_collection("tenant-acme")
```

The SDK wraps all 40 product HTTP endpoints. `list_contradictions()` /
`resolve_contradiction()` are **deprecated** (they called the legacy Next.js UI
layer, not the node) — use `contradict()` / `consolidate()` instead.

### Multi-node cluster client

```python
from valoricore.remote import ClusterClient

# Point at all nodes — client discovers the leader automatically.
c = ClusterClient([
    "http://node1:3000",
    "http://node2:3000",
    "http://node3:3000",
])

# Writes go to the leader (307-redirect self-heal).
rid = c.insert([0.1, 0.2, 0.3, 0.4], collection="tenant-acme")

# Local reads round-robin across all nodes for throughput.
hits = c.search([0.1, 0.2, 0.3, 0.4], k=5, consistency="local")

# Linearizable reads go to the leader.
hits = c.search([0.1, 0.2, 0.3, 0.4], k=5, consistency="linearizable")

# Cluster inspection.
print(c.leader_url())          # → 'http://node2:3000'
print(c.get_cluster_role())    # → 'leader'
print(c.cluster_health())      # → True
```

Every mutating call auto-generates a UUID4 idempotency key. On a retry after a
connection reset, the Raft cluster deduplicates the write server-side.
Pass `idempotency_key=my_bytes` to supply your own 16-byte token.
```

---

## Index configuration (Phase 3.13)

### `GET /v1/index/config`

Returns the active index type and its parameters.

```bash
curl http://localhost:3000/v1/index/config
# BruteForce (default):
# {"index_type":"brute_force","hnsw":null}

# HNSW:
# {"index_type":"hnsw","hnsw":{"m":16,"m_max0":32,"ef_construction":100,"ef_search":50}}
```

### HNSW environment variables

| Variable | Default | Description |
|---|---|---|
| `VALORI_HNSW_M` | `16` | Max edges per node per layer. `m_max0` and `lambda` are derived automatically (`m_max0 = 2*M`). |
| `VALORI_HNSW_EF_CONSTRUCTION` | `100` | Beam width during index build. Higher = better recall, slower inserts. |
| `VALORI_HNSW_EF_SEARCH` | `50` | Beam width floor during queries. Higher = better recall, slower search. |

Only takes effect when `VALORI_INDEX=hnsw`. Has no effect in cluster mode (cluster uses kernel brute-force for linearizable consistency).

### IVF environment variables (Phase P2)

| Variable | Default | Description |
|---|---|---|
| `VALORI_IVF_N_LIST` | auto | Fix centroid count. When absent, `n_list = max(16, sqrt(N))` is computed at build time. |
| `VALORI_IVF_N_PROBE` | auto | Fix probe count. When absent, `n_probe = max(1, sqrt(n_list))` is computed at build time. |

Only takes effect when `VALORI_INDEX=ivf`. Setting either variable disables auto-scaling and pins the values. The auto-scaling rule (`k = sqrt(N)`) keeps average bucket size near `sqrt(N)` and scan cost at O(sqrt(N)) regardless of dataset size — this is the FAISS-recommended operating point.

S11 real-measurement note: at 50K vectors/384D, search latency was
flat (~660ms) across every tested `n_list`/`n_probe` combination — no
configuration produced a meaningful latency win over BruteForce. A
small fixed `n_list` (e.g. 64) does meaningfully reduce restart/recovery
time vs the auto-scaled default (16.9s vs 47.4s at 50K), since
`Engine::rebuild_index()` re-runs k-means on every restart regardless
of index kind. See `docs/phases/phase-S11-index-tuning.md`.

### BQ environment variables (Phase S11)

| Variable | Default | Description |
|---|---|---|
| `VALORI_BQ_POOL_FACTOR` | 10 | Candidate pool = `max(pool_factor * k, min_candidates)`, evaluated before the exact-L2 re-rank stage. |
| `VALORI_BQ_MIN_CANDIDATES` | 200 | Floor on the candidate pool size. |

Only takes effect when `VALORI_INDEX=bq`. The candidate pool controls
recall: the default (200, i.e. 0.4% of a 50K corpus) measured
Recall@10=0.48 in S10. Widening it to `VALORI_BQ_MIN_CANDIDATES=10000`
(≈20% of a 50K corpus) measured Recall@10=0.99 in S11, at the cost of
losing BQ's latency edge over BruteForce (both land within noise of
each other at that setting). See `docs/phases/phase-S11-index-tuning.md`.

### Decay (Phase C4.1)

| Variable | Default | Description |
|---|---|---|
| `VALORI_DECAY_HALF_LIFE_SECS` | — | Default recency half-life (seconds) for search ranking. Per-request `decay_half_life_secs` overrides; omit or `0` = no decay. |

---

## Concurrency model (Phase 3.11)

The engine state is wrapped in `Arc<RwLock<Engine>>`. Read-only handlers
(search, proof, health, timeline, metrics, list collections, etc.) acquire a
shared read lock and execute concurrently. Write handlers (insert, delete,
restore, shred) acquire an exclusive write lock.

## Persistence funnel (Phase E1)

Every standalone mutation flows through ONE path:
`Engine::commit_and_apply_ns(event, ns)` → `Persistence::log_event_ns`
(durable log) → `apply_committed_event_ns` (state + index + derived maps).
`Persistence` is an enum — `EventLog(EventCommitter)` (canonical),
`Wal(WalWriter)` (legacy), or `Ephemeral` (in-memory). Do not add a write
method that logs or applies outside this funnel. Observability code reads
the committer via `engine.event_committer()` / `event_committer_mut()`.

`tests/architecture.rs` additionally fails the build if a source file with
the same crate-relative path exists in both `valori-node/src` and any of the
extracted crates (`valori-storage`, `valori-state`, `valori-metadata`) —
dead copies left behind by an extraction are a test failure, not a code
review hope.

**Extracted crates (Phase N-series):**

| Logic | Crate | Phase |
|-------|-------|-------|
| Decay re-rank, BM25 reranker, metadata filter | `valori-search` | N1 |
| BruteForce, HNSW, IVF, BQ, quantizers, deterministic k-means | `valori-index` | N2 |
| GraphRAG, Tree-RAG, Community Layer, LLM entity extraction | `valori-rag` | N3 |
| Embedding client (Ollama/OpenAI/custom), chunker (4 strategies), `POST /v1/ingest/document` handler | `valori-ingest` | N4 |
| `Engine` struct, `EngineConfig`, `EngineHealth`, `Persistence`, `MetadataStore`, `EngineError`, `CommitError` | `valori-engine` | N5 |

`valori-node` retains ownership of all HTTP routes, `NodeConfig`, `AesGcmVault` construction, and the `EngineFromNodeConfig` bridge trait. Extracted crates contain pure computation logic.

## Testing

```bash
cargo test -p valori-node
```

Key test suites:

| Suite | What it covers |
|---|---|
| `tests/collections.rs` | 16 tests: collection CRUD, namespace isolation, snapshot persistence, error paths. |
| `tests/cluster_boot.rs` | Single-node Raft boot, restart recovery from redb log, state-hash watcher teardown. |
| `tests/replication.rs` | Leader→follower snapshot push, `LeaderProof` hex-format verification. |
| `tests/api.rs` | All HTTP endpoints, status codes, and response shapes. |
| `tests/api_batch_idempotency.rs` | 4 tests: per-item dedup, mixed batches, backward compat, fully-deduped batch. |
| `tests/api_index_config.rs` | 5 tests: brute-force config, HNSW defaults, custom M derivation, ef_search, all params. |

## Effect system integration (Phases A7–A9)

`valori-node` wires the `valori-effect` capability model into the live node subsystems:

| Module | Role |
|---|---|
| `src/capabilities.rs` | Concrete capability implementations: `EngineKernelCapability` (standalone — `SharedEngine`), `RaftKernelCapability` (cluster — `raft.client_write()` + `state_hash()`), `HttpEmbedCapability`, `PassthroughHttpCapability`, `CapabilityRegistryBuilder` |
| `src/runner.rs` | `TaskRegistry` (maps 12 `TaskKind`s to `Arc<dyn Task>`), `TaskRunner` (topological execution, predecessor threading, retry), `run_graph()` |

`ReceiptStore` is available as `axum::Extension<Arc<ReceiptStore>>` in every handler.
Receipts are assembled by `ReceiptAssembler` (in `valori-effect`) and stored in the node-local
in-process store (last 256 receipts).

**Handlers that emit receipts (Phase A10/A11) — standalone + cluster:**

| Handler | Kind | State captured |
|---|---|---|
| `insert_record` | `OperationKind::Ingest` | `state_before` + `state_after` via `hash_state_blake3` (standalone) or SM hash (cluster) |
| `batch_insert` | `OperationKind::BatchInsert` | Same pattern; `count` captured in `OperationInputs` |
| `delete_record` | `OperationKind::Delete { mode: "hard" }` | Cluster path uses `raft_write_data` for `log_index` |
| `soft_delete_record` | `OperationKind::Delete { mode: "soft" }` | Cluster path uses `raft_write_data` for `log_index` |
| `search` | `OperationKind::Search` | Read-only; state captured at handler entry |

The `op_hash` in every receipt is `BLAKE3(kind_discriminant ‖ bincode(inputs) ‖ bincode(policy))` —
reproducible from the planning parameters alone, with no timestamps or data.
Remaining write handlers (`memory_upsert`, `consolidate`, `contradict`, `ingest`) are deferred to A12.
