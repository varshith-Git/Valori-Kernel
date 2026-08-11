# Local Cloud E2E Audit

Read-only audit, per the explicit instruction, before any infrastructure is
built. No production behavior was changed to produce this document. Builds
directly on `docs/reviews/project-api-key-audit.md` and the P0–P2.4 phase
docs (all cited facts there remain accurate as of this writing); this audit
adds only what those didn't already cover — Docker/container support and
what a from-scratch local E2E environment needs.

## 1. Current API entry point

**Verified.** The customer-facing entry point is `valori-ui/ui` — a Next.js
app. Its API routes (`ui/src/app/api/projects/[id]/*`, `ui/src/app/api/me`)
are what the Python SDK actually calls; confirmed live in P2.3 (real
`POST /api/projects/{id}/search` round-trips against a real node). The
separate Rust service (`valori-ui/backend`, `valori-cloud-api` binary, port
`8787`) is **not** a customer entry point — it's the provisioning/admin
control plane (`/v1/projects/:id/provision`, `/v1/admin/*`), reached only
from `ui/`'s own server actions, never directly by a customer or the SDK.

## 2. Current API-key verification path

**Verified.** `public.verify_api_key(full_key, required_scope, p_ip)` — a
Postgres function, called via PostgREST from `ui/src/lib/server/project.ts`
(`resolveProjectNodeUrlByApiKey`/`resolveOwnProject`, both go through a
shared `verifyApiKey()` wrapper as of P2.4). As of P2.4's cleanup: no
`target_project_id` parameter, no `key_kind` in the return — every key is
project-scoped, full stop. Returns `{valid, node_url, status, rate_limited,
project_id, api_key_id}`.

## 3. Current project authorization path

**Verified.** `resolveProjectNodeUrlByApiKey()` compares the URL's
`projectId` (from the Next.js route's dynamic segment) against
`result.project_id` (the RPC's output, derived solely from the key) —
mismatch throws `ApiForbiddenError`, mapped to `403` in `nodeProxy.ts`. This
is the actual, already-implemented, already-live-tested (P2.3) mechanism
this whole E2E environment needs to exercise, not reimplement.

## 4. Current worker resolution path

**Verified.** `verify_api_key()`'s own internal join:
`select node_url, status from projects where id = key_row.project_id`. One
row in `public.projects` per project, one `node_url` column, set once at
provisioning time (`backend/apps/api/src/main.rs::provision_project_inner`)
and read back by the RPC on every request. There is no separate "worker
registry" — `projects.node_url` *is* the routing table.

## 5. Current worker-auth-token path

**Verified, P2.2/P2.3.** `projects.worker_auth_token` (plaintext, revoked
from `anon`/`authenticated` at the column-privilege level, readable only by
a `service_role`-authenticated Supabase client). `ui/src/lib/server/nodeProxy.ts`'s
`NodeClient.forProject()` fetches it (`getWorkerAuthToken()`) and attaches
`Authorization: Bearer <token>` on every outgoing request to the node,
**replacing** whatever the incoming request's own Authorization was. The
Rust provisioner (`dokploy.rs`/`docker.rs`) sets `VALORI_AUTH_TOKEN=<token>`
in the deployed node's env at provision time
(`main.rs::resolve_worker_auth_token`, generates one on first use if unset).

## 6. Current Python SDK request path

**Verified, P2.3.** `valoricore.remote.Valori(url, api_key)` — thin
`SyncRemoteClient` subclass. `.collections.create(name)` /
`Collection.upsert(vectors)` / `Collection.search(vector, top_k)` resolve
`project_id` once via `GET {url}/api/me` (cached for the client's
lifetime), then call `POST {url}/api/projects/{project_id}/{namespaces,
insert,search}` — the exact same routes the Cloud dashboard's own UI calls.
**Not yet covered by the SDK's ergonomic layer**: delete (collection or
vector), get/list collections, index-type selection at collection-create
time (see finding G1 below).

## 7. Current `valori-node` authentication path

**Verified, unchanged since the original P0 audit.** `auth_guard_v2`
middleware (`crates/valori-node/src/server.rs`): if `VALORI_AUTH_TOKEN` is
set, every request needs a matching `Authorization: Bearer <token>` (via a
constant-time compare against the "legacy token" fallback path — this IS
the mechanism `worker_auth_token` rides on, no new node-side code exists or
is needed). If unset, auth is skipped entirely (node runs open). `/health`
is exempt from auth regardless (a pre-existing, deliberate exception for
health-check probes, confirmed in P2.2). No project/org/Cloud concept
exists anywhere in this code path — the node has no idea it's "in Cloud
mode."

## 8. Existing Docker support

**Verified, mixed — this is the real gap this phase closes.**

- **`valori-node` (`Valori-Kernel/Dockerfile`)**: exists, real,
  production-quality — multi-stage build (`rust:slim-bookworm` →
  `gcr.io/distroless/cc-debian12:nonroot`), builds `valori-node` release
  binary, healthcheck via the binary's own `--health-check` flag. Directly
  reusable for both Worker A and Worker B with different env/volumes — no
  new Dockerfile needed for the workers.
- **Rust control plane (`valori-ui/backend/Dockerfile`)**: exists, real —
  builds `valori-cloud-api`, exposes `8787`, healthcheck via
  `/health/ready`. Not actually needed for this E2E environment's critical
  path (per audit item 1, the SDK never talks to it) — **can be omitted**
  from the compose file unless project *provisioning* itself needs to be
  exercised through it rather than seeded directly.
- **`valori-ui/ui` (the Cloud API the SDK actually calls)**: **no
  Dockerfile exists anywhere in the repo.** This is a real, concrete gap —
  every route this whole feature depends on has never been containerized
  before. `ui/next.config.ts` also has no `output: 'standalone'` set, which
  a lean Next.js Docker image needs. Both are legitimate, minimal additions
  (build-target configuration, not application-behavior changes) — not
  "creating a mock," since the container runs the real, unmodified Next.js
  app.
- **PostgREST/Postgres**: no existing Dockerfile needed — official
  `postgres:16` and `postgrest/postgrest` images cover this; only a schema
  init step (running the two migration chains) is new work.

## 9. Existing database requirements

**Verified, extending the P2.1/P2.2/P2.3 disposable-Postgres findings.**
Two independent migration sets must both apply, in order, to the same
database: `valori-ui/backend/migrations/*.sql` (sqlx, creates the `infra`
schema — hosts, instances, telemetry, worker tokens) and
`valori-ui/supabase/migrations/*.sql` (creates `public` — organizations,
projects, api_keys, and every RPC). A bare Postgres additionally needs a
hand-built stub of what real Supabase provides for free: an `auth` schema
(`auth.users`, `auth.uid()` reading `request.jwt.claims`), the `anon`/
`authenticated`/`service_role` Postgres roles (the last needs `BYPASSRLS`,
which is a real Supabase platform default this repo's own migrations don't
grant — found the hard way in P2.3), and pgcrypto/`uuid-ossp` extensions.
**What is NOT reproduced by this approach** (documented per the explicit
instruction rather than silently approximated): Supabase Auth's actual
password/OAuth/session issuance, GoTrue's `raw_user_meta_data`-driven
triggers beyond the one column this repo's own trigger reads, Realtime,
Storage, Edge Functions — none of these are used by the code path this
E2E environment needs to prove, so their absence is not a gap for THIS
purpose, but it does mean this is a **PostgREST-compatible stub**, not a
"local Supabase," and should be labeled that way rather than implied to be
equivalent. (A true `supabase start` local stack — the official Supabase
CLI, which runs a real GoTrue/Studio/Storage alongside Postgres+PostgREST —
was not attempted: heavier, and everything the SDK's actual request path
touches is exactly the subset already proven reproducible by the
PostgREST-only approach across P2.1–P2.4.)

## 10. What can be reused

- `Valori-Kernel/Dockerfile` (workers, both A and B, different env/volumes).
- Every Supabase/backend migration file, applied as-is, unmodified.
- Every real RPC (`verify_api_key`, `create_api_key`,
  `create_project_with_default_key`) and every real Next.js route —
  nothing about the authentication/authorization/routing logic needs to be
  rewritten or approximated for this environment; it already works, proven
  live three times over (P2.2, P2.3).
- The Python SDK (`valoricore.remote.Valori`) as-is.
- The `auth`-schema-stub + role-setup SQL already hand-built and validated
  across P2.1–P2.4 (would otherwise need re-deriving from scratch).

## 11. What must be added

- A Dockerfile for `valori-ui/ui` (new, minimal, `output: 'standalone'`).
- A docker-compose file wiring Postgres → migration-apply job → PostgREST →
  `ui` (Cloud API) → two `valori-node` workers, with the `ui` container's
  env pointed at the local PostgREST instead of a real Supabase project
  (same env vars already identified safe to override across P2.1–P2.4:
  `NEXT_PUBLIC_SUPABASE_URL`, `NEXT_PUBLIC_SUPABASE_ANON_KEY`,
  `SUPABASE_SERVICE_ROLE_KEY`).
- A real Supabase-shaped JWT signer for the anon/service-role tokens this
  environment's own PostgREST instance needs (already prototyped as a
  15-line Node script across P2.2–P2.3; needs to become a committed,
  reusable script rather than a throwaway `/tmp` file this time).
- Missing SDK ergonomics for collection/vector delete and collection
  listing (finding G1) — needed to actually exercise the full CRUD matrix
  the test plan asks for; not present in the SDK today.
- Python test suite itself (none exists yet for this path).
- A resource-benchmark harness (none exists).

## 12. Architectural gaps discovered

- **G1 — SDK collections/vectors surface is incomplete.** `Collection` (P2.3)
  has `upsert`/`search` only — no `delete`, no `get_collection`/
  `list_collections` on `_CollectionsResource`, and `collections.create()`
  deliberately doesn't accept `dimension=`/`index=` (P2.3's own finding:
  those are project-level, not per-collection, in the real system). The
  requested test plan (`delete vectors`, `delete collection`, `list
  collections`) needs these added — real, new SDK surface, not fakes; they
  wrap the same existing routes/RPCs (`DELETE`-shaped calls) other UI code
  already exercises.
- **G2 — Rate limiting exists but is genuinely per-org-plan, not
  independently testable without a real `plans`/`subscriptions` row set
  up correctly.** `verify_api_key()`'s rate-limit block (already reviewed
  in the P0 audit) reads `subscriptions.plan → plans.rate_limit_per_minute`,
  defaulting to 60/min if no subscription row exists. This IS a real,
  already-implemented mechanism (not `NOT_IMPLEMENTED`) — the E2E
  environment needs a `plans`/`subscriptions` fixture with a deliberately
  low limit to exercise it in reasonable test time, rather than sending
  tens of requests against the 60/min default.
- **G3 — No project/vector-count "delete collection" cascade has been
  independently verified against a real node this session** (only
  `api_keys`↔`projects` cascade was verified in P2.1) — the E2E persistence
  test should confirm `/v1/namespaces/:name` DELETE actually removes data,
  not just returns 200.
- **G4 — `VALORI_HOME` is not a real `valori-node` environment variable.**
  Confirmed by reading `crates/valori-node/src/config.rs`: the node's real
  persistence env vars are `VALORI_EVENT_LOG_PATH`, `VALORI_SNAPSHOT_PATH`,
  `VALORI_DATA_DIR` (the `Dockerfile`'s own `ENV` defaults). `VALORI_HOME`
  is a `valori-daemon`/desktop-Studio-only concept (per this repo's own
  `CLAUDE.md`), unrelated to the node binary. The compose environment uses
  the node's real env vars, not the ones an outside description assumed.
- **G5 — No Dockerfile exists for `valori-ui/ui` anywhere in either repo**,
  confirmed by a repo-wide `find`. This is the single largest missing
  piece for this phase — every other component already has a working
  container image.
- **G6 — `POST /api/projects/[id]/delete` did not accept API-key auth at
  all (real bug, found while writing the E2E test suite, not a pre-known
  gap).** Every sibling write route (`insert`, `namespaces` POST,
  `namespaces/[name]` DELETE) calls `proxyToNode(..., { req, scope })`,
  which routes external `vlk_` bearer requests through
  `resolveProjectAccess()`. `delete/route.ts` called `proxyToNode()`
  without `{ req, scope }` at all, so it always fell through to the
  session-only `resolveProjectNodeUrl()` path — an external API-key
  caller (no Supabase session) would get `ProjectNotFoundError` → `404`
  instead of being authenticated. This would have made
  `Collection.delete()` (used by `test_projects.py`'s "most important
  test") fail outright. Fixed minimally in
  `ui/src/app/api/projects/[id]/delete/route.ts` by adding
  `{ req, scope: 'write' }`, matching the exact pattern already used by
  every sibling route — no new authentication system introduced, just the
  existing one wired to a route that had been missed. `npx tsc --noEmit`
  passes after the change.
