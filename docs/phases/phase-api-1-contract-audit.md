# Phase API-1 — Deep API Audit + Canonical REST/OpenAPI v1 Contract

## Goal

Establish **one** canonical, machine-readable public REST contract for Valori
before any SDK generation or gRPC work begins — derived from a code-first
audit of every externally reachable route, not from prior phase reports or
prose documentation.

## Delivered

| File | Contents |
|---|---|
| `api/openapi/valori-v1.yaml` | The canonical contract. OpenAPI 3.0.3, 75 paths, 79 operations, 101 reusable schemas, 13 reusable responses, 5 reusable parameters, 1 security scheme. Every operation carries `x-status`, `x-required-scope`, and — where a mode does not implement it — `x-cluster-status` / `x-standalone-status`. |
| `api/README.md` | What the contract is, why it exists, the control-plane/data-plane boundary, ownership, validation commands, breaking/non-breaking/deprecation rules, the embedded-`.so`-vs-remote-SDK separation, and the (documented, not implemented) future SDK-generation and gRPC architecture. |
| `docs/api/api-inventory.md` | Every externally reachable route across all four routers, with a full matrix (method, path, domain, standalone, cluster, auth, async, public-SDK), per-endpoint request/response detail, the real error shape, the real status-code set, an explicit standalone/cluster parity matrix, and the actual idempotency, consistency, pagination and timeout behaviour. |
| `docs/api/current-vs-target.md` | 52-row severity-ranked gap analysis, plus a P0-first Phase-2 priority order. |
| `docs/api/ui-parity.md` | TypeScript/UI drift: hand-written type mirrors vs the implementation, dead code, and a call to a route that does not exist. |
| `CHANGELOG.md` | `[Unreleased]` entry for the `api/` deliverable. |
| `docs/phases/README.md` | Status-table row. |

**No production code was changed.** No SDK was generated. No `.proto` file
was created. No route, request struct, response struct or status code was
modified.

### Audit method

Route inventory came from the four router builders read on disk:
`crates/valori-node/src/server.rs::build_router_with_keys`,
`crates/valori-node/src/cluster_server.rs::build_cluster_router_with_keys`,
`crates/valori-node/src/cluster_api.rs::cluster_router`, and
`crates/valori-daemon/src/http.rs::router`. Semantics came from the handler
bodies, the shared `crates/valori-node/src/routes/*` modules,
`crates/valori-engine/src/error.rs`, `crates/valori-engine/src/index_manager.rs`,
`crates/valori-node/src/api_keys.rs`, `crates/valori-node/src/capabilities.rs`
(for the real GraphRAG output shape) and `crates/valori-domain/src/project.rs`
(for the real `Metric` / `IndexKind` enums). `crates/valori-node/tests/route_parity.rs`
supplied the machine-verified path/method parity baseline.

## Findings

### P0 — clients get wrong or unusable behaviour

1. **`GET /health` returns two structurally different objects.** Standalone
   returns `EngineHealth` (capacity pools, persistence mode, collection
   count); cluster returns `{status, leader, dim, embed_*}`. Only `status` is
   common, and its value set differs too (`no-leader` is cluster-only). One
   client cannot parse both.
2. **`POST /v1/records` has two different request *and* response types.**
   Standalone accepts `{values, collection, text}`; cluster accepts
   `{values, collection, metadata, tag, request_id}`. Serde silently ignores
   unknown fields on both, so a client sending the union gets no error — only
   missing behaviour.
3. **Idempotency is a no-op in standalone mode.** `request_id` exists only on
   the cluster insert type. The Python SDK sends `request_id` on *every*
   insert (`uuid4().bytes` by default), so users reasonably believe inserts
   are deduplicated when standalone they are not.
4. **`POST /v1/search` `k` is required standalone but defaults to 10 on
   cluster.** A behaviour fork, not merely a schema fork.
5. **`POST /v1/search/multi` returns different status codes per mode.**
   Unknown collection → 400 standalone / 404 cluster. Collection with no
   vector config → 400 standalone / **500** cluster.
6. **`/v1/cluster/add-node`, `/remove-node`, `/snapshot` require only
   `read_write`, not `admin`.** `required_scope()` derives admin from the
   path prefixes `/v1/keys`, `/v1/snapshot`, `/v1/storage`, `/v1/replication`
   — `/v1/cluster` is not among them. Any read-write key can reconfigure
   cluster membership.
7. **`ApiKeyRecord.collection` is never enforced.** The per-key collection
   lock is stored, returned by `GET /v1/keys`, and checked by neither auth
   guard. A field that looks like a tenancy boundary and is not one.
8. **401 and 403 responses have no body at all** — the auth middleware
   returns a bare `StatusCode`. Every other error path emits
   `{"error": …}`, so a client's error parser breaks precisely on auth
   failures.
9. **The Python SDK still assumes an implicit `"default"` collection.**
   Every data method defaults to `collection="default"` and *omits* the field
   when it equals `"default"`. Since Phase 3.3 a new project has zero
   collections, so every default-argument call 404s.

### P1 — real drift requiring a client workaround

10. **`GET /v1/memory/meta/list` is called by the UI but does not exist.**
    `ui/src/app/api/contradictions/route.ts` fetches it and swallows the
    failure, so the contradiction review queue silently returns an empty list
    forever. The SDK's deprecated `list_contradictions()` reaches the same
    dead path via that UI route.
11. **`ui/src/lib/valori-client.ts` is dead code** (nothing imports it) and
    contains the worst drift in the tree: `createCollection(name)` posts
    `{name}` with no `dimension`/`metric` (guaranteed 400), `search()` posts
    to the deprecated `/search` alias, and both `converged: true` and
    `queried_at` are fabricated client-side.
12. **`valori-daemon`'s collection proxy is broken the same way.**
    `Daemon::create_collection` posts `{"name": collection}` to the node's
    `/v1/namespaces` with no dimension or metric — `POST /v1/projects/:name/collections`
    cannot succeed against a current node.
13. **Two enums for one concept.** `IndexKind` (`brute|hnsw|ivf|bq|auto`) is
    accepted at Collection creation; `IndexBuildRequest.type`
    (`hnsw|ivf|bq|null`) at the lifecycle endpoint. `auto` is valid in one and
    not the other.
14. **`IndexStatus.status` is derived, not the raw `IndexState`.** `ready` and
    `retiring` are practically unobservable, and `status` is not
    round-trippable back into the state machine.
15. **The 501 "cluster does not support ANN" branch is unreachable.** Both
    `IndexOps` impls have returned `supports_ann_builds() == true` since Phase
    4.3; `cluster_unsupported_response()` is dead.
16. **Object-store endpoints return 400 when the store is unconfigured**,
    blaming the caller for a deployment setting. Should be 501.
17. **`collection` vs `namespace`.** Tree, community and entity-extraction
    endpoints take `namespace`; every other endpoint takes `collection`, for
    the same concept.
18. **`/v1/operations/:id` and `/v1/operations/:id/execution` serve two
    different id spaces** on one path prefix — `op-N` audit-log ids versus
    planner execution ids returned by `/v1/ingest`.
19. **`valori-daemon` has no authentication middleware whatsoever.** Safe only
    because it is meant to bind loopback; nothing enforces that.
20. **Receipts are not durable.** `/v1/proof/receipt` reads an in-memory
    256-entry ring buffer; a restart empties it.
21. **`SDK.set_index(index)`** POSTs to `/v1/index/rebuild`, which ignores the
    value and echoes it back as `effective`. The method name promises
    something the endpoint does not do.
22. **`SDK.search(filter_tag=…)` and `SDK.insert(tag=…)` are silent no-ops
    standalone** — neither field exists on the standalone request struct.

### P2 — cosmetic / structural

23. Only **one** endpoint in the entire API paginates (`GET /v1/graph/nodes`).
    `GET /v1/operations` and `GET /v1/timeline` are unbounded reads over the
    whole event log.
24. **No request-timeout field exists anywhere.** `timeout` in the SDK and UI
    is client-side only.
25. **No `Idempotency-Key` header support.** The only idempotency mechanism is
    the body-level `request_id` / `request_ids`.
26. **429 is never emitted** — there is no rate limiting in the node.
    `KernelError::CapacityExceeded` maps to **507 Insufficient Storage**.
27. `GET /v1/version` returns `text/plain`, unlike every other endpoint.
28. `GET /v1/models/health` returns `{"error": …}` with **HTTP 200** on failure.
29. Graph `kind` is a raw `u8` on the wire — the kernel's `NodeKind`/`EdgeKind`
    discriminants are now part of the public contract.
30. Two byte-identical hand-written `types/valori.ts` files
    (`ui/src/types/` and `ui/studio/src/types/`).
31. `GET /v1/timeline` and `GET /v1/proof/event-log` read **shard 0's log
    only** in a sharded deployment (previously known gap, re-confirmed).

### Things the audit confirmed are already correct

* **Zero-collection projects work.** `Engine::create_collection` has no
  config-free overload; a new deployment lists `{"collections": []}`.
* **`"default"` has no special meaning.** `parse_collection_config` has no
  name-based exception — a Collection literally named `default` needs an
  explicit dimension and metric like any other.
* **Project does not own vector configuration.** `create_project()` hard-writes
  `dim: None, index: None`; the manifest fields survive only as documented
  legacy. (The `CreateProjectRequest` fields that accept them are vestigial and
  should be removed.)
* **Score semantics are honest.** `score` is the raw squared-L2 distance
  everywhere; decay changes ranking via `score / decay_factor` without
  mutating `score`.
* **Multi-search compatibility is right.** Same dimension *and* metric
  required; index types may differ; the query length must match; vectors are
  never padded, truncated or transformed.
* **Cross-collection hits keep their identity** — `MultiSearchHit` always
  carries `collection`.
* **Graph is Collection-scoped**, with no cross-collection edges, and
  `GET /v1/graph/nodes` without a `collection` lists only the default
  namespace (a tenant leak closed in Phase R2).
* **GraphRAG never overloads `score`.** It duplicates it into `vector_score`
  and adds `graph_score`/`final_score` alongside, so no single field silently
  changes meaning.
* **Route path/method parity is mechanically enforced** by
  `tests/route_parity.rs` with explicit, justified allowlists.

## Validation

**N/A for behaviour — this phase changed no code.** `cargo test` was
deliberately not re-run because nothing under `crates/` was touched.

Contract validation actually performed:

| Check | Tool | Result |
|---|---|---|
| YAML syntax | `python3 -c "import yaml; yaml.safe_load(...)"` | pass |
| Full OpenAPI 3.0.3 validation — structure, `$ref` resolution, examples validated against their schemas | `npx @redocly/cli@latest lint` | **valid**, 0 errors, 4 intentional warnings |
| Unique `operationId` | script over the parsed document | 79 operations, 0 duplicates, 0 missing |
| Tag hygiene | script | 0 used-but-undeclared, 0 declared-but-unused |
| `$ref` resolution | script | 0 unresolved |
| Unused components | script + redocly | 1 (`IndexSpec`, deliberate — marked `x-status: target`) |
| `x-status` coverage | script | 79/79 operations marked; all `current` |
| Route coverage | manual diff of contract paths against the two router builders | every documented path exists in code; every intentionally excluded path is listed in `api/README.md` §3 with a reason |

The four accepted redocly warnings are documented in `api/README.md` §5:
two `localhost` server URLs (Valori is self-hosted, there is no canonical
public host), `GET /health` having no 4xx (it is unauthenticated and returns
only 200/503), and the deliberately reserved `IndexSpec` schema.

Manual smoke test — reproduce the validation locally:

```bash
python3 -c "import yaml; yaml.safe_load(open('api/openapi/valori-v1.yaml'))"
npx --yes @redocly/cli@latest lint api/openapi/valori-v1.yaml
```

## Follow-ups

### Phase API-2 — make the implementation conform (P0 first)

1. Unify `GET /health` into one schema across both paths (finding 1).
2. Unify `POST /v1/records` request and response types; honour `request_id`
   standalone (findings 2–3).
3. Make `k` required on both search paths (finding 4).
4. Unify multi-search error codes — 404 for unknown collection, never 500
   (finding 5).
5. Add `/v1/cluster` to the admin prefix list; enforce or remove the per-key
   collection lock (findings 6–7).
6. Emit a JSON `Error` body on 401/403 (finding 8).
7. Remove the implicit `"default"` collection from the Python SDK
   (finding 9).

### Phase API-3 — client generation

8. Generate `@valori/api-types` from the contract; delete both hand-written
   `types/valori.ts` files; delete or rewrite `ui/src/lib/valori-client.ts`
   (findings 10–11, `docs/api/ui-parity.md`).
9. Fix `Daemon::create_collection` to forward dimension/metric (finding 12).
10. Decide the fate of `/v1/memory/meta/list`: implement the prefix scan, or
    remove the contradiction queue and the two deprecated SDK methods.

### Deferred, tracked in `docs/api/current-vs-target.md`

* Converge `IndexKind`/`IndexType` (13); expose raw generation state (14);
  delete the dead 501 branch (15).
* Change object-store-unconfigured to 501 (16).
* Deprecate `namespace` in favour of `collection` (17).
* Split the two `/v1/operations/:id` id spaces (18) — v2.
* Add authentication (or an enforced loopback bind) to `valori-daemon` (19).
* Durable receipts (20).
* Pagination for `/v1/operations` and `/v1/timeline` (23).
* Machine-readable `Error.code` — the enum is already reserved in the contract
  and maps 1:1 onto existing `EngineError`/`KernelError` variants.

### Explicitly NOT in scope for any of the above

No `.proto` files, no gRPC, no SDK generation, and no separate OpenAPI
document for the Cloud or daemon control planes were created in this phase.
The Cloud and daemon contracts, when written, are separate documents — see
`api/README.md` §3.
