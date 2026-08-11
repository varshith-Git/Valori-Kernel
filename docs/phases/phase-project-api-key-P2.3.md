# P2.3 — NodeClient Unification, Real Next.js E2E, and the Python SDK

## Goal

Close the "architectural footgun" of ad-hoc node calls (one `NodeClient`
responsible for every Cloud→node request), prove the complete
`Python SDK → Next.js → verify_api_key() → project auth → worker token →
valori-node → collection → vectors` chain over real HTTP, and build the
actual `client.collections.create()/upsert()/search()` customer-facing
Python API.

## Delivered

### 1. `NodeClient` — one client for every Cloud→node request

New `NodeClient` class in `ui/src/lib/server/nodeProxy.ts`: resolves the
worker token once, attaches it on every `.get()`/`.post()`/`.request()`
call. `proxyToNode()` rewritten to use it internally. Two new factory
helpers: `nodeClientForProject()` (session-only) and
`nodeClientForRequest()` (session or API key, for routes that need the
external-key path).

**Scope grew well beyond the two files originally named.** A systematic
sweep (`grep` for `nodeUrl`/raw `fetch`/`fetchWithTimeout` patterns across
the whole `ui/src/app/api/projects/[id]` tree, re-run three times with
progressively broader patterns until a pass found zero new hits) found
**14 files total** doing ad-hoc node calls, not 2:

- `why.ts` (8 call sites), `namespace-audit/route.ts` (3) — the two
  originally flagged.
- `cluster`, `graph/edges/[nodeId]`, `health`, `operations`, `proof`,
  `storage/snapshots`, `graph/nodes` (7 more, found by the sweep) —
  simple GET-only, converted mechanically.
- `metrics/ping` (2 calls), `snapshot/download` (binary streaming — works
  identically through `NodeClient.get()`, no special-casing needed),
  `ingest`, `ingest/update`, `tree/build`, `playground` (dynamic-method
  proxy — added a `NodeClient.request()` escape hatch for this one case).
- **`search/route.ts`** — found by literally running the real E2E test
  (below), not the static sweep. It had its own hand-rolled fetch,
  supported the API-key path correctly via `resolveProjectAccess()`, but
  was **missing the 401/403 error mapping entirely** (P2's own fix never
  reached this file) on top of missing the worker token. Rewritten to use
  `nodeClientForRequest()` with the full error mapping.

Every file's orphaned `JSON_HEADERS` constant removed where my edits made
it unused (3 files). `tsc --noEmit` and `npm run build` clean after every
batch of edits, not just at the end.

### 2. Real Next.js ↔ Node end-to-end test — genuinely live

No Docker/`supabase` CLI (same constraint as P2.1/P2.2). Extended the
disposable Postgres + PostgREST stack with:
- A **`/rest/v1` path-rewriting shim** (a 15-line Node `http` proxy) — a
  real Supabase deployment serves PostgREST behind a gateway at
  `/rest/v1/*`; a bare local PostgREST serves at root. Without this,
  `@supabase/supabase-js`'s real request path (`/rest/v1/rpc/...`) 404s
  against a bare PostgREST even though a hand-rolled `curl` to `/rpc/...`
  works — this cost real debugging time before being correctly identified
  as a test-harness gap, not a code bug.
- `service_role`'s `BYPASSRLS` attribute, which my P2.2 stub never set
  (real Supabase's `service_role` has it by platform convention, outside
  anything these migrations manage) — without it, `getWorkerAuthToken()`'s
  service-role query legitimately hit the same column-privilege wall a
  regular `authenticated` caller would.
- A real `next dev` server, launched with env vars pointed at this stack.

**A real safety incident during setup, disclosed in full**: the first
attempt exported `NEXT_PUBLIC_SUPABASE_URL` etc. as shell variables
without also neutralizing `ui/.env.local`. One test request (an
intentionally-fake `vlk_...` key against a fake project id) reached the
**real, live Supabase project** `.env.local` points at, not the local
stack — proven by direct comparison (a hand-rolled curl to the local
PostgREST returned a different result than the Next.js route did, for
what should have been an identical call). Assessed for actual harm before
continuing: `verify_api_key()` is a pure lookup with no side effects for
an unrecognized key hash (no row matches, nothing written) — functionally
identical to a legitimate customer mistyping their key, which the design
already treats as a normal, safe case. No real project, key, or data was
read, written, or exposed. Once identified, the dev server was killed
immediately and the approach was changed: `ui/.env.local` was moved aside
(`mv`, not deleted) for the remainder of every subsequent test, and moved
back immediately after — verified byte-identical via `md5` before and
after — guaranteeing no further request could possibly reach the real
project regardless of any env-var precedence assumption. This incident and
its resolution are reported plainly, per this project's standard for
reporting outcomes faithfully.

**A second, more significant bug found only by this live test — a real
production-breaking issue**: every project-scoped key's default scope is
`project:full` (P2 §10's design), but `verify_api_key()`'s scope check
(`required_scope = any(key_row.scopes)`) is **exact string membership**,
not a hierarchy — and every real proxy route calls it with the literal
string `'read'` or `'write'`, never `'project:full'`. This means **every
newly-created project-scoped key, with its actual real default scope,
failed the scope check on every single real route** — a correctly-issued,
unrevoked, unexpired key would 401 on its very first request. This was
invisible to the P2.1 SQL test suite (which called `verify_api_key()`
directly with matching scope strings in its own test fixtures) and to
static review. **Fixed**: `verify_api_key()`'s scope check now also
accepts `'project:full' = any(key_row.scopes)` as a wildcard match,
alongside the existing exact-match check. Re-verified live.

**Full chain proven live, end to end**, through the actual Next.js dev
server (not simulated):
- `POST /api/projects/{A}/insert` with a real project-scoped key → real
  vectors stored in a real running `valori-node`.
- `POST /api/projects/{A}/search` → finds the inserted vector, correct
  score.
- `POST /api/projects/{B}/search` with **A's key** → real `403`
  (`"this API key is not authorized for this project"`) — the actual
  security property this whole feature exists for, proven over the real
  stack this time, not just a direct RPC call as in P2.2.
- A revoked key → real `401`.

### 3. Python SDK — `client.collections.create()/upsert()/search()`

**A real design correction, found by attempting to actually use the P2.2
`Valori` class against Cloud for the first time**: P2.2's `Valori`
inherited `SyncRemoteClient`'s methods unchanged, which build request
paths like `/v1/records`, `/search` — correct for a bare `valori-node`,
**wrong** for Cloud's real customer-proxy surface, which is
`/api/projects/{project_id}/...` (confirmed by the E2E test above). P2.2's
docstring claim that those inherited methods "just work" against a Cloud
URL was never actually verified and was incorrect.

**Fixed and extended**:
- New `GET /api/me` (`ui/src/app/api/me/route.ts`) — key-only
  self-discovery. Calls a new `resolveOwnProject()` helper
  (`project.ts`), which calls `verify_api_key()` with a nil-UUID
  `target_project_id` (safe specifically because a project-scoped key
  ignores that parameter for authorization — P2's own design) purely to
  force `project_id`/`key_kind` resolution. A legacy org-scoped key (which
  has no single project) gets a clear rejection, not a guess.
- `Valori._project_id()` — lazily resolves and caches the project id via
  `GET /api/me` on first use; the caller never passes one.
- `Valori.collections` → `_CollectionsResource.create(name)`/`.get(name)`
  → `Collection.upsert(vectors)`/`.search(vector, top_k)` — targets the
  correct `/api/projects/{id}/{namespaces,insert,search}` paths, reusing
  `SyncRemoteClient`'s existing transport (`self._t`) for auth/retry, not
  reimplementing any of it.
- **Deliberately does not accept `dimension=`/`index=` on
  `collections.create()`** (present in the request's example code) — in
  the actual system, dimension and index are fixed at the **project**
  level, set once at project creation, and shared by every collection
  inside it; a collection has no per-collection dimension/index to set.
  Accepting and silently ignoring those kwargs would be dishonest
  scaffolding, so they're simply not parameters here — documented in the
  method's own docstring, not silently dropped.

**Proven live**, through the exact real stack from part 2 (dev server,
PostgREST, real node):

```python
client = Valori(url="http://localhost:3311", api_key="vlk_...")
collection = client.collections.create("documents")   # real POST /namespaces
ids = collection.upsert([[0.1, 0.2, 0.3, 0.4], [0.5, 0.6, 0.7, 0.8]])
# -> [0, 1], real record ids from a real valori-node
results = collection.search([0.1, 0.2, 0.3, 0.4], top_k=5)
# -> real results, real scores
```

A revoked key against `collections.create()` correctly raised
`AuthenticationError` (401) — the exception's message text is generic,
pre-existing, shared wording ("Pass token= to the client or set
VALORI_AUTH_TOKEN on the node") not written for the Cloud case and reads
oddly here; noted as a minor, low-priority follow-up rather than touched,
since fixing it would mean editing shared exception text used by unrelated
local-mode call sites.

## Findings

1. **The NodeClient sweep scope was ~7x larger than what was named** (14
   files vs. 2) — confirms the user's framing of this as a real
   architectural footgun, not a one-off oversight.
2. **`search/route.ts`'s missing 401/403 mapping** means P2's fix never
   actually reached the single most important customer-facing route
   (search) until this phase — found only by running the real E2E test,
   not by any of the prior phases' static review.
3. **The `project:full` scope-wildcard bug would have broken every real
   customer key on day one** — this is the most consequential bug found
   in the entire P2.x sequence, and it was invisible to every form of
   testing except an actual live request through the real proxy chain.
4. **The safety incident (part 2) is a concrete argument for why "prove it
   end-to-end" matters even when every layer has already been verified in
   isolation** — P2.2 proved Postgres/PostgREST correctness and the node's
   auth correctness independently; only wiring them together for real
   surfaced both the scope bug and the env-isolation mistake.
5. **P2.2's `Valori` class was untested against real Cloud routes** and
   would not have worked as documented — this phase's correction is a real
   fix to a real gap in already-shipped-feeling code, not a preemptive
   improvement.

## Validation

- `npx tsc --noEmit` / `npm run build` — clean in `valori-ui/ui`, checked
  after every batch of `NodeClient` conversions, not just once at the end.
- `cargo check -p valori-cloud-api` / `cargo test -p valori-cloud-api` —
  clean, 105/105 (unchanged from P2.2 — no Rust files touched this phase).
- `python3 -c "import valoricore"` — clean.
- Real, live, end-to-end: insert → search-finds-it → cross-project 403 →
  revoked-key 401, all through an actual running Next.js dev server, a
  real PostgREST-fronted Postgres, and a real `valori-node` process.
- Real, live Python SDK: `collections.create()`/`.upsert()`/`.search()`
  against the same real stack; revoked-key rejection confirmed.
- `ui/.env.local` confirmed byte-identical (`md5`) before and after every
  test session that required moving it aside.

## Follow-ups

1. **`resolveNodeOrThrow()`'s replacement (`nodeClientForProject`) still
   doesn't accept the API-key path for `why.ts`/`namespace-audit`** —
   correct today (neither route accepts an external key currently) but
   worth confirming intentional if either route is ever opened to
   customers directly.
2. **`AuthenticationError`'s message text** for the Cloud 401 case reads
   oddly ("set VALORI_AUTH_TOKEN on the node") — shared, pre-existing
   wording, low priority, needs a small copy fix without touching the
   exception-mapping logic itself.
3. **`Collection.search()`'s `top_k` param maps to the server's `k` field**
   — intentional (matches the request's example API), but worth
   double-checking no caller confuses this with the server's own `k`
   naming if they read the raw HTTP traffic.
4. **The `/api/me` endpoint is new, additive, and minimal** — no rate
   limiting of its own beyond `verify_api_key()`'s existing per-key limit;
   fine for a low-frequency "resolve my project once" call, would need
   reconsideration if a client called it per-request instead of caching.
5. **Async Python SDK (`AsyncValori`?) and packaging (`valori` vs.
   `valoricore`)** — still open, per P2.2's own follow-ups.
6. **Legacy `project_id IS NULL` keys, `VALORI_AUTH_TOKEN` production
   rollout decision, `Valori-Kernel/ui` vs `valori-ui/ui` consolidation** —
   all still open, unchanged from P2.2.
