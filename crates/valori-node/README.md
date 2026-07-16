# valori-node

HTTP API server and orchestration layer for Valori. Runs in standalone mode
or as a member of a Raft cluster (`VALORI_CLUSTER_MEMBERS`).

## Base URL

- **Local**: `http://localhost:3000`
- **Production**: `https://<your-app>.koyeb.app`

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

---

## Collections (Multi-tenancy)

Valori supports up to **1 024 named collections** (namespaces). Every data
endpoint accepts an optional `"collection"` field. Omitting it (or setting it
to `"default"`) targets the always-present default collection.

Records in non-default collections are **fully isolated** — they never appear
in default-collection searches and vice versa.

| Endpoint | Method | Description |
|---|---|---|
| `/v1/namespaces` | `POST` | Create a collection (idempotent). |
| `/v1/namespaces` | `GET` | List all collections and their numeric IDs. |
| `/v1/namespaces/:name` | `DELETE` | Drop a collection and all its records. |

### Create a collection

```bash
curl -X POST http://localhost:3000/v1/namespaces \
  -H "Content-Type: application/json" \
  -d '{"name": "tenant-acme"}'
# → {"name":"tenant-acme","id":1,"created":true}
# Second call with the same name → {"created":false,"id":1} (idempotent)
```

### List collections

```bash
curl http://localhost:3000/v1/namespaces
# → {"collections":[{"name":"default","id":0},{"name":"tenant-acme","id":1}]}
```

### Drop a collection

```bash
curl -X DELETE http://localhost:3000/v1/namespaces/tenant-acme
# → 204 No Content
# Dropping "default" → 400 Bad Request
# Dropping an unknown collection → 404 Not Found
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
| `/search` | `POST` | K-nearest-neighbour search. `rerank=true` (default) + `query_text` enables the Valori Reranker (Phase C5). Supports `as_of` / `as_of_log_index` for point-in-time reads, `decay_half_life_secs` for recency-aware ranking (Phase C4.1), and `metadata_filter` for JSON predicate post-filtering (Phase I7). |
| `/v1/delete` | `POST` | Permanently remove a record by ID (accepts an optional `"collection"` field, S7). |
| `/v1/soft-delete` | `POST` | Mark a record inactive without removing it — searchable-off but still present for audit (accepts an optional `"collection"` field, S7). |
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

# Search the default collection (no "collection" field needed).
curl -X POST http://localhost:3000/search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.1, 0.2, 0.3, 0.4], "k": 5}'
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

### GraphRAG — `POST /v1/graphrag` (Phase 3.15)

Retrieve the K nearest vectors **and** the connected knowledge subgraph around
them in a single call, from one consistent kernel snapshot — no second store, no
cross-system drift. Vectors and graph live in the same kernel, so the KNN, the
record→node resolution, and the subgraph BFS all run under one read lock.

```bash
curl -X POST http://localhost:3000/v1/graphrag \
  -H "Content-Type: application/json" \
  -d '{"query_vector": [0.1, 0.2, 0.3, 0.4], "k": 5, "depth": 2}'
```

```jsonc
{
  "hits":       [ { "memory_id", "record_id", "score", "node_id", "metadata" } ],
  "seed_nodes": [ /* node ids the hits mapped to */ ],
  "subgraph":   { "nodes": [...], "edges": [...] }
}
```

`depth` is clamped to 4. The subgraph is only as rich as the edges that exist
(ingest creates a document→chunk edge per memory; entity/citation edges and
manual `/graph/edge` calls add more). On a cluster the request also honours
`consistency` (linearizable by default). For agents, prefer the
`memory_graph_recall` MCP tool, which wraps this with a verifiable receipt.

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
collections = client.list_collections()   # [{"name": "default", "id": 0}, ...]
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
