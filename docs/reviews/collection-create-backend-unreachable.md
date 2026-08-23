# Collection-Create Proxy 404 — Source Investigation

Source-level audit. **No code bug found — the route is already correct.**
No fix was implemented because none is needed; this document explains
precisely why the reported curl result is expected, by-design behavior,
not a defect.

---

## Part 1 — The exact route, every 404 path

`ui/src/app/api/projects/[id]/namespaces/route.ts` (full file, 23 lines):
```ts
export async function GET(req: NextRequest, { params }) {
    const { id } = await params
    return proxyToNode(id, '/v1/namespaces', {}, { req, scope: 'read', fallbackBody: { error: 'backend unreachable' } })
}

export async function POST(req: NextRequest, { params }) {
    const { id } = await params
    const body = await req.text()
    return proxyToNode(id, '/v1/namespaces', { method: 'POST', headers: {...}, body },
        { req, scope: 'write', quotaDimension: 'collections', fallbackBody: { error: 'backend unreachable' } })
}
```
Neither handler itself returns a 404 or `{"error":"not found"}` directly —
both are thin wrappers around `proxyToNode()` (`ui/src/lib/server/
nodeProxy.ts`). Tracing into that shared function, the **only** place that
exact response (`{"error":"not found"}`, HTTP 404) is produced is
`nodeProxy.ts:221-223`:
```ts
if (e instanceof ProjectNotFoundError) {
  return NextResponse.json({ error: 'not found' }, { status: 404 })
}
```
inside `proxyToNode`'s `catch` block. So the question reduces to: what
throws `ProjectNotFoundError` here, and did that happen for this project?

**Note on the reported curl command**: it was a bare `curl -i` — no `-X
POST`, no `-d` body — i.e. a **GET** request. Both `GET` and `POST` on
this route pass `req` into `proxyToNode`, so both go through the identical
authorization path traced below; the finding applies to either verb.

## Part 2 — Project lookup trace, compared with the (already-fixed) detail page

`proxyToNode` (`nodeProxy.ts:142-145`):
```ts
const resolved = opts.req
  ? await resolveProjectAccess(opts.req, projectId, opts.scope ?? 'read')
  : await resolveProjectNodeUrl(projectId)
```
This route passes `req`, so `resolveProjectAccess` runs
(`lib/server/project.ts:270-283`):
```ts
export async function resolveProjectAccess(req, projectId, requiredScope = 'read') {
  const authHeader = req.headers.get('authorization')
  const bearer = authHeader?.match(/^Bearer\s+(.+)$/i)?.[1]
  if (bearer?.startsWith('vlk_')) {
    return resolveProjectNodeUrlByApiKey(bearer, projectId, requiredScope, callerIp)
  }
  return resolveProjectNodeUrl(projectId)
}
```
**The reported curl command carried no `Authorization` header at all** —
so `bearer` is `undefined`, and this falls through to the **session-based**
path, `resolveProjectNodeUrl(projectId)` (`project.ts:140-167`):
```ts
export async function resolveProjectNodeUrl(projectId: string): Promise<ResolvedNode> {
  const supabase = await createClient()
  const { data: { user } } = await supabase.auth.getUser()
  if (!user) {
    throw new ProjectNotFoundError(projectId)
  }
  const { data: project } = await supabase
    .from('projects')
    .select('node_url, status, last_active_at')
    .eq('id', projectId)
    .single()
  if (!project) {
    throw new ProjectNotFoundError(projectId)
  }
  if (!project.node_url || project.status !== 'active') {
    throw new ProjectNotReadyError(projectId)
  }
  ...
}
```
**Root cause, exact line: `project.ts:149-151`.** `curl` from a terminal
carries no Supabase session cookie, so `supabase.auth.getUser()` returns
`user: null`, and this line throws `ProjectNotFoundError` — **before the
project id is ever looked up in the database at all.** This is caught by
`proxyToNode` and turned into exactly the observed response.

**This is deliberate, documented, existing behavior** — the function's own
doc comment (`project.ts:132-138`): *"An empty result is ambiguous by
design: either the project doesn't exist, or the caller isn't a member of
the org that owns it — both cases should read as 'not found' to the
caller, never leaking which."* The same caller-agnostic ambiguity was
confirmed as intentional and preserved (not weakened) in the project-
detail-404 fix a few phases ago — this route already follows the identical
security posture on purpose, and an unauthenticated request is
architecturally indistinguishable from a nonexistent/inaccessible one, by
design, not by accident.

**Comparison with `fetchSafeProjectById()`** (Part 4's specific ask):
`resolveProjectNodeUrl`'s own inline `.select('node_url, status,
last_active_at')` selects three columns, all three of which are inside
`SAFE_PROJECT_COLUMNS` — confirmed **not** the `select('*')` privilege bug
from the project-detail-404 phase. It does not call the shared
`fetchSafeProjectById()` helper — it has its own narrower, independently-
correct inline query (this function needs different fields, and predates
that helper). This is a minor duplication, not a bug, and refactoring it
was judged out of scope: `resolveProjectNodeUrl` is called from many
routes across the app (not just this one), so changing its internals
carries real ripple risk for a function that has no bug to fix — the
"minimal fix" mandate cuts against touching it here.

## Part 3 — This specific project

The claim that project `0ce442c9-96c9-4884-b62f-db816ad90ac5` is real, has
a `node_url`, and is reachable (the control-plane scheduler polling
`https://0ce442c9-....nodes.valori.systems/v1/usage`) is consistent with
everything above — **and irrelevant to the 404**, because the code never
reaches the database lookup for this project id at all when `user` is
null (Part 2's exact line). The project's own state was never rejected;
the caller's identity was never established.

## Part 4 — Confirmed: no `select('*')`, no outdated lookup

Covered in Part 2. This route has no privilege-scoping bug of any kind —
neither the historical `select('*')` issue nor any status/node_url
condition that incorrectly excludes a valid project. `ProjectNotReadyError`
(`status !== 'active'` or missing `node_url`) is the only state-based
rejection in this path, and it maps to 409, not 404 — a different response
than what was observed, ruling it out directly (this project's node_url IS
set, so this branch wouldn't fire even if reached).

## Part 5 — Success path (never reached in the reported test)

For a genuinely authorized caller: `resolveProjectAccess` resolves
`nodeUrl` → `NodeClient.forProject(nodeUrl, projectId)` attaches
`Authorization: Bearer <worker_auth_token>` (via `getWorkerAuthToken`,
service-role Postgres read, never RLS-governed) → for `POST`,
`client.post('/v1/namespaces', JSON.parse(bodyText), init, timeoutMs)` →
real request to `{node_url}/v1/namespaces`, `Content-Type: application/
json`, no client-supplied `Authorization` header ever forwarded (the
worker token always overwrites it — `mergedHeaders()`,
`nodeProxy.ts:56-63`). None of this ran in the reported test — the
exception threw before `NodeClient.forProject` was ever called
(`nodeProxy.ts:142-145`, inside the same `try` block, before line 170).
This matches the report's own observation: *"Control-plane logs show no
collection-create request during the test."*

## Part 6 — Error classification

Confirmed real and worth noting, independent of this specific 404:
`proxyToNode`'s final catch-all (`nodeProxy.ts:253`) —
```ts
return NextResponse.json(opts.fallbackBody ?? { error: 'node unreachable' }, { status: 503 })
```
— collapses every exception that isn't one of the six specifically-typed
error classes (`ApiKeyInvalidError`, `ApiForbiddenError`,
`ProjectNotFoundError`, `ProjectNotReadyError`, `ApiRateLimitedError`,
`ProjectOverQuotaError`) into one generic `{error: 'backend unreachable'}`
/ 503 (this route's own `fallbackBody`). This is broad by design for
customer-facing messaging (never leak internals), but it does mean a real
network timeout, a real 5xx from the node, a JSON-parse failure, and any
unexpected bug would all look identical to the client. **Not implicated in
the reported 404** (that response came from the specifically-typed
`ProjectNotFoundError` branch, not this catch-all) — but flagged as a
genuine, separate observation per the task's own framing, not acted on:
inventing a new error branch without concrete evidence of what a *real*
failure looks like would be guessing, not a source-level finding. The
constructor name of every uncaught error is already logged server-side
(`console.log('[proxyToNode]', { ...timing, error: e.constructor.name })`,
`nodeProxy.ts:207`) — sufficient to diagnose a real occurrence if/when one
happens, just not surfaced to the client (correctly).

## Part 7 — Fix

**None implemented.** No condition in this route incorrectly blocks a
valid project — the 404 is the correct, intended response to an
unauthenticated request, and nothing here needed to change to make the
*actual* collection-creation flow (an authenticated browser session
calling this same route) work. That flow was already fixed in the earlier
G2.4 phase (`useCollections.ts`'s `create()` now sends
`{name, dimension, metric, index?}`, matching the node's real contract) —
this investigation found no additional defect in the path between that fix
and the node.

## Part 8 — Verification

No code changed, so `tsc`/`eslint`/`build` were not re-run for this
investigation — the last known-clean state (from the retry-provisioning
phase) stands unchanged.

**Real production/browser verification: NOT PERFORMED.** Same limitation
as every infrastructure/production step in this engagement — no real
Supabase session or API key available in this session to issue an
authenticated request. Per the task's own instruction, GET was not used as
proof of POST behavior, and no bare curl was used as proof of anything
requiring authentication. The correct verification, for whoever has real
access:
```bash
# With a real vlk_... API key:
curl -i -X POST https://app.valori.systems/api/projects/0ce442c9-96c9-4884-b62f-db816ad90ac5/namespaces \
  -H "Authorization: Bearer vlk_..." \
  -H "Content-Type: application/json" \
  -d '{"name":"test","dimension":768,"metric":"squared_l2"}'
# Expect 200/201, not 404 — the project-scoped key path
# (resolveProjectNodeUrlByApiKey) never hits the null-user branch that
# curl-with-no-auth did.
```
or, preferably, the real authenticated browser "New collection" action
in the dashboard UI (exercises the session-cookie path, the one real
customers actually use).

## Part 9 — No infrastructure touched

Confirmed: nothing in Azure/Docker/Caddy/PROVISIONER/routing was inspected
as a cause and nothing was changed — the entire finding is inside this one
Next.js proxy route's authorization logic, exactly as the task's own
framing already suspected.

---

## FINAL VERDICT

```
VERCEL ROUTE:
PASS

ROUTE MATCH:
PASS

ROUTE INTERNAL LOGIC:
PASS — no bug found; the 404 is the correct, by-design response to an unauthenticated request (project.ts:149-151, "ambiguous by design")

PROJECT LOOKUP:
PASS — resolveProjectNodeUrl's inline select() is already column-scoped (not select('*')); never reached in the reported test because the auth check on line 149 threw first

RUST API REACHED:
NO — by design, for an unauthenticated caller; the exception is thrown before NodeClient.forProject() is ever constructed

COLLECTION CREATE:
PASS (code-level) — the actual flow (authenticated session or a real vlk_ key -> this route -> the node) was already correct after the earlier G2.4 fix; not re-verified live, no session available

AUTH/RLS:
PASS — unchanged, not touched, working exactly as designed

worker_auth_token EXPOSED:
NO

TYPESCRIPT:
N/A — no code changed

ESLINT:
N/A — no code changed

BUILD:
N/A — no code changed

ROOT CAUSE:
ui/src/lib/server/project.ts:149-151 — resolveProjectNodeUrl() throws ProjectNotFoundError whenever supabase.auth.getUser() returns no user, by design ("ambiguous... never leaking which"). The reported curl request carried no Authorization header and no session cookie, so it hit exactly this branch. Not a defect.

MINIMUM FIX:
None required. Recommend re-testing with either a real vlk_... API key (Authorization: Bearer header) or the real authenticated browser session — both take the code path that actually reaches the Rust API and the node.
```

STOP.
