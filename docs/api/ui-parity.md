# TypeScript / UI Parity — Phase API-1

How the TypeScript surfaces in `ui/` and `ui/studio/` model the Valori API,
and where they disagree with the implementation. **Audit only — no UI code was
changed in this phase.**

---

## 1. The three TypeScript layers

| Layer | Location | What it talks to |
|---|---|---|
| **BFF routes** | `ui/src/app/api/**/route.ts` (34 route groups) | server-side `fetch` → `valori-node` and/or `valori-daemon` |
| **Hooks** | `ui/src/lib/hooks/*.ts`, `ui/studio/src/lib/hooks/*.ts` | browser `fetch` → the BFF routes (never the node directly) |
| **Types** | `ui/src/types/valori.ts`, `ui/studio/src/types/valori.ts` | hand-written mirrors of Rust response shapes |

There is **no generated client anywhere**. Every request shape is hand-written
at the call site. The two `types/valori.ts` files are byte-identical copies —
a duplicated hand-maintained mirror, which is precisely the drift source the
OpenAPI contract is meant to remove.

`ui/studio/` is the embeddable `@valori/studio` package. Its components call
BFF paths (`/api/...`) that the *host application* is expected to provide, so
its coupling to the Valori API is indirect but real.

---

## 2. Hand-defined interfaces vs. the implementation

### `SearchRequest` (`ui/src/types/valori.ts`)

```ts
export interface SearchRequest {
  vector: number[];
  k: number;
  collection?: string;
  consistency?: "local" | "linearizable";
}
```

* **`vector` is not the wire field.** The node's `POST /v1/search` takes
  `query`. `ui/src/lib/valori-client.ts` maps it (`query: req.vector`) with a
  comment admitting the mismatch. Every other call site builds the body inline
  and uses `query` directly.
* Missing every other real request field: `as_of`, `as_of_log_index`,
  `decay_half_life_secs`, `rerank`, `query_text`, `metadata_filter`,
  `graph_rerank`.
* `consistency` is modelled as always-available; it is **cluster-only** on the
  server (the standalone `SearchRequest` struct has no such field and silently
  ignores it).

### `SearchResponse`

```ts
export interface SearchResponse {
  results: SearchResult[];
  state_hash?: string;
  queried_at?: string;
}
```

* **`state_hash` and `queried_at` do not exist on any server response.**
  `queried_at` is synthesised client-side in `valori-client.ts`
  (`new Date().toISOString()`); `state_hash` is never populated by anything.
* Missing the real optional response fields: `as_of_log_index`,
  `as_of_timestamp_unix`, `as_of_timestamp_iso`, `as_of_state_hash`.

### `SearchResult`

```ts
{ id, score, collection?, text?, source? }
```

* `id` + `score` match `SearchHit`.
* Missing the real optional fields `decay_factor`, `age_secs`,
  `graph_distance`.
* `text` and `source` are **not** on `SearchHit`. `collection` exists only on
  `MultiSearchHit`, and `source` only on `GraphRagHit`/`HybridHit`. This is one
  TS type flattening three distinct server hit shapes.

### `Collection`

```ts
export interface Collection { name: string; id?: number; record_count?: number }
```

Missing `dimension`, `metric`, `index`, `max_records` — all of which
`CollectionInfo` returns and all of which the Collection contract says a
collection owns. `useCollections.ts` defines its own richer `CollectionMeta`
that *does* carry `dimension`/`metric`/`index`, so the shared type is simply
stale relative to the hook that superseded it.

### `HealthResponse`

```ts
{ status: "ok"|"degraded"|"full", version?, dim?, index?, records?, nodes?, edges?, event_log_height? }
```

* Models the **standalone** `EngineHealth` shape only.
* `status` union is missing `"no-leader"`, which the **cluster** `/health`
  returns.
* `dim` and `index` are **not** on the standalone health body (they are on the
  cluster one — `dim` — and were project-level concepts before the
  Collection-scoped move). Standalone returns `collections`, `persistence`,
  `embed_enabled`, `embed_provider`, `shard_count`, `event_log_path`,
  `snapshot_path` — none of which are in the TS type.
* The two `/health` bodies are structurally different objects; one TS
  interface cannot honestly describe both.

### `ClusterStatus`

```ts
{ leader_id?, nodes: ClusterNode[], converged: boolean }
```

The server's `StatusView` returns `{node_id, current_leader, is_leader, term,
last_log_index, last_applied_index, members[]}`. `valori-client.ts` maps
`members → nodes` and **hard-codes `converged: true`** — a value the server
never asserts. `ClusterNode.state_hash` is declared but never populated
(`/v1/cluster/status` members carry `{id, raft_addr, api_addr, voter}`, not
`state_hash`; the field names differ too — `id` vs `node_id`, `api_addr`/
`raft_addr` vs `addr`).

### `ProofResponse`

```ts
{ final_state_hash, chain_height?, record_count?, event_count? }
```

`GET /v1/proof/state` returns **only** `{final_state_hash}`. The three optional
fields are never sent by that endpoint (`event_count` exists on
`/v1/proof/event-log`, under a different response type).

---

## 3. Call-site drift

### 3.1 `ui/src/lib/valori-client.ts` is dead code

Nothing in `ui/` imports it (verified by grep across `ui/src` and `ui/test`).
It contains the most severe drift in the tree and is worth calling out
precisely because a future contributor could revive it:

* `createCollection(name)` posts `{ name }` with **no `dimension` and no
  `metric`** → guaranteed **400** against the current node.
* `search()` posts to the **deprecated** `/search` alias rather than
  `/v1/search`.
* `getClusterStatus()` fabricates `converged: true`.
* `search()` fabricates `queried_at`.

**Recommendation: delete the file, or rewrite it against the v1 contract.**
Not done in this phase (audit-only).

### 3.2 `/v1/memory/meta/list` does not exist

`ui/src/app/api/contradictions/route.ts` calls

```
GET {node}/v1/memory/meta/list?prefix=contradiction:&limit=200
```

**No such route is registered on either router.** The code defends itself
(`if (!listRes.ok) return { contradictions: [] }`), so the contradiction review
queue silently returns an empty list forever. The Python SDK's deprecated
`list_contradictions()` calls *this UI route*, so the same dead path is reached
from two directions.

Either the endpoint must be implemented (a prefix scan over the metadata
sidecar) or the feature must be removed. Not a v1 contract item — it is
currently vapour.

### 3.3 Correct call sites (for contrast)

* `ui/src/lib/hooks/useCollections.ts::create` builds
  `{name, dimension, metric: "squared_l2", index?}` — **matches the contract
  exactly**, including omitting `index` when it would be `"brute"`.
* `ui/src/app/api/namespaces/route.ts` is a transparent pass-through of body
  and status.
* Studio's `IndexLifecycleTab` uses `POST/GET /v1/namespaces/{name}/index` with
  `{type, parameters}` and handles the 501 branch — correct against
  `IndexBuildRequest`/`IndexStatusResponse`.

### 3.4 `metric` is hard-coded

`useCollections.ts` writes `metric: "squared_l2"` as a literal. There is no
metric picker anywhere in the UI. That is *correct today* (only one metric
exists) but means adding a second metric is a UI change, not just a server
change. Modelling `Metric` as a real enum in the contract keeps that a
one-line UI change later.

### 3.5 Legacy namespace prefixing is a UI-only concept

`ui/src/app/api/namespaces/route.ts` splits collection names on the first
`--` to strip a legacy `"${project}--${collection}"` prefix, and
`useCollections` carries a `rawNamespace` alongside every `name`. **The server
knows nothing about this.** It is a client-side compatibility shim over
pre-dedicated-node data and must stay out of the public contract.

### 3.6 Routes the UI calls that are not Valori data-plane routes

Recorded so they are not mistaken for missing endpoints:

| Path | What it actually is |
|---|---|
| `POST {llm}/v1/chat/completions` | OpenAI-compatible upstream LLM (Ollama / OpenAI) |
| `POST {provider}/v1/embeddings` | OpenAI-compatible upstream embeddings |
| `GET {cloud}/v1/settings/public` | **Valori Cloud control plane** |
| `GET {cloud}/v1/regions` | Valori Cloud control plane |
| `POST {cloud}/v1/projects/{id}/provision` | Valori Cloud control plane |
| `GET {cloud}/v1/projects/{id}/status` | Valori Cloud control plane |
| `/v1/projects/*`, `/v1/models/*`, `/v1/system`, `/v1/config`, `/v1/events` | **valori-daemon** local control plane |

None of these belong in `valori-v1.yaml`, which covers the node data plane
only. See §33 of the phase brief and `api/README.md` for the boundary.

---

## 4. Summary table

| Area | UI expectation | Implementation | Severity |
|---|---|---|---|
| Search request field name | `vector` | `query` | **High** (only masked by a manual map in dead code) |
| Search request coverage | 4 fields | 11 fields | Medium (features unreachable from UI) |
| Search response `state_hash`/`queried_at` | present | **do not exist** | Medium (client-fabricated) |
| Search hit `text`/`source` | present | not on `SearchHit` | Medium |
| `Collection` type | name/id/record_count | + dimension/metric/index/max_records | Medium (stale; hook has its own richer type) |
| `HealthResponse` | one shape | **two incompatible shapes** | **High** |
| `ClusterStatus.converged` | boolean from server | fabricated `true` | Medium |
| `ClusterNode` field names | `node_id`/`addr` | `id`/`api_addr`/`raft_addr` | Medium |
| `ProofResponse` extra fields | 3 optional | endpoint returns 1 field | Low |
| `valori-client.ts::createCollection` | `{name}` | requires dimension+metric | **High** (but dead code) |
| `/v1/memory/meta/list` | called | **route does not exist** | **High** |
| Two duplicated `types/valori.ts` | hand-maintained | — | Medium (structural) |
| Legacy `--` namespace prefix | UI-only shim | server unaware | Informational |

---

## 5. What Phase 2 should do for the UI — outcome

1. **RESOLVED.** `@valori/api-types` exists as an internal workspace package
   (`ui/api-types/`), generated from `api/openapi/valori-v1.yaml` by
   `scripts/generate-api-types.sh` (`openapi-typescript@7`).
   `ui/api-types/src/valori-v1.ts` is machine output;
   `ui/api-types/src/index.ts` is the hand-written alias layer that maps
   `components["schemas"][…]` onto the short names the UI uses — so a renamed
   or deleted schema becomes a TypeScript error there instead of silent drift.
   Both `ui/src/types/valori.ts` and `ui/studio/src/types/valori.ts` now
   re-export from it rather than redeclaring the wire model. They were not
   deleted outright: each still carries the UI-only view types that were mixed
   into them, with a header comment naming every field that is app-derived
   rather than wire (`state_hash`, `queried_at`, `converged`).
2. **RESOLVED.** `ui/src/lib/valori-client.ts` deleted. Every import was
   checked first; it had none. Its `createCollection({name})` — which sent no
   dimension or metric — died with it.
3. **OPEN.** `/health` still returns two structurally different bodies. The
   contract records both; converging them is a separate phase (see
   `contract-conformance.md` §4 row 37). The UI types split accordingly rather
   than pretending one shape exists.
4. **OPEN.** `ui/src/app/api/contradictions/route.ts` still calls
   `/v1/memory/meta/list`, which no node route serves. The matching Python SDK
   methods are already `DeprecationWarning`-flagged. Removing the contradiction
   queue is a UI feature decision, not an API-stabilisation one.
5. **RESOLVED (documented, not removed).** `converged` and `queried_at` are
   still computed client-side — `converged` in `useCluster` from every member's
   state, `queried_at` by the app's own BFF route — but they are no longer
   typed as if the server sent them. Both files say so in their header.

### Summary-table rows that moved

| Row | Status |
|---|---|
| `valori-client.ts::createCollection` | **RESOLVED** — file deleted |
| Two duplicated `types/valori.ts` | **RESOLVED** — both consume `@valori/api-types` |
| Search response `state_hash`/`queried_at` | **RESOLVED** — no longer typed as wire fields |
| `ClusterStatus.converged` | **RESOLVED** — documented as derived |
| `HealthResponse` one shape | **OPEN** — server still forks |
| `/v1/memory/meta/list` | **OPEN** — route still does not exist |
