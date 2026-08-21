# Current vs Target — Valori API v1

What the implementation does **today** versus what
[`api/openapi/valori-v1.yaml`](../../api/openapi/valori-v1.yaml) defines as the
canonical v1 contract, per product area.

Severity: **P0** = clients get wrong or unusable behaviour; **P1** = real drift
a client must work around; **P2** = cosmetic / naming / future-proofing.

No code was changed in this phase. Every row is a Phase-2 work item.

---

## Legend for the contract's own status marks

Every operation in `valori-v1.yaml` carries `x-status`:

* `current` — implemented and verified in this audit.
* `target` — **not implemented**; present in the contract as a stated intent.

Only two operations are marked `target`
(`restore_snapshot_from_store` in cluster mode is annotated as partially
unimplemented via `x-cluster-status: not_implemented`, and `search`'s unified
`consistency`/`as_of` semantics are annotated per-field). Everything else in
the document exists in code today.

---

## Master table

| # | Area | Current implementation | Target API v1 | Severity | Notes |
|---|---|---|---|---|---|
| 1 | **Project** | Owned entirely by `valori-daemon` (`/v1/projects/*`). `ProjectManifest.dim`/`.index` still exist as `Option`, marked "legacy". `CreateProjectRequest` still *accepts* `dim`/`index`. | Project is an infrastructure boundary. It owns identity, workspace, restart policy, cluster topology, storage limits, embedding config. It owns **no** vector config. | P1 | `create_project()` already hard-writes `dim: None, index: None`, so the accepted fields are inert. Remove them from the request type and from the manifest in Phase 2. **Project is not in `valori-v1.yaml` at all** — it belongs to the control plane. |
| 2 | **Zero-collection project** | ✅ Already correct. `Engine` has no config-free `create_collection(name)`; a new project lists `{"collections": []}`. | Same. | — | Verified in `routes/collections.rs` and `engine.rs::create_collection` doc contract. |
| 3 | **"default" collection** | ✅ Already correct. `parse_collection_config` has no name-based exception; `"default"` needs explicit dimension+metric like any other name. | Same. | — | The constant `api::DEFAULT_COLLECTION = "default"` and `validate_collection()` survive as vestigial, self-documented, unwired code. |
| 4 | **Collection creation** | `POST /v1/namespaces`. `dimension`/`metric` are typed `Option` but **rejected with 400 when absent**. | Contract declares both `required`, so a generated client cannot omit them. | P2 | Type/contract mismatch only — behaviour is right. Making the Rust fields non-`Option` in Phase 2 turns a runtime 400 into a deserialisation 422 and loses the good error message; keep the current shape and let the contract carry `required`. |
| 5 | **Collection resource path** | Path segment is `namespaces`; the response body, the SDK, the UI and this contract all say *collection*. | Contract uses `/v1/namespaces` (the real path) with tag `Collections` and operationIds `create_collection`/`list_collections`/`delete_collection`. | P2 | Renaming the path is a v2 breaking change. Do **not** rename in v1. |
| 6 | **Metric** | Only `Metric::SquaredL2`. Parses `squared_l2`, `l2`, `l2sq`. | Contract models `Metric` as an enum whose **only** member is `squared_l2`. Aliases are documented as accepted-but-not-canonical. | — | Correctly no cosine/dot/ip invented. |
| 7 | **Index kind** | `IndexKind` = `brute\|hnsw\|ivf\|bq\|auto` (+ aliases `bruteforce`, `mstg`) on collection **creation**; `IndexBuildRequest.type` = `hnsw\|ivf\|bq\|null` on the **lifecycle** endpoint. | Two distinct enums in the contract: `IndexKind` (creation) and `IndexType` (lifecycle, no `brute`/`auto`; `null` = drop). | P1 | Genuine drift: `auto` is accepted at creation but is **not** an acceptable `IndexBuildRequest.type`. Two enums for one concept. Converge in Phase 2. |
| 8 | **IndexSpec vs IndexStatus** | ✅ Already separated: `IndexSpec{index_type, parameters}` vs `IndexStatusResponse{active_type, active_generation, desired_type, status, building_generation, base_lsn, build_started_at, error}`. | Same, as two schemas. | — | Good. `IndexGeneration` stays internal. |
| 9 | **Index states** | Runtime enum `IndexState` = `none\|building\|ready\|active\|failed\|retiring`. `IndexStatusResponse.status` is a **derived** string, not the raw enum — `ready`/`retiring` surface only through the building-generation branch. | Contract exposes `IndexState` with all six values and documents that `status` is derived. | P1 | The derivation makes some states practically unobservable and makes `status` non-round-trippable. Document now; consider exposing the raw generation list in Phase 2. |
| 10 | **Index 501 branch** | `cluster_unsupported_response()` (501) is **unreachable** — both `IndexOps` impls return `supports_ann_builds() == true` since Phase 4.3. | Contract does not advertise 501 for index endpoints. | P2 | Dead branch. Delete or keep as defence; do not document. |
| 11 | **Insert record** | Standalone body `{values, collection?, text?}` → `{id, receipt}`. Cluster body `{values, metadata?, tag?, request_id?, collection?}` → `{id, log_index, deduplicated, receipt}`. | **One** `InsertRecordRequest` (union of both, all extras optional) and **one** `InsertRecordResponse` with `log_index`/`deduplicated` optional. | **P0** | The single worst parity break in the API. A client written against standalone loses idempotency and metadata; a client written against cluster loses BM25 `text`. |
| 12 | **Idempotency (standalone)** | Standalone `InsertRecordRequest` has no `request_id`; serde silently drops it. The Python SDK sends `request_id` on **every** insert — a no-op standalone. | `request_id` accepted and honoured on both paths. | **P0** | Users believe inserts are idempotent when they are not. |
| 13 | **`tag` / `filter_tag`** | `tag` exists on cluster insert and on `GET /v1/records/:id` output, but not on standalone insert. `filter_tag` is sent by the SDK's `search()` and exists on **no** server request type. | `tag` optional on insert (both paths); **`filter_tag` removed from the SDK** — it is not a real feature. | P1 | Silent no-ops are worse than errors. |
| 14 | **Search request** | Standalone: `k` required, has `as_of`/`as_of_log_index`, ignores `consistency`. Cluster: `k` defaults to 10, has `consistency`, no `as_of`. | One `SearchRequest`. `k` required (no hidden default). `as_of*` documented standalone-only; `consistency` documented cluster-only. | **P0** | `k` having a default on one path and not the other is a behaviour fork, not just a schema fork. |
| 15 | **Search response** | Standalone `SearchResponse` adds `as_of_*` fields; cluster returns `{results}` only. | One `SearchResponse`; the `as_of_*` fields are `nullable`/omitted. | P1 | Already compatible by omission — just needs the cluster path to use the same struct. |
| 16 | **Score semantics** | `score` = raw squared-L2 distance, **lower is better**, never normalised. Decay changes ranking via `score / decay_factor` but leaves `score` untouched. | Contract states this explicitly on `SearchHit.score`, `MultiSearchHit.score` and in the `Search` tag description. Never called "similarity". | — | Correct today; the contract's job is to stop it drifting. |
| 17 | **Multi-search compatibility** | Same `dim` **and** same `metric` required; index type free. Query length must equal the shared dim. No padding or truncation, ever. | Same, stated in the operation description and enforced by a documented 400. | — | Correct. |
| 18 | **Multi-search error codes** | Unknown collection → **400 standalone / 404 cluster**. Collection without vector config → **400 standalone / 500 cluster**. | 404 for unknown collection on both; 409 (or 400) for a mis-configured collection on both — never 500. | **P0** | Two status-code forks on the same request. |
| 19 | **Multi-search hit identity** | ✅ Every hit carries `collection`. | Same. | — | `MultiSearchHit` is a distinct schema from `SearchHit` for exactly this reason. |
| 20 | **Graph scope** | ✅ Collection-scoped throughout: every handler resolves `collection → ns`; `GET /v1/graph/nodes` without `collection` lists the **default** namespace only. No cross-collection edges exist. | Same. | — | Good. Do not introduce a project-wide graph. |
| 21 | **Graph `kind` encoding** | Raw `u8` on the wire for both `NodeKind` and `EdgeKind`; unknown values → 400. The kernel's discriminants are the public contract. | Contract documents `kind` as `integer (uint8)` with the known discriminants listed in the description, and reserves a string form for v2. | P1 | Internal enum discriminants leaking into a public API. Changing them is now a breaking change. |
| 22 | **`log_index` in responses** | Present on cluster responses, omitted (`skip_serializing_if`) on standalone. | Contract marks it optional everywhere, described as "Raft log index; absent in standalone mode". | — | The omission is the right design; just needs documenting. |
| 23 | **GraphRAG params** | `k` (alias) / `retrieval_k` / `final_k` / `max_graph_candidates` / `max_nodes` / `max_edges` / `graph_weight` / `depth` / `collection`, + `consistency` on cluster. | Same. `k` marked `deprecated: true` with `retrieval_k` as the successor. | — | Verified against `capabilities.rs::graph_rag`, not against the Phase 5.2 report. |
| 24 | **GraphRAG hit fields** | `memory_id`, `record_id`, `score`, `vector_score`, `graph_score`, `final_score`, `node_id`, `graph_distance`, `source`, `metadata`. `score` and `vector_score` always carry the same value. | Same, with `score` marked `deprecated: true`. `graph_score` and `final_score` documented as always numeric; `score`/`vector_score`/`node_id`/`graph_distance` as nullable. | P2 | Two names for one value is exactly the "one `score` field silently changing meaning" trap the brief warns about — solved here by never overloading `score`, only duplicating it. |
| 25 | **GraphRAG provenance** | Verified in code: `vector` (kNN hit, no node), `vector_and_graph` (kNN hit with a node; `graph_distance = 0`), `graph` (BFS-reached, not in kNN; `score`/`vector_score` null). Single merged list sorted by `final_score` DESC, `record_id` ASC, truncated to `final_k`. Graph-only candidates pre-truncated to `max_graph_candidates` sorted by `(distance, record_id)`. | Documented verbatim in the `graphrag` operation description. | — | At high `graph_weight`, graph-only candidates **can** outrank vector hits — stated explicitly. |
| 26 | **Snapshots** | Node-scoped only. Local (`/v1/snapshot/*`) and object-store (`/v1/storage/*`) are two families. Cluster `POST /v1/storage/snapshots/restore` returns **501**. | Both families in the contract, tagged `Snapshots`; the cluster 501 is declared with `x-cluster-status: not_implemented`. | P1 | Per-collection snapshots do not exist and are **not** invented here. |
| 27 | **Object store not configured** | Every `/v1/storage/*` endpoint returns **400** with `"object store not configured — set VALORI_OBJECT_STORE_URL"`. | **501 Not Implemented** (feature not enabled on this deployment) — a 400 wrongly blames the caller. | P1 | Behaviour change; deferred to Phase 2. Contract documents the current 400 and marks the intended 501 in the description. |
| 28 | **Async operations** | Only two things are asynchronous: `POST /v1/namespaces/:name/index` (**202** + poll `GET .../index`) and `POST /v1/ingest?async=true` (job id + poll `GET /v1/ingest/status/:job_id`). There is **no generic operation/`Operation` resource**. | Contract documents exactly these two, and documents that `/v1/operations` is an **audit-log reader** (one entry per committed kernel event), not an async-job registry. | P1 | `/v1/operations/:id` and `/v1/operations/:id/execution` serve **two different id spaces** on one path prefix (`op-N` audit ids vs planner execution ids). Split them in v2. |
| 29 | **Errors** | One shape: `{"error": "<human string>"}`. No code, no details, no request_id. 401/403 have **no body at all**. `models_health` returns `{"error": …}` with **HTTP 200**. | Contract defines `Error` = `{error: string}` — the real shape — and adds `code`, `details`, `request_id` as **optional, `x-status: target`** fields so adding them later is non-breaking. | **P0** for 401/403 (unparseable by clients); P1 for the missing machine codes. |
| 30 | **Error codes** | None. Clients string-match on the message. | A `code` enum reserved in the contract: `validation_error`, `unauthorized`, `forbidden`, `not_found`, `collection_not_found`, `record_not_found`, `dimension_mismatch`, `invalid_metric`, `invalid_index`, `index_build_failed`, `conflict`, `capacity_exceeded`, `not_leader`, `unavailable`, `not_implemented`, `internal_error`. | P1 | Each maps to a real `EngineError`/`KernelError` variant already in the code — nothing invented. |
| 31 | **Status codes** | Emitted: 200, 201, 202, 204, 307, 400, 401, 403, 404, 409, 422, 500, 501, 502, 503, 507. **429 never**. `CapacityExceeded` → **507**, not 429/503. | Contract declares the same set per operation, including 507. | P2 | 507 for a full record pool is unusual but honest. No rate limiting exists, so no 429 is documented. |
| 32 | **Auth** | Single scheme: `Authorization: Bearer <token>`. Token = API key (`vk_…`) **or** the legacy static `VALORI_AUTH_TOKEN`. **If neither is configured, all auth is skipped.** | `components.securitySchemes.bearerAuth` (HTTP bearer). Global `security` applied, with `security: []` on `/health` and `/metrics`. | P1 | "No credentials configured ⇒ wide open" is a deployment footgun that must be documented loudly, not hidden. |
| 33 | **Authorization / scopes** | Three tiers (`read_only < read_write < admin`) derived from `(method, path)` by `required_scope()`, not declared per route. | Contract records the required scope per operation as `x-required-scope`, derived by applying `required_scope()` to each real route. | P1 | Derivation-by-prefix has real gaps — see rows 34-36. |
| 34 | **Scope gap: `/v1/cluster/*` mutations** | `POST /v1/cluster/add-node`, `remove-node`, `snapshot` require only **`read_write`** (they don't match the admin prefix list). | Should be `admin`. | **P0** | Any read-write key can reconfigure cluster membership. |
| 35 | **Scope gap: read-only can't search everything** | `POST /v1/search/multi` and `POST /v1/graphrag` require **`read_write`** because they don't literally end in `/search`. | Both should be `read_only`. | P1 | A read-only key cannot do cross-collection search. |
| 36 | **Per-key collection lock** | `ApiKeyRecord.collection` is stored, returned by `GET /v1/keys`, and **never enforced by either auth guard**. | Either enforce it or stop advertising it. | **P0** | A field that looks like a tenancy boundary and isn't. |
| 37 | **`/health` body** | **Two structurally different objects** — standalone `EngineHealth` vs cluster `{status, leader, dim, embed_*}`. | One `Health` schema: common fields required, mode-specific fields optional; a `mode: "standalone"\|"cluster"` discriminator. | **P0** | One client cannot parse both. |
| 38 | **`/v1/version`** | Returns `text/plain` (a bare version string), not JSON. | Contract declares `text/plain` — the real behaviour. | P2 | Inconsistent with every other endpoint; changing it is breaking. Leave for v2. |
| 39 | **Consistency** | `consistency: linearizable\|local` on cluster search / memory-search / graphrag (default linearizable). `as_of`/`as_of_log_index` on standalone search only. No LSN or read token on records. | `Consistency` enum in the contract, documented as cluster-only; `as_of` documented as standalone-only. | P1 | Two different mechanisms for "which version am I reading" that don't compose. |
| 40 | **Pagination** | Exists on exactly one endpoint: `GET /v1/graph/nodes` (`offset`/`limit`). Everything else is unbounded. | Contract documents the single paginated endpoint and **does not invent** cursors elsewhere. | P1 | `GET /v1/operations` and `GET /v1/timeline` are unbounded reads over the whole event log — a real scaling hazard, recorded as a Phase-2 item. |
| 41 | **Timeouts** | No request timeout field exists anywhere in either router. | Contract has none. | — | Nothing to standardise. `timeout` in the SDK/UI is client-side only. |
| 42 | **`collection` vs `namespace`** | Tree, community and entity-extraction endpoints take **`namespace`**; everything else takes **`collection`**. Same concept. | Contract documents both as-is (renaming is breaking) and marks `namespace` deprecated in favour of `collection`. | P1 | |
| 43 | **Python SDK** | See §I of the phase report and rows 11-14, 44-47. | Generated transport + hand-written ergonomics from this contract. | P1 | |
| 44 | **SDK `collection="default"` default** | Every data method defaults to `collection="default"` and **omits** the field when it equals `"default"`. | No implicit collection. `collection` should be a required argument. | **P0** | A zero-collection project makes every default-argument call 404. The SDK still encodes the pre-Phase-3.3 world. |
| 45 | **SDK `list_contradictions` / `resolve_contradiction`** | Call `{ui_url}/api/contradictions` — a **Next.js UI route**, not the node — which itself calls the non-existent `/v1/memory/meta/list`. | Not in the contract. Already `DeprecationWarning`-flagged. | P1 | Two dead layers stacked. Remove in Phase 2. |
| 46 | **SDK `set_index(index)`** | POSTs `{"index": …}` to `/v1/index/rebuild`, which **ignores** the value and only echoes it back as `effective`. | Not in the contract as a "set index" operation; `POST /v1/index/rebuild` is documented as an operator rebuild-all with no arguments. | P1 | Method name promises something the endpoint does not do. |
| 47 | **SDK vs endpoint coverage** | SDK covers the whole product surface. Deliberately uncovered: `GET /metrics`, `GET /v1/replication/*`. Uncovered by omission: `GET /v1/usage`, `GET /v1/ingest/document` behaviours, daemon routes. | Contract marks `/metrics` and `/v1/replication/*` as `x-sdk: false`. | P2 | |
| 48 | **Embedded `.so` vs remote SDK** | `valoricore/local.py` + `valoricore_ffi.abi3.so` (PyO3, `crates/valori-ffi`) is an **in-process** engine. `remote.py` is HTTP. `factory.py` picks between them. | **The OpenAPI contract governs the remote path only.** Stated in `api/README.md` and in the contract's `info.description`. | P1 | The two paths have never been proven feature-equivalent; the contract must not imply they are. |
| 49 | **TypeScript / UI** | See [`ui-parity.md`](./ui-parity.md). Two duplicated hand-written `types/valori.ts`; `ui/src/lib/valori-client.ts` is dead code with a broken `createCollection`; `/v1/memory/meta/list` is called but does not exist. | Generated types from this contract; hand-written mirrors deleted. | P1 | |
| 50 | **Cluster contract leakage** | `/v1/cluster/status` exposes `term`, `last_log_index`, `last_applied_index` — raw Raft. `/v1/cluster/read-index` is the raw read-index protocol. `307 + Location` exposes leader routing. | Contract includes `status`/`health`/`role`/`proof` (semantic) and **excludes** `read-index` (`INTERNAL`). Raft-native fields stay but are documented as advisory telemetry. | P2 | `add-node`/`remove-node`/`snapshot` are documented as `x-sdk: false` operator actions. |
| 51 | **Control plane vs data plane** | Three separate HTTP surfaces already: `valori-daemon` (local project lifecycle, **no auth**), a separate Cloud API (Next.js, `/api/projects/{id}/...` + `/v1/regions`, `/v1/settings/public`, `/v1/projects/{id}/provision`), and `valori-node` (data). | Keep them separate. `valori-v1.yaml` is **the data plane only**. | P1 | The daemon having zero authentication is a P0 for any non-loopback bind; recorded as a deployment constraint. |
| 52 | **Versioning** | `/v1/` prefix; deprecated aliases carry `Deprecation: true` + `Link` (RFC 8594). API version is not tied to the crate version anywhere. | Contract `info.version: 1.0.0` = **API** version, explicitly decoupled from `CARGO_PKG_VERSION`. Breaking/non-breaking/deprecation rules in `api/README.md`. | — | Good foundation. `COMPATIBILITY.md` already covers the HTTP API section. |

---

## Phase-2 priority order (derived from the P0 rows)

1. **Row 37** — unify `GET /health` into one schema across both paths.
2. **Rows 11, 12, 14** — unify `POST /v1/records` and `POST /v1/search`
   request/response types; honour `request_id` standalone.
3. **Row 18** — unify multi-search error codes (404, never 500).
4. **Rows 34, 36** — fix the two authorization holes (`/v1/cluster/*` scope,
   unenforced per-key collection lock).
5. **Row 29** — always emit a JSON body on 401/403.
6. **Row 44** — remove the implicit `"default"` collection from the Python SDK.

Everything else is P1/P2 and can follow.

---

## Phase API-2 resolution log

The master table above is the **audit as it stood at the end of Phase API-1**
and is deliberately left intact — deleting rows would erase the reasoning that
justified each fix. This section records what Phase API-2 did to each row that
moved. Rows not listed here are unchanged; the full open list, with the reason
each one is still open, lives in
[`contract-conformance.md`](./contract-conformance.md) §4.

| Row | Status | What changed |
|---|---|---|
| 11 | **RESOLVED** | One `api::InsertRecordRequest` / `InsertRecordResponse`, deserialised by both routers. All six public fields (`values`, `collection`, `text`, `metadata`, `tag`, `request_id`) are accepted and honoured on both paths. Guarded by `api_contract.rs::both_routers_share_the_canonical_insert_dto` so a router-private insert type cannot be reintroduced. |
| 12 | **RESOLVED** | Standalone `request_id` dedup implemented in `valori-engine` (`dedup_lookup`/`dedup_record`), reusing the cluster state machine's 16-byte token and bounded-FIFO window. Replay returns the original `record_id`; nothing is written twice. Two wire spellings (16-byte array, 32-char hex with optional UUID dashes) are normalised by `api::RequestId`; anything else is a hard 4xx, never a dropped field. |
| 13 | **RESOLVED** | `tag` is on the canonical insert DTO for both paths. `filter_tag` now raises in the Python SDK instead of silently no-opping. |
| 14 | **RESOLVED** | `k` is required on both paths — the cluster's hidden default of 10 is gone. `as_of`/`as_of_log_index` documented standalone-only, `consistency` cluster-only, both modelled explicitly rather than silently ignored. |
| 15 | **RESOLVED** | Both paths return the same `SearchResponse`; the `as_of_*` fields are omitted rather than the schema differing. |
| 18 | **RESOLVED** | Unknown Collection is **404 `collection_not_found`** on both paths, from the single helper `error_codes::collection_not_found`. The 400/500 "Collection without vector config" fork is unreachable since Phase 3.2. |
| 29 | **RESOLVED** | `error_codes::attach_error_code` is a response-layer middleware on both routers, so every error body — including the bare 401/403 axum used to emit with no body at all — is JSON carrying `{error, code}`. |
| 30 | **RESOLVED** | `valori_engine::ErrorCode`, 16 variants, each mapped from a real `EngineError`/`KernelError` variant. `error` stays for backward compatibility; `code` is the stable field. `api_contract.rs::error_code_enum_matches_the_openapi_contract` diffs the Rust enum against the committed YAML. |
| 34 | **RESOLVED** | `/v1/cluster/add-node`, `/remove-node`, `/snapshot` now require `admin`. Contract's `x-required-scope` updated to match — the implementation was wrong, not the contract. |
| 35 | **RESOLVED** | `/v1/search/multi` and `/v1/graphrag` now require `read_only`. A read-only key can finally run cross-collection and GraphRAG queries. |
| 44 | **RESOLVED** | The implicit `collection="default"` default is gone from every `SyncRemoteClient`/`AsyncRemoteClient` method where the server requires a Collection. |
| 49 | **RESOLVED** | `ui/src/lib/valori-client.ts` (dead, with a `createCollection` that sent no dimension/metric) deleted. Both `ui/src/types/valori.ts` and `ui/studio/src/types/valori.ts` now re-export `@valori/api-types`, generated from this contract by `scripts/generate-api-types.sh`; the fabricated `SearchResponse.state_hash`, `queried_at` and hard-coded `converged: true` are documented at the top of each file as app-derived, not wire, fields. |
| 37 | **OPEN** | `/health` still returns two structurally different objects. Out of Phase API-2's section list; needs its own phase because the UI, CLI wizard, compose healthchecks and MCP server all read it. |
| 36 | **OPEN** | The per-key `collection` lock is still stored, still returned by `GET /v1/keys`, still unenforced. The fix is either a real feature or a breaking response change; neither belongs in a stabilisation phase. |
| 7, 9, 27, 28, 40, 42, 45, 46, 5 | **OPEN (documented)** | Each is a breaking change, a new feature, or both. Reasons in `contract-conformance.md` §4. |

### Items the audit did not have a row for, fixed anyway

* `ui/src/app/api/ingest/route.ts` auto-created the target Collection by
  POSTing `{"name": collection}` with no dimension or metric, and swallowed
  the failure. It now fails fast with a message naming the endpoint to call.
* `valori-cli import` derived the target dimension from `/health.dim` — a
  node-wide value that stopped existing when Collections became the unit of
  vector configuration. Dimension now comes from the source (Qdrant collection
  config, or the first JSONL vector); an existing target Collection is
  validated and never mutated.
* `Daemon::create_collection` forwarded no dimension or metric. It now takes
  both, plus an optional index.
