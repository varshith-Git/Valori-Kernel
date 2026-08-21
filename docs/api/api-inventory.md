# Valori API Inventory — Phase API-1

**Source of truth**: the route registration code, read on disk at audit time.

| Surface | File | Router builder |
|---|---|---|
| Data plane — standalone | `crates/valori-node/src/server.rs` | `build_router_with_keys()` (L306–…) |
| Data plane — cluster | `crates/valori-node/src/cluster_server.rs` | `build_cluster_router_with_keys()` (L778–…) |
| Cluster management plane | `crates/valori-node/src/cluster_api.rs` | `cluster_router()` (L54–72) |
| Local control plane (daemon) | `crates/valori-daemon/src/http.rs` | `router()` (L30–76) |

Nothing else in the workspace registers HTTP routes. `crates/valori-mcp` speaks
MCP over stdio (not HTTP). `crates/valori-ffi` is an in-process PyO3 module.
`ui/src/app/api/**` are Next.js BFF routes that *call* the node — they are not
part of the Valori API and are inventoried separately in
[`ui-parity.md`](./ui-parity.md).

This document records **what exists**, not what should exist. Divergences are
recorded, never silently normalised. See
[`current-vs-target.md`](./current-vs-target.md) for the gap analysis.

> **Phase API-2 update.** The route set below is unchanged — no route was
> added, removed or renamed. What changed is *behaviour behind* five of them,
> and the change is not visible in a path table:
>
> * `POST /v1/records` — both routers now deserialise one canonical
>   `InsertRecordRequest`; no field is silently dropped on either path, and
>   `request_id` is honoured standalone as well as on cluster.
> * `POST /v1/search` — `k` is required on both paths (the cluster's hidden
>   default of 10 is gone).
> * `POST /v1/search/multi` — unknown Collection is 404 on both paths, and the
>   endpoint now requires only `read_only`.
> * `POST /v1/graphrag` — now requires only `read_only`.
> * `POST /v1/cluster/add-node`, `/remove-node`, `/snapshot` — now require
>   `admin` instead of `read_write`.
>
> Every error response on every route now carries a machine-readable `code`
> alongside `error`, including the 401/403 that previously had no body at all.
> Full status in [`contract-conformance.md`](./contract-conformance.md).

---

## 1. Classification legend

| Class | Meaning |
|---|---|
| `PUBLIC` | Unauthenticated by design (liveness/scrape targets). |
| `AUTHENTICATED` | Requires a key with `read_only` or `read_write` scope when auth is enabled. |
| `ADMIN` | Requires `admin` scope (`required_scope()` in `api_keys.rs`). |
| `INTERNAL` | Node-to-node / operator plumbing. Should not ship in a client SDK. |
| `DEPRECATED` | Served with `Deprecation: true` + `Link` successor header. |

**Auth mechanics (verified in `server.rs::auth_guard_v2` / `cluster_server.rs::cluster_auth_guard`)**

* One scheme only: `Authorization: Bearer <token>`.
* Token is either an API key (`vk_<64 hex>`, BLAKE3-hashed in `KeyStore`) or the
  legacy static `VALORI_AUTH_TOKEN` (constant-time compared).
* **If no key store entries and no legacy token are configured, auth is entirely
  bypassed** (`AuthState::has_any_auth() == false` → `next.run(req)`).
* `/health` and `/metrics` are merged *outside* the auth layer on both routers.
* Scope is derived from `(method, path)` by `api_keys::required_scope()`, not
  from a per-route declaration. Rules, in order:
  1. path starts with `/v1/keys`, `/v1/snapshot`, `/v1/storage`, `/v1/replication` → `admin`
  2. path is `/search`, ends with `/search`, starts with `/v1/memory/search`,
     starts with `/v1/proof`, or is `/timeline`, `/v1/timeline`, `/health`,
     `/metrics`, `/version` → `read_only`
  3. method is `GET` → `read_only`
  4. everything else → `read_write`
* **`ApiKeyRecord.collection` (the per-key collection lock) is stored and returned
  but never enforced by either guard.** Recorded as a finding, not fixed here.

---

## 2. Data plane — matrix

Standalone = `server.rs`; Cluster = `cluster_server.rs`. "Async" = the request
returns before the work completes.

| Method | Path | Domain | Purpose | Standalone | Cluster | Auth | Async | Public SDK |
|---|---|---|---|---|---|---|---|---|
| GET | `/health` | Service | Node liveness + capacity | ✅ | ✅ | PUBLIC | no | ✅ |
| GET | `/metrics` | Service | Prometheus exposition | ✅ | ✅ | PUBLIC | no | ✗ (scrape target) |
| GET | `/v1/version` | Service | Crate version, `text/plain` | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/usage` | Service | records/collections/storage bytes | ✅ | ✅ | read_only | no | ✗ (Cloud metering) |
| GET | `/v1/models/health` | Service | Local model package store health | ✅ | ✅ | read_only | no | ✅ |
| POST | `/v1/namespaces` | Collections | Create collection | ✅ | ✅ | read_write | no | ✅ |
| GET | `/v1/namespaces` | Collections | List collections | ✅ | ✅ | read_only | no | ✅ |
| DELETE | `/v1/namespaces/:name` | Collections | Drop collection | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/namespaces/:name/index` | Indexes | Create / change / drop index | ✅ | ✅ | read_write | **yes (202)** | ✅ |
| GET | `/v1/namespaces/:name/index` | Indexes | Index lifecycle status | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/index/config` | Indexes | Node-level index config view | ✅ | ✅ | read_only | no | ✗ (vestigial) |
| POST | `/v1/index/rebuild` | Indexes | Rebuild all collection indexes | ✅ | ✅ | read_write | no | ✗ (operator) |
| POST | `/v1/records` | Records | Insert one vector | ✅ | ✅ | read_write | no | ✅ |
| GET | `/v1/records/:id` | Records | Fetch record by id | ✅ | ✅ | read_only | no | ✅ |
| PATCH | `/v1/records/:id/metadata` | Records | Replace record metadata | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/vectors/batch-insert` | Records | Batch insert | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/records/encrypted` | Records | Insert envelope-encrypted payload | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/delete` | Records | Hard delete | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/soft-delete` | Records | Soft delete (tombstone) | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/search` | Search | Single-collection kNN | ✅ | ✅ | read_only | no | ✅ |
| POST | `/v1/search/multi` | Search | Cross-collection kNN | ✅ | ✅ | read_only | no | ✅ |
| POST | `/v1/graphrag` | GraphRAG | kNN + subgraph in one read | ✅ | ✅ | read_write¹ | no | ✅ |
| POST | `/v1/graph/node` | Graph | Create node | ✅ | ✅ | read_write | no | ✅ |
| GET | `/v1/graph/node/:id` | Graph | Get node | ✅ | ✅ | read_only | no | ✅ |
| DELETE | `/v1/graph/node/:id` | Graph | Delete node | ✅ | ✅ | read_write | no | ✅ |
| GET | `/v1/graph/nodes` | Graph | List nodes (kind filter + paging) | ✅ | ✅ | read_only | no | ✅ |
| POST | `/v1/graph/edge` | Graph | Create edge | ✅ | ✅ | read_write | no | ✅ |
| GET | `/v1/graph/edges/:id` | Graph | Outgoing edges of a node | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/graph/subgraph` | Graph | BFS expansion from a root | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/graph/query` | Graph | Filtered depth-bounded traversal | ✅ | ✅ | read_only | no | ✅ |
| POST | `/v1/memory/upsert` | Memory | Vector + doc/chunk nodes in one op | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/memory/upsert_vector` | Memory | Alias of the above | ✅ | ✅ | read_write | no | ✗ (alias) |
| POST | `/v1/memory/search` | Memory | Recall with decay/rerank/filter | ✅ | ✅ | read_only | no | ✅ |
| POST | `/v1/memory/search_vector` | Memory | Alias of the above | ✅ | ✅ | read_only | no | ✗ (alias) |
| POST | `/v1/memory/consolidate` | Memory | Soft-delete + insert + Supersedes | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/memory/contradict` | Memory | Cosine check + Contradicts edge | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/memory/meta/set` | Memory | Metadata sidecar write | ✅ | ✅ | read_write | no | ✅ |
| GET | `/v1/memory/meta/get` | Memory | Metadata sidecar read | ✅ | ✅ | read_only | no | ✅ |
| POST | `/v1/ingest` | Ingest | chunk + embed + insert | ✅ | ✅ | read_write | optional² | ✅ |
| GET | `/v1/ingest/status/:job_id` | Ingest | Async ingest job status | ✅ | ✅ | read_only | no | ✅ |
| POST | `/v1/ingest/document` | Ingest | Chunk only (stateless) | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/ingest/update` | Ingest | Diff-based document re-ingest | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/ingest/extract-entities` | Ingest | LLM entity/relation extraction | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/tree/build` | Tree-RAG | Markdown → tree index | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/tree/query` | Tree-RAG | ToC navigation + receipt | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/tree/hybrid` | Tree-RAG | Tree + vector blend | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/tree/verify` | Tree-RAG | Replay one receipt (stateless) | ✅ | ✅ | read_write³ | no | ✅ |
| POST | `/v1/tree/chain-verify` | Tree-RAG | Verify a receipt chain (stateless) | ✅ | ✅ | read_write³ | no | ✅ |
| POST | `/v1/community/detect` | Community | Label-propagation detection | ✅ | ✅ | read_write | no | ✅ |
| POST | `/v1/community/search` | Community | Rank communities by centroid | ✅ | ✅ | read_write³ | no | ✅ |
| GET | `/v1/community/overview` | Community | Community summary | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/proof/state` | Proof | Current BLAKE3 state hash | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/proof/event-log` | Proof | Event-log + state hash receipt | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/proof/receipt` | Proof | Latest operation receipt | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/proof/receipt/:id` | Proof | Receipt by id | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/timeline` | Proof | Committed events, time-filtered | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/operations` | Operations | One entry per committed event | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/operations/:id` | Operations | Operation detail | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/operations/:id/execution` | Operations | Per-stage execution trace | ✅ | ✅ | read_only | no | ✅ |
| GET | `/v1/snapshot/download` | Snapshots | Raw snapshot bytes | ✅ | ✅ | **admin** | no | ✅ |
| POST | `/v1/snapshot/upload` | Snapshots | Restore from a raw body | ✅ | ✗ | **admin** | no | ✅ |
| POST | `/v1/snapshot/save` | Snapshots | Write snapshot to local path | ✅ | ✅ | **admin** | no | ✅ |
| POST | `/v1/snapshot/restore` | Snapshots | Restore from local path | ✅ | ✅ | **admin** | no | ✅ |
| GET | `/v1/storage/snapshots` | Snapshots | List object-store snapshots | ✅ | ✅ | **admin** | no | ✅ |
| POST | `/v1/storage/snapshots/upload` | Snapshots | Offload snapshot to object store | ✅ | ✅ | **admin** | no | ✅ |
| POST | `/v1/storage/snapshots/restore` | Snapshots | Restore from object store | ✅ | ⚠️ 501 | **admin** | no | ✅ |
| GET | `/v1/storage/manifest` | Snapshots | DR manifest (`manifest.json`) | ✅ | ✅ | **admin** | no | ✅ |
| GET | `/v1/storage/wal` | Snapshots | List archived WAL segments | ✅ | ✅ | **admin** | no | ✅ |
| POST | `/v1/storage/wal/archive` | Snapshots | Archive a sealed WAL segment | ✅ | ✅ | **admin** | no | ✗ (operator) |
| DELETE | `/v1/crypto/shred/:key_id` | Crypto | Crypto-shred a data key | ✅ | ✅ | read_write | no | ✅ |
| GET | `/v1/crypto/status/:key_id` | Crypto | Key existence check | ✅ | ✅ | read_only | no | ✅ |
| POST | `/v1/keys` | Admin | Mint an API key | ✅ | ✅ | **admin** | no | ✅ |
| GET | `/v1/keys` | Admin | List masked keys | ✅ | ✅ | **admin** | no | ✅ |
| DELETE | `/v1/keys/:id` | Admin | Revoke a key | ✅ | ✅ | **admin** | no | ✅ |
| GET | `/v1/shard/routing` | Admin | namespace→shard map | ✅ | ✅ | read_only | no | ✗ (operator) |
| GET | `/v1/replication/wal` | INTERNAL | Stream the WAL file | ✅ | ✗ | **admin** | no | ✗ |
| GET | `/v1/replication/events` | INTERNAL | Unbounded live event stream | ✅ | ✗ | **admin** | streaming | ✗ |
| GET | `/v1/replication/state` | INTERNAL | Replication watcher state | ✅ | ✗ | **admin** | no | ✗ |
| GET | `/v1/cluster/proof` | Cluster | Cluster-wide hash convergence | ✗ | ✅ | read_only | no | ✅ |

¹ `/v1/graphrag` is a POST that does not match any `read_only` rule in
`required_scope()`, so a read-only key is rejected. Same for `/v1/search/multi`
— it *does* end with `…/multi`, not `…/search`, so rule 2 does not fire.
Recorded as a finding.

² `POST /v1/ingest` becomes asynchronous when `async: true` is passed in the
body or `?async=true` in the query; it then returns a `job_id` polled via
`/v1/ingest/status/:job_id`. Synchronous otherwise.

³ Stateless/read-shaped handlers served over POST inherit `read_write` from
rule 4.

### Cluster management plane (`cluster_api.rs`, cluster mode only)

| Method | Path | Domain | Purpose | Auth | Public SDK |
|---|---|---|---|---|---|
| GET | `/v1/cluster/status` | Cluster | node id, leader, term, indices, members | read_only | ✅ |
| GET | `/v1/cluster/health` | Cluster | leader-elected check (503 if none) | read_only | ✅ |
| GET | `/v1/cluster/role` | Cluster | `leader`/`follower` (LB probe) | read_only | ✅ |
| GET | `/v1/cluster/read-index` | INTERNAL | read-index protocol leader half | read_only | ✗ |
| POST | `/v1/cluster/add-node` | Cluster (ADMIN) | Add a voter | read_write | ✗ (operator) |
| POST | `/v1/cluster/remove-node` | Cluster (ADMIN) | Remove a voter | read_write | ✗ (operator) |
| POST | `/v1/cluster/snapshot` | Cluster (ADMIN) | Trigger a Raft snapshot | read_write | ✗ (operator) |

> `add-node` / `remove-node` / `snapshot` are **not** matched by the `admin`
> prefix rules in `required_scope()` (they start with `/v1/cluster`, not
> `/v1/keys|snapshot|storage|replication`), so they are reachable by any
> `read_write` key. Recorded as a finding.

### Deprecated legacy aliases

Served by the same handlers behind `deprecation_warning` middleware, which adds
`Deprecation: true` and `Link: <https://docs.valori.ai/api/v1>; rel="successor-version"`.

| Method | Legacy path | Canonical v1 path | Standalone | Cluster |
|---|---|---|---|---|
| GET | `/version` | `/v1/version` | ✅ | ✗ |
| POST | `/records` | `/v1/records` | ✅ | ✅ |
| POST | `/search` | `/v1/search` | ✅ | ✅ |
| GET | `/timeline` | `/v1/timeline` | ✅ | ✗ |
| GET | `/operations` | `/v1/operations` | ✅ | ✅ |
| GET | `/operations/:id` | `/v1/operations/:id` | ✅ | ✅ |
| POST | `/graph/node` | `/v1/graph/node` | ✅ | ✅ |
| GET/DELETE | `/graph/node/:id` | `/v1/graph/node/:id` | ✅ | ✅ |
| GET | `/graph/nodes` | `/v1/graph/nodes` | ✅ | ✗ |
| POST | `/graph/edge` | `/v1/graph/edge` | ✅ | ✅ |
| GET | `/graph/edges/:id` | `/v1/graph/edges/:id` | ✅ | ✅ |
| GET | `/graph/subgraph` | `/v1/graph/subgraph` | ✅ | ✅ |
| POST | `/v1/vectors/batch_insert` | `/v1/vectors/batch-insert` | ✅ | ✅ |

**None of these are in the OpenAPI contract.** They exist, they are documented
here, and they are scheduled for removal in v2.

---

## 3. Local control plane — `valori-daemon`

`crates/valori-daemon/src/http.rs`. This process manages **project lifecycle on
one machine** (start/stop a `valori-node` per project). It is a *different API*
from the data plane and is **not** in `valori-v1.yaml`.

**It has no authentication middleware whatsoever.** It is intended to bind
loopback only.

| Method | Path | Purpose | Notes |
|---|---|---|---|
| GET | `/health` | Daemon liveness | `{status, service, version}` |
| GET | `/version` | Daemon version | `{version, api:"v1"}` |
| GET | `/v1/system` | Discovery — platform, pid, counts | |
| GET | `/v1/config` | Effective daemon config | |
| GET | `/v1/events` | Recent daemon events (`?limit=100`) | no cursor |
| POST | `/v1/shutdown` | Graceful daemon shutdown | exits the process |
| GET/POST | `/v1/workspaces` | List / create workspace | |
| PATCH/DELETE | `/v1/workspaces/:name` | Rename / delete workspace | |
| GET/POST | `/v1/projects` | List / create project | |
| GET/PATCH/DELETE | `/v1/projects/:name` | Detail / rename / delete | |
| POST | `/v1/projects/:name/start` | Start the project's node | → `NodeInfo` |
| POST | `/v1/projects/:name/stop` | Stop the node | → `NodeInfo` |
| POST | `/v1/projects/:name/restart` | Restart the node | → `NodeInfo` |
| GET | `/v1/projects/:name/logs` | Tail node logs (`?tail=200`) | |
| GET | `/v1/projects/:name/runtime` | Process resource stats | |
| GET | `/v1/projects/:name/cluster` | Aggregated per-node cluster health | |
| GET/POST | `/v1/projects/:name/collections` | **Proxy** to the node's `/v1/namespaces` | |
| DELETE | `/v1/projects/:name/collections/:collection` | **Proxy** to the node | |
| GET | `/v1/models` | Local model catalog | |
| POST | `/v1/models/install` | Install a model package | |
| GET/DELETE | `/v1/models/*id` | Model detail / remove | id may contain `/` |

**Project does not own vector configuration** in the *created* manifest:
`create_project()` (http.rs) hard-writes `dim: None, index: None` regardless of
what the request body sends, even though `CreateProjectRequest` still accepts
`dim`/`index`. `ProjectManifest` retains both as `Option`, explicitly labelled
"Legacy … vector config is now Collection-scoped". This matches the target
contract; the vestigial request fields do not.

**Daemon error → status mapping** (`DaemonError::into_response`):
`NotFound`→404, `AlreadyExists`/`Running`→409, `InvalidInput`/`InvalidState`→400,
`NoFreePort`/`NodeBinaryMissing`/`StartFailed`→503, `Io`/`Serde`/`Model`→500.
Body is `{"error": "<message>"}` — same shape as the node.

---

## 4. Per-endpoint detail (data plane)

Only fields verified in code are listed.

### 4.1 Collections

`POST /v1/namespaces` — body `CreateCollectionRequest` (`api.rs`):
`{name: string, dimension?: u32, metric?: string, index?: string}`.
Shared handler `routes/collections.rs::create_collection`:

* `name` trimmed; empty → 400; `>64` chars → 400; not `[a-zA-Z0-9_-]` → 400.
* `dimension` **required despite the `Option`** — absent → 400; `0` or
  `> MAX_DIM` → 400.
* `metric` **required** — absent → 400; unparseable → 400. Only `squared_l2`
  (aliases `l2`, `l2sq`) parses.
* `index` optional — absent means `IndexKind::Brute`, i.e. *index NONE*, not a
  BruteForceIndex object. Unparseable → 400. Accepted: `brute`/`bruteforce`,
  `hnsw`, `ivf`, `bq`, `auto`/`mstg`.
* 200 `{name, id, created}`. `created=false` when the name already existed.
* **"default" has no special case anywhere in this path.**

`GET /v1/namespaces` → 200 `{collections: [{name, id, dimension?, metric?,
index?, record_count?, max_records?}]}`. A brand-new project returns
`{"collections": []}`.

`DELETE /v1/namespaces/:name` → 404 unknown, else **204 No Content**.

### 4.2 Indexes

`POST /v1/namespaces/:name/index` — body `IndexBuildRequest`
(`valori-engine/src/index_manager.rs`): `{"type": "hnsw"|"ivf"|"bq"|null,
"parameters": {}}`. `type: null` = **drop**.

* unknown collection → 404
* unsupported `type` → 400 (`"supported: hnsw, ivf, bq, or null to drop"`)
* drop success → **200** + `IndexStatusResponse`
* build accepted → **202** + `IndexStatusResponse`
* build rejected (e.g. already building) → **409**
* drop failure → 500
* `supports_ann_builds() == false` → **501** with a "not yet supported in
  cluster mode" body. **As of Phase 4.3 both impls return `true`, so 501 is
  currently unreachable** — the branch remains in the code.

`GET /v1/namespaces/:name/index` → 404 unknown, else 200 `IndexStatusResponse`:
`{collection, active_type, active_generation?, desired_type?, status,
building_generation?, base_lsn?, build_started_at?, error?}`.

`IndexState` (the real runtime enum): `None | Building | Ready | Active |
Failed | Retiring` — serialised lowercase. `IndexStatusResponse.status` is
derived, not the raw enum: `building_generation` present → that generation's
state; else `active_generation` present → `"active"`; else last failed
generation exists → `"failed"`; else `"none"`. So `ready` and `retiring` are
reachable in `status` only via the building-generation branch.

`active_type` is `"none"` when no ANN index is active — the wire tag for
"exact search".

### 4.3 Records

`POST /v1/records`
* standalone body `InsertRecordRequest`: `{values: [f32], collection?, text?}`;
  response `{id, receipt}`.
* cluster body `InsertRequest`: `{values: [f32], metadata?: [u8], tag?: u64,
  request_id?: [u8;16], collection?}`; response `{id, log_index, deduplicated,
  receipt}`.
* **The two bodies and the two responses are different types.** Recorded as
  drift.

`POST /v1/vectors/batch-insert` — `BatchInsertRequest`:
`{batch: [[f32]], collection?, metadata?: [string|null], request_ids?:
[string|null] (32-hex), texts?: [string|null]}` → `{ids: [u32]}`.
`request_ids` is the only **idempotency key** in the whole API.

`GET /v1/records/:id?collection=` → `{id, vector, metadata, tag}`; 404 when the
record is missing **or belongs to another namespace** (never distinguished).

`PATCH /v1/records/:id/metadata?collection=` — body is an arbitrary JSON object
(replaces, not merges) → `{ok: true, id}`; 404 unknown; 500 on kernel failure.

`POST /v1/delete` / `POST /v1/soft-delete` — `{id, collection?}` →
`{success: true, log_index?}`. Unknown collection → 404 (both paths, unified in
`routes/records.rs`). Unknown record → 404.

`POST /v1/records/encrypted` — `{payload: base64, tag?, collection?, key_id?}`
→ **201** `{id, key_id}`; bad base64 or bad `key_id` hex → 400.

### 4.4 Search

`POST /v1/search`

| Field | Standalone | Cluster |
|---|---|---|
| `query: [f32]` | required | required |
| `k: usize` | **required** | optional, default **10** |
| `collection` | ✅ | ✅ |
| `as_of: ISO8601` | ✅ | ✗ |
| `as_of_log_index: u64` | ✅ | ✗ |
| `decay_half_life_secs` | ✅ | ✅ |
| `rerank: bool` (default `true`) | ✅ | ✅ |
| `query_text` | ✅ | ✅ |
| `metadata_filter` | ✅ | ✅ |
| `graph_rerank {seed_count, weight, direction, max_depth}` | ✅ | ✅ |
| `consistency: "linearizable"\|"local"` | ✗ (ignored) | ✅, default linearizable |

Validation: `k == 0 || k > 5000` → 400 on both (`MAX_SEARCH_K = 5000`).

Response — standalone `SearchResponse`: `{results: [SearchHit], as_of_log_index?,
as_of_timestamp_unix?, as_of_timestamp_iso?, as_of_state_hash?}`.
Cluster returns `{"results": [...]}` only.
`SearchHit`: `{id, score, decay_factor?, age_secs?, graph_distance?}`.

**Score semantics (both paths): `score` is the raw squared-L2 distance. Lower is
better. It is never normalised or inverted, and decay never mutates it — decay
changes *ranking* (`score / decay_factor`) while `score` stays the true
distance.**

`POST /v1/search/multi` — `MultiSearchRequest`: `{query, k, collections: [name],
decay_half_life_secs?, metadata_filter?}`.

* empty `collections` → 400; `> 32` → 400 (`MAX_MULTI_COLLECTIONS`)
* `k == 0 || k > 5000` → 400
* **compatibility (`routes/query_planner.rs::check_compatibility`): every listed
  collection must share the same `dim` AND the same `metric`. Index type may
  differ freely.** Mismatch → 400 with the offending pair named.
* query length ≠ shared dim → 400. Vectors are never padded or truncated.
* unknown collection → **400 standalone / 404 cluster** (drift)
* collection with no vector config → **400 standalone / 500 cluster** (drift)

Response `MultiSearchResponse`: `{results: [{collection, id, score,
decay_factor?, age_secs?}], collections_searched: [name], partial_failures?:
[{collection, error}]}`. **Every hit carries its `collection`** — the
cross-collection identity requirement is met.

BM25 rerank and graph rerank are deliberately excluded from multi-search
(scores from different corpora are not comparable; graph edges are
collection-scoped).

### 4.5 Graph

Graph state is namespace-scoped. Every handler resolves `collection` → `ns` and
operates inside it; unknown collection → 404 on both paths. There is no
cross-collection edge and no project-wide graph.

`POST /v1/graph/node` — `{kind: u8, record_id?: u32, collection?}` →
`{node_id, log_index?}`. Unknown `kind` → 400 (unified; standalone previously
coerced silently).
`POST /v1/graph/edge` — `{from, to, kind: u8, collection?}` → `{edge_id, log_index?}`.
`GET /v1/graph/node/:id?collection=` → `{kind, record_id, namespace_id}`; 404.
`DELETE /v1/graph/node/:id?collection=` → `{success, log_index?}`; 404.
`GET /v1/graph/nodes?collection=&kind=&offset=&limit=` → `{nodes: [{node_id,
kind, record_id, namespace_id}], count}`. **Absent `collection` lists the
DEFAULT namespace only** (a cluster-side tenant leak fixed in R2).
`GET /v1/graph/edges/:id?collection=` → `{edges: [{edge_id, to_node, kind}]}`.
`GET /v1/graph/subgraph?root=&depth=2&collection=` → `{nodes, edges}` JSON
arrays where a node is `{id, kind, record}` and an edge is `{id, from, to, kind}`.
`GET /v1/graph/query?start=&direction=&edge_kind=&node_kind=&depth=&limit=&collection=`
→ `{hits: [{node_id, kind, record_id, depth}], count}`; unknown `start`
(including "exists in another namespace") → 404.

`kind` is a raw `u8` on the wire for both nodes and edges — the kernel's
`NodeKind`/`EdgeKind` discriminants leak into the public API. Recorded as a
finding.

### 4.6 GraphRAG

`POST /v1/graphrag` — request (identical fields both paths; cluster adds
`consistency`):

```
query_vector: [f32]          required
k?: usize                    legacy alias for retrieval_k
retrieval_k?: usize          vector seed count (default 5, min 1)
final_k?: usize              result cap (default = retrieval_k)
max_graph_candidates?: usize graph-only budget (default 100, min 1)
max_nodes?: usize            BFS node budget (default unlimited)
max_edges?: usize            BFS edge budget (default unlimited)
graph_weight?: f32           β, clamped [0,1], default 0.3
depth?: u32                  default 2
collection?: string
consistency?: string         cluster only
```

Response (built in `capabilities.rs::graph_rag`):

```
{ "hits": [ {
     "memory_id": "rec:<record_id>",
     "record_id": u32,
     "score":        f32 | null,   // backward-compat duplicate of vector_score
     "vector_score": f32 | null,   // null iff source == "graph"
     "graph_score":  f64,          // ALWAYS present, ∈ [0,1]
     "final_score":  f64,          // ALWAYS present, ∈ [0,1]
     "node_id":      u32 | null,
     "graph_distance": u32 | null,
     "source": "vector" | "vector_and_graph" | "graph",
     "metadata": any | null
  } ],
  "seed_nodes": [u32],
  "subgraph": { "nodes": [...], "edges": [...] } }
```

Verified provenance semantics:

* **vector-only** — hit was in the kNN top-`retrieval_k` and its record maps to
  no graph node. `source="vector"`, `graph_distance=null`, `graph_score=0.0`,
  `final_score=(1-β)·vector_rel`.
* **vector+graph** — kNN hit whose record maps to a node. `source=
  "vector_and_graph"`, `graph_distance=0` (a seed is distance 0 from itself),
  `graph_score=1.0`.
* **graph-only** — record reached by BFS from the seeds but absent from the kNN
  results. `source="graph"`, `score=null`, `vector_score=null`,
  `final_score=β·graph_score`. Graph-only candidates are sorted by
  `(distance asc, record_id asc)` and truncated to `max_graph_candidates`
  *before* the merge.
* Ranking: `vector_rel = 1/(1+L2)`, `graph_rel = 1/(1+hops)`,
  `final_score = (1-β)·vector_rel + β·graph_rel`. All candidates go into one
  list sorted by `final_score` DESC, `record_id` ASC, then truncated to
  `final_k`. **At high β a graph-only candidate can outrank a vector hit.**
* `seed_nodes` is the list of node ids resolved from the kNN records, in kNN
  order, deduplicated only by the resolver.

`score` and `vector_score` always carry the same value; `score` exists only for
backward compatibility.

### 4.7 Memory

`POST /v1/memory/upsert` — `{vector, collection?, attach_to_document_node?,
tags?, metadata?}` → `{memory_id, record_id, document_node_id, chunk_node_id,
log_index?}`. (`tags` is accepted and currently unused.)
`POST /v1/memory/search` — `{query_vector, k, collection?, decay_half_life_secs?,
consistency?, metadata_filter?, rerank?, query_text?}` → `{results:
[{memory_id, record_id, score, metadata, decay_factor?, age_secs?}]}`.
`POST /v1/memory/consolidate` — `{old_record_id, new_vector, collection?,
metadata?}` → `{old_record_id, new_record_id, supersedes_edge_id, state_hash,
log_index?}`.
`POST /v1/memory/contradict` — `{record_a, record_b, threshold?, collection?}` →
`{record_a, record_b, similarity, contradicts, edge_id?, state_hash, log_index?}`.
`POST /v1/memory/meta/set` — `{target_id, metadata}` → `{success: true}`.
`GET /v1/memory/meta/get?target_id=` → `{target_id, metadata|null}`.

### 4.8 Ingest / Tree / Community

`POST /v1/ingest` — `{text, collection?, strategy?, source?, chunk_size?,
chunk_overlap?, async?}` (also `?async=`) → `{ok, document_node_id,
strategy_used, chunk_count, record_ids, collection, operation_id}`. Requires
`VALORI_EMBED_PROVIDER`; 422 when absent.
`POST /v1/ingest/update` — `IngestUpdateRequest` → `IngestUpdateResponse`
(kept/removed/added counts).
`POST /v1/ingest/extract-entities` — `{text, namespace?, entity_types?, model?}`
→ `{entities, relationships, entity_count, relationship_count,
skipped_relationships}`.
`POST /v1/tree/build` — `{text, doc_name?}` → `{cache_key, doc_name, node_count,
structure_map, tree}`.
`POST /v1/tree/query` — `{tree?|cache_key?, query, k=2, prev_hash?}`.
`POST /v1/tree/hybrid` — `{query, text?, tree?, cache_key?, namespace?, k=2,
tree_weight=0.6, prev_hash?, doc_name?}` → `{query, hits: [HybridHit],
tree_hit_count, vector_hit_count, tree_answer, reasoning}`.
`POST /v1/tree/verify` — `{tree, receipt}` → `{valid: bool}` (pure).
`POST /v1/tree/chain-verify` — `{receipts: [Receipt]}` → `{valid, broken_at?}`.
`POST /v1/community/detect` — `{namespace?, max_iter?}`.
`POST /v1/community/search` — `{vector, k=5, depth=2, namespace?, drill_in=false}`
→ `{communities: [{community_id, score, member_count, sample_node_ids}],
total_communities_searched}`.

**Naming inconsistency**: tree/community/entity endpoints use `namespace`, the
rest of the API uses `collection`, for the same concept.

### 4.9 Proof / Operations / Timeline

`GET /v1/proof/state` → `{final_state_hash: "<64 hex>"}` (identical wire shape
on both paths).
`GET /v1/proof/event-log` → `EventProofResponse` `{kernel_version,
event_log_hash, final_state_hash, snapshot_hash?, event_count,
committed_height}`. Requires an event log; 400 when absent.
`GET /v1/proof/receipt` → the latest `Receipt` from the in-memory
`ReceiptStore` (capacity 256); 404 when empty. `GET /v1/proof/receipt/:id` →
404 when unknown. **Receipts are not durable — a restart empties the store.**
`GET /v1/timeline?from=&to=` (ISO 8601) → `{events: [TimelineEntry], total,
from_unix?, to_unix?}`; 400 when no event log. Reads **shard 0 only** in a
sharded deployment (known gap).
`GET /v1/operations` → `{operations: [OperationSummary], total}` — **empty list,
not an error, when no event log**. `GET /v1/operations/:id` → 404 unknown.
`GET /v1/operations/:id/execution` → per-stage trace; 404 when the id never ran
through the planner. **Two different id spaces share this path prefix**: `op-N`
ids from `/v1/operations` (one per committed kernel event) and planner
execution ids returned by `/v1/ingest`.

### 4.10 Snapshots / object store

`GET /v1/snapshot/download` → raw bytes.
`POST /v1/snapshot/upload` → raw body (standalone only).
`POST /v1/snapshot/save` — `{path?}` → `{success, path}`. `path` is validated
by `safe_path()`: no `..`, must stay inside the configured data dir, absolute
paths rejected when no data dir is configured.
`POST /v1/snapshot/restore` — `{path}` → `{success}`.
`GET /v1/storage/snapshots` → `{snapshots, count}`.
`POST /v1/storage/snapshots/upload` → `{key, state_hash, size_bytes, pruned}`.
`POST /v1/storage/snapshots/restore` — `{key?}`; omitted `key` resolves via
`manifest.json`. → `{key, state_hash, size_bytes}`. **Cluster: 501.**
`GET /v1/storage/manifest` → `{manifest: SnapshotManifest|null}`.
`GET /v1/storage/wal` → `{segments, count}`.
`POST /v1/storage/wal/archive` — `{path}` → `{key, size_bytes}`.

**When `VALORI_OBJECT_STORE_URL` is unset every storage endpoint returns 400**
(`EngineError::InvalidInput`), not 501/503. Recorded as a finding.

`SnapshotManifest` is the only object-store internal exposed; `StorageKey` and
the provider implementation are not.

---

## 5. Cross-cutting behaviour

### 5.1 Error shape

**The single canonical error body is `{"error": "<human message>"}`** — emitted
by `EngineError::into_response()` (`valori-engine/src/error.rs`), by every
hand-built `Json(json!({"error": …}))` in the handlers, and by
`DaemonError::into_response()`. There is **no machine-readable error code**,
**no `details`**, and **no `request_id`** anywhere in the codebase.

Exceptions found:
* `auth_guard_v2` / `cluster_auth_guard` return a bare `StatusCode` (401/403)
  with **no body at all**.
* `not_leader_response()` returns `{"error": "not-leader", "leader_api_addr":
  …}` + `Location` header, status **307**.
* `cluster_api::leadership_error` returns `{"error": "not-leader-or-rejected" |
  "raft-fatal", "detail": …}`, status **403**.
* `read_index` failure returns `{"error", "leader", "shard", "detail"}`, 503.
* `index_lifecycle::cluster_unsupported_response` returns `{"error", "note"}`, 501.
* `models_health` returns `{"error": …}` with status **200** on failure.

### 5.2 Status codes actually emitted

`200, 201, 202, 204, 307, 400, 401, 403, 404, 409, 422, 500, 501, 502, 503, 507`.

Notable mappings from `EngineError::into_response`:

| Condition | Status |
|---|---|
| `KernelError::NotFound` | 404 |
| `KernelError::CapacityExceeded` | **507 Insufficient Storage** |
| `KernelError::DimensionMismatch` | 400 |
| `KernelError::MetadataTooLarge` (>4 KB) | 400 |
| `KernelError::QueryOutOfRange` (Q16.16 range) | 400 |
| `KernelError::NamespaceAlreadyConfigured` | 409 |
| `KernelError::NotImplemented` | 501 |
| `EngineError::InvalidInput` | 400 |
| `EngineError::Network` | **502 Bad Gateway** |
| `EngineError::Internal` / `Unknown` | 500 |

Cluster-only: **307** (not leader, with `Location`), **422** (Raft rejected the
write, e.g. duplicate `request_id`), **503** (raft write failed / no quorum /
readiness gate not satisfied).

**429 is never emitted. There is no rate limiting in the node.**

### 5.3 Standalone / cluster parity matrix

Path/method parity is mechanically enforced by
`crates/valori-node/tests/route_parity.rs` (two tests, with `STANDALONE_ONLY` /
`CLUSTER_ONLY` allowlists). Request/response/status parity is **not** enforced
anywhere; the column below is this audit's finding.

| Capability | REST | Standalone | Cluster | Request same | Response same | Status same |
|---|---|---|---|---|---|---|
| Collections create/list/drop | `/v1/namespaces*` | ✅ | ✅ | ✅ | ✅ | ✅ (shared handler) |
| Index lifecycle | `/v1/namespaces/:n/index` | ✅ | ✅ | ✅ | ✅ | ✅ (shared handler) |
| Insert record | `POST /v1/records` | ✅ | ✅ | ❌ `text` vs `metadata`/`tag`/`request_id` | ❌ cluster adds `log_index`, `deduplicated` | ⚠️ cluster also 307/422 |
| Batch insert | `POST /v1/vectors/batch-insert` | ✅ | ✅ | ✅ | ✅ | ⚠️ cluster also 307/422 |
| Get / patch record | `/v1/records/:id[/metadata]` | ✅ | ✅ | ✅ | ✅ | ✅ |
| Delete / soft-delete | `/v1/delete`, `/v1/soft-delete` | ✅ | ✅ | ✅ | ⚠️ `log_index` cluster-only (omitted) | ✅ (shared handler) |
| Search | `POST /v1/search` | ✅ | ✅ | ❌ `as_of*` standalone-only; `consistency` cluster-only; `k` required vs default 10 | ❌ `as_of_*` fields standalone-only | ✅ for 400; cluster adds 307/503 |
| Multi-search | `POST /v1/search/multi` | ✅ | ✅ | ✅ | ✅ | ❌ unknown collection 400 vs 404; no-config 400 vs 500 |
| Graph (all) | `/v1/graph/*` | ✅ | ✅ | ✅ | ⚠️ `log_index` cluster-only (omitted) | ✅ (shared handler) |
| GraphRAG | `POST /v1/graphrag` | ✅ | ✅ | ⚠️ cluster adds `consistency` | ✅ | ⚠️ cluster adds dim-mismatch 400, 404, 503 |
| Memory | `/v1/memory/*` | ✅ | ✅ | ✅ | ⚠️ `log_index` cluster-only | ✅ |
| Health | `GET /health` | ✅ | ✅ | n/a | ❌ **completely different objects** | ✅ (200/503) |
| Snapshot restore from store | `POST /v1/storage/snapshots/restore` | ✅ | ⚠️ | ✅ | ❌ cluster always 501 | ❌ |
| Raw snapshot upload | `POST /v1/snapshot/upload` | ✅ | ✗ | — | — | 404 on cluster |
| WAL/event replication | `/v1/replication/*` | ✅ | ✗ | — | — | 404 on cluster |
| Cluster proof | `GET /v1/cluster/proof` | ✗ | ✅ | — | — | 404 standalone |

`GET /health` bodies:
* standalone `EngineHealth`: `{status, version, collections, persistence,
  records{live,slots_used,capacity,fill_pct}, nodes{…}, edges{…},
  event_log_height?, event_log_path?, snapshot_path?, embed_enabled,
  embed_provider?, shard_count, …}`; 503 when `status == "full"`.
* cluster: `{status: "ok"|"no-leader", leader?, dim, embed_enabled,
  embed_provider}`; 503 when there is no leader.

### 5.4 Consistency

Client-visible knobs:
* `consistency: "linearizable" | "local"` on cluster `POST /v1/search`,
  `POST /v1/memory/search`, `POST /v1/graphrag`. Default **linearizable** —
  the node performs a read-index round trip and waits until its applied index
  reaches the leader's read index before serving. `local` skips it
  (eventually consistent).
* `as_of` / `as_of_log_index` on **standalone** `POST /v1/search` only — a
  point-in-time replay of the event log. Response echoes `as_of_log_index`,
  `as_of_timestamp_unix`, `as_of_timestamp_iso`, `as_of_state_hash`.
* Writes on the cluster path always go through Raft; a follower answers
  **307** with the leader's `Location`.
* There is **no LSN/version field on records** and no read-your-writes token.

### 5.5 Idempotency

| Operation | Classification | Mechanism |
|---|---|---|
| `POST /v1/namespaces` | idempotent | name lookup; `created: false` on repeat |
| `DELETE /v1/namespaces/:name` | not idempotent | second call → 404 |
| `POST /v1/records` (cluster) | idempotent *if* `request_id` supplied | `ClientRequest.request_id` dedup in the state machine |
| `POST /v1/records` (standalone) | **not idempotent** | no `request_id` field on the standalone body |
| `POST /v1/vectors/batch-insert` | per-item idempotent *if* `request_ids` supplied | repeated key → item skipped, prior id returned |
| `POST /v1/delete`, `/v1/soft-delete` | not idempotent | second call → 404 |
| `POST /v1/graph/node`, `/v1/graph/edge` | not idempotent | fresh id each call |
| `POST /v1/memory/upsert` | **not idempotent** despite the name | always allocates a new record |
| `POST /v1/namespaces/:n/index` | effectively idempotent | new generation per call; 409 while one is building |
| `POST /v1/snapshot/save`, `/v1/storage/snapshots/upload` | retry-safe | rewrites the same target / new keyed object |
| `POST /v1/cluster/add-node` / `remove-node` | idempotent (Raft membership) | |

**There is no `Idempotency-Key` header support anywhere.** The only idempotency
mechanism is the body-level `request_id` / `request_ids`.

### 5.6 Pagination

Only one endpoint paginates: `GET /v1/graph/nodes` (`offset`, `limit`; absent
`limit` returns everything).

Everything else is unbounded: `GET /v1/namespaces`, `/v1/operations`,
`/v1/timeline` (time-filtered only), `/v1/storage/snapshots`, `/v1/storage/wal`,
`/v1/cluster/status` members, daemon `/v1/projects`, `/v1/workspaces`,
`/v1/models`. Daemon `/v1/events?limit=` and `/v1/projects/:n/logs?tail=` are
tail-limits, not cursors. **No cursor-based pagination exists in the codebase.**

### 5.7 Timeouts

The API exposes **no request timeout field at all**. `timeout` appears only as
a client-side constructor argument in the Python SDK and in internal
`fetchWithTimeout` helpers in the UI. Nothing in either Rust router reads a
timeout from a request. Nothing to standardise yet.
