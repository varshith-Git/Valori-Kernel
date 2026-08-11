# Valori Cloud Project API — v1

Frozen contract, documenting only endpoints that actually exist in
`valori-ui/ui/src/app/api/projects/[id]/*` and were exercised end-to-end
by the Local Cloud E2E suite (`Valori-Kernel/e2e/cloud/`) against the
real Next.js app, real Postgres/PostgREST, and real `valori-node`
workers. Nothing here is aspirational.

## Base URL

**Production:** `https://app.valori.systems` — not a separate API
subdomain. `api.valori.systems` does not exist and is not planned; the
Cloud dashboard and the API live in the same Next.js deployment.

**Local / self-hosted (this repo's own E2E environment):**
`http://localhost:3311`

The SDK's `url` parameter controls this; see [SDK](#sdk) below.

## Authentication

```http
Authorization: Bearer vlk_<prefix>_<secret>
```

Every request identifies its project **solely** from this key — never
from a `project_id` the caller supplies. `verify_api_key()` (a Postgres
function, called via PostgREST) resolves the key to exactly one project;
the route then compares that resolved project against whatever project
the URL names.

## Project authorization — the core invariant

```
API key  --resolves to-->  authenticated_project
URL path (/api/projects/{id}/...)  -->  requested_project

authenticated_project MUST equal requested_project, or the request is 403.
```

This is checked server-side on every route below — a request is never
authorized just because the caller supplied a project ID in the URL.
Verified directly (not assumed) by `test_projects.py`'s parametrized
403 matrix, run against every route that accepts a `vlk_` key.

## Errors

Actual JSON error shape: `{"error": "<message>"}` (routes proxying a
non-JSON node response, e.g. HTTP 204, return an empty body — see
below).

| Status | Meaning | Observed on |
|---|---|---|
| `401 Unauthorized` | No credentials, or key is invalid/revoked/expired | any key-protected route |
| `403 Forbidden` | Key is valid but bound to a different project than the URL names | any key-protected route |
| `404 Not Found` | No credentials at all reach a session-only route (ambiguous by design — never distinguishes "doesn't exist" from "not yours"), or a genuinely unknown resource | `/search` with zero credentials; unknown project id on session-only routes |
| `409 Conflict` | Key is valid and correctly scoped, but the project isn't `active` yet | any key-protected route, on a project still `status = 'creating'` |
| `429 Too Many Requests` | Rate limit exceeded (see [Rate limiting](#rate-limiting)) | any key-protected route |
| `500 Internal Server Error` | Unhandled failure | not deliberately triggered in this suite; not separately documented beyond "possible" |

`503 Service Unavailable` also occurs — not an application error, but the
Cloud API's own fallback when the underlying `valori-node` is
unreachable (e.g., the worker container is stopped). Body:
`{"error": "backend unreachable"}` or `{"error": "node unreachable"}`.

## Collections

All under `/api/projects/{id}/...`, `vlk_`-key-authenticated (verified —
see [Project isolation](#project-authorization--the-core-invariant)):

| Route | Method | Body | Notes |
|---|---|---|---|
| `/namespaces` | `POST` | `{"name": string}` | Create (idempotent by name) |
| `/namespaces` | `GET` | — | `{"collections": [{"name", "id"}, ...]}` |
| `/namespaces/{name}` | `DELETE` | — | Drops the collection and its data. Returns `204 No Content` — **no JSON body**; a client that assumes every 2xx has a JSON body will break here (this was a real bug, fixed — see `nodeProxy.ts`'s handling of 204/205/304). |

## Vectors

| Route | Method | Body | Notes |
|---|---|---|---|
| `/insert` | `POST` | `{"batch": [[float, ...], ...], "collection"?: string}` | Wraps the node's `batch-insert`; returns `{"ids": [int, ...]}` |
| `/search` | `POST` | `{"query": [float, ...], "k": int, "collection"?: string}` | Returns `{"results": [{"id", "score"}, ...], "queried_at"}` |
| `/delete` | `POST` | `{"id": int, "collection"?: string}` | Deletes one record by id |

No separate "fetch by id" route is exposed at the Cloud API layer today
(the node has `/v1/records/:id`, but no `/api/projects/{id}/...`
equivalent proxies it with `vlk_` key auth) — not documented here as
available, since it isn't.

## API keys

Key lifecycle is **not** exposed through `/api/projects/{id}/*` routes —
it goes through PostgREST RPCs directly (`create_api_key`,
`verify_api_key`) and the `api_keys_public` view, called by the Cloud
dashboard's own server actions using the signed-in user's session, not a
`vlk_` key. There is currently no project-isolation invariant to check
here (it's an org-role check, not a project-match check) — not force-fit
into the matrix above.

- **Default project key**: `create_project_with_default_key()` mints one
  automatically on project creation, scope `project:full`.
- **Create**: `create_api_key(target_org_id, key_name, p_project_id,
  key_scopes?, p_service_account_id?, p_expires_at?)` — org owner/admin
  only. Enforces a real per-project active-key cap (verified: 3).
- **Reveal-once**: the plaintext key is returned exactly once, in
  `create_api_key`'s own response (`plaintext_key`). Never persisted —
  only `key_hash` (SHA-256) is stored, and `api_keys_public` never
  selects `key_hash` at all. Verified directly against the real schema.
- **Revoke**: `PATCH api_keys_public?id=eq.{id}` with
  `{"revoked_at": "<iso8601>"}`.
- **Expiry**: `p_expires_at` on creation; `verify_api_key()` rejects
  (401, same shape as an unrecognized key — doesn't leak *why*) once
  `now() >= expires_at`. Revoked-and-expired rejects the same way.

## Rate limiting

Real, implemented, per-**key** (not per-project or per-org — the counter
column lives on `api_keys`, one window per key) — driven by
`public.plans.rate_limit_per_minute` for the org's subscribed plan.
Verified defaults: `free` = 60/min, `pro` = 600/min, `enterprise` =
6000/min. Applies to every route that authenticates a `vlk_` key via
`resolveProjectAccess()` — confirmed on `/search` and `/insert` in this
suite; the check point (`verify_api_key()`) is shared by every other
key-protected route too, so there's no per-route carve-out.

An org-level IP allowlist (`ip_allowlist_rules`) can additionally reject
requests independent of the rate limit — not exercised by this suite (no
rules configured in the E2E seed).

## SDK

```python
from valoricore import Valori

# Production — defaults to https://app.valori.systems
client = Valori(api_key="vlk_...")

# Local / self-hosted (e.g. this repo's own E2E environment)
client = Valori(url="http://localhost:3311", api_key="vlk_...")

collection = client.collections.create("documents")
collection.upsert([[0.1, 0.2, 0.3, 0.4]])
results = collection.search([0.1, 0.2, 0.3, 0.4], top_k=10)
collection.delete(record_id)
collection.drop()
```

`import valoricore` (and `valoricore.remote`) works without the compiled
Rust FFI extension present — only the embedded `LocalClient`/
`MemoryClient` path (a different, offline mode) actually requires it,
and raises a clear `ImportError` only if you try to construct one of
those without the extension built.
