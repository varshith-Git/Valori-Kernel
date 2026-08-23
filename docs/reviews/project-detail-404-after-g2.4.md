# Project Detail 404 Investigation (post-G2.4)

Source-code audit only. **No code, database, RLS, or project data changed.**
Labels: **FACT** (observed in source, cited), **HYPOTHESIS** (strongly
evidenced, not executable-verified in this session — no live Supabase
access), **DECISION** (this audit's judgment call).

---

## 1. Project list lookup — two separate list queries exist

**A. Sidebar** (`ui/src/app/dashboard/layout.tsx:37-42`, `SidebarServerWrapper`):
```ts
supabase.from('projects').select('id, name, status')
  .neq('status', 'deleted').neq('status', 'archived')
  .order('created_at', { ascending: false })
```
Server Component, session-based `createClient()` (`@/utils/supabase/server`).
Result passed as a prop into `<AppSidebar projects={...} />` — the sidebar
itself (`AppSidebar.tsx`) is `'use client'` but does **not** query Supabase
directly; it only renders what the layout handed it.

**B. `/dashboard` page table** (`ui/src/app/dashboard/page.tsx:40-45`):
```ts
supabase.from('projects').select('*')
  .neq('status', 'deleted').neq('status', 'archived')
  .order('created_at', { ascending: false })
```
Same session-based client. **Selects `*`** — see §3, this one is at real
risk of the same failure mode as the detail pages, just with a softer
failure mode (empty array, not a 404 boundary) — see §9.

## 2. Project detail lookup

`ui/src/app/dashboard/projects/[id]/page.tsx:28`:
```ts
const { data: project } = await supabase.from('projects').select('*').eq('id', id).single()
if (!project) { notFound() }
```
**Identical pattern** — `.select('*').eq('id', id).single()` — appears in
**8 more** per-project pages: `metrics/page.tsx`, `cluster/page.tsx`,
`playground/page.tsx`, `tools/page.tsx`, `proof/page.tsx`,
`snapshots/page.tsx`, `graph/page.tsx`, `operations/page.tsx`. Two
project-scoped pages use a narrower select and are **not** affected:
`archived/page.tsx` route uses a different query, and
`operations/[opId]/page.tsx` selects only `id, status`.

## 3. Exact difference — and the likely root cause

Both A and B (§1) and the detail query (§2) use the **same**
`createClient()` (`@/utils/supabase/server`, session-based, same cookies,
same RLS context). The row-level policy governing all of them is identical
— confirmed in `supabase/migrations/20260721120200_policies.sql:54-56`:
```sql
create policy projects_select on public.projects
  for select to authenticated
  using (public.is_org_member(org_id));
```
One simple, row-level check — no status condition, no column condition.
Since the sidebar's row-restricted query (`id, name, status`) already
proves this user is a member of `org_id = 8614ebf8-99d3-4702-a0fa-993a78933e47`
and RLS lets that row through, **RLS is not what's different** between the
two lookups (see §4).

**The actual difference is a separate, additive privilege layer: column-level
SELECT grants**, introduced by
`supabase/migrations/20260811000000_worker_auth_token.sql`:
```sql
revoke select on public.projects from authenticated;
grant select (
  id, org_id, name, slug, region, status, node_url, replication,
  created_by, created_at, updated_at, last_active_at, dim, index_type,
  max_records, pinned_image
) on public.projects to authenticated;
```
The `authenticated` role's SELECT privilege on `public.projects` is **not**
a blanket table grant — it's an explicit 16-column list that **does not
include `worker_auth_token`**, added specifically so that column (a
Cloud-internal secret proxied to the node, per that migration's own header)
can never be read by a customer session, ever.

**HYPOTHESIS, not executable-verified in this session (no live Supabase
access):** PostgREST does not translate `select=*` into a literal `SELECT *`
left for Postgres to narrow by privilege. It resolves `*` against its own
schema-cache column list and builds an explicit query naming every physical
column — including `worker_auth_token`. Against a role whose SELECT grant
is an explicit column list that omits one of those, Postgres returns
`permission denied for column worker_auth_token`, and the Supabase client
surfaces this as `{ data: null, error: {...} }`.

**Every one of the 9 `select('*')...single()` detail-page call sites
discards `error` entirely** — `const { data: project } = await supabase...`
— so a genuine permission-denied error and a genuinely-nonexistent row are
indistinguishable to this code. Both produce `!project === true`, and
`notFound()` fires either way.

This migration's own comment (reproduced in full because it is directly
relevant, not because it's needed for color): *"Caught only by real
end-to-end testing over PostgREST (not by review, not by the SQL test
suite, which runs as the Postgres superuser and so never exercises
role-based ACLs at all)"* — i.e., the authors of that migration already
knew this exact class of privilege interaction is easy to introduce and
hard to catch without hitting PostgREST as the real `authenticated` role.
These 9 detail pages' `select('*')` calls are exactly that: pre-existing
code, never updated when the column grant narrowed, never caught because
nothing in this repo's test suite exercises a real PostgREST role.

**One inconsistency worth flagging as a data point, not a contradiction**:
if this hypothesis is correct, `/dashboard/page.tsx`'s own `select('*')`
(§1B) should fail the same way — but its failure mode is softer
(`projects = projectsResult.data || []`, silently rendering "No projects
yet" instead of a 404 boundary), so it would present as a *different*
visible symptom (an apparently-empty dashboard) rather than the reported
404. Whether that page is also currently broken wasn't independently
confirmed in this session — but it uses the identical vulnerable query
shape, so it is at the same risk, just with different visible fallout.

## 4. Auth / org behavior

**PASS — not the root cause.** Confirmed the current user is a member of
`org_id = 8614ebf8-99d3-4702-a0fa-993a78933e47` by the sidebar's own
successful query (§1A) — the same RLS-governed table, same role, same
policy. `projects_select`'s `is_org_member(org_id)` check is
identical for both the list and detail lookups; nothing in either query
path applies a different or additional row-level predicate. RLS is not
touched, weakened, or bypassed anywhere in this audit.

## 5. Status filtering

**NOT ROOT CAUSE.** The detail query (§2) has **no status filter at all**
— `.select('*').eq('id', id).single()` matches on `id` only, `status =
'error'` included. Confirmed by direct reading, not inferred. The `status
!== 'active'` branch in `page.tsx:56-65` (which shows "Provisioning
failed. Try creating a new project." for `status === 'error'`) is **only
reached after** `project` has already loaded successfully — it is
downstream of, and irrelevant to, the 404.

## 6. Server-side fetch path

**Confirmed: direct Supabase call from a Next.js Server Component**, not
`/api/...`, not the Rust control plane, not a client-side fetch. The whole
component is `async function ProjectPage(...)`, runs entirely server-side
during SSR/RSC rendering, and calls `createClient()` →
`supabase.from('projects')...` directly. This is exactly why the browser
Network tab showed nothing relevant to provisioning: **this fetch never
appears in the browser's network panel at all** — it's a server-to-Supabase
call that happens before any HTML/RSC payload reaches the browser. Your own
observation ("no visible POST /v1/project/.../provision, therefore
provisioning is not the immediate problem") is correct, and this confirms
exactly why — the failing call isn't a browser-visible request of any kind.

## 7. Cache behavior

**NOT ROOT CAUSE — already mitigated.** `projects/[id]/page.tsx:14`:
```ts
export const dynamic = 'force-dynamic'
```
with a comment explicitly describing the exact prior-bug class this guards
against (a route cached before the project existed, staying stale
indefinitely). This page is not using `unstable_cache`, `revalidate`,
`force-static`, or cached `fetch()` calls — it re-runs the Supabase query
on every request. Ruled out directly, not assumed.

## 8. Why `notFound()` fires

Because `const { data: project } = await supabase.from('projects')
.select('*').eq('id', id).single()` discards the query's `error` entirely
and only checks `if (!project)`. Per §3's hypothesis, a column-privilege
`permission denied` error and a genuinely-missing/inaccessible row both
collapse to `data: null` — the code cannot and does not distinguish them.
`ui/src/app/dashboard/not-found.tsx`'s deliberately ambiguous copy ("We
couldn't find that project, or you don't have access to it.") is,
ironically, textbook-accurate for what's actually happening — just not for
the reason a reader would assume (this isn't an authorization boundary
doing its job; it's an unrelated column-grant error being swallowed and
misread as "not found"). The not-found boundary itself is not the bug and
was not touched.

## 9. Why provisioning was never triggered

`ProjectActions` (which renders the provisioning-retry UI) is instantiated
inside the JSX returned by `ProjectPage` — `page.tsx:47-52` — which is only
reached if execution gets past the `if (!project) { notFound() }` check.
Since `project` is `null`, `notFound()` throws (Next.js's `notFound()`
throws a special error caught by the nearest `not-found` boundary) before
that JSX is ever constructed. **The provisioning action is not reached at
all** — not conditionally hidden, not gated by `status`, simply never
rendered because the page bails out earlier.

## 10. Minimum required fix (NOT implemented — audit only)

Replace `select('*')` with an explicit column list (matching or a subset of
the 16 columns `authenticated` actually has SELECT on — §3) on all 9
affected detail-page call sites, and on `/dashboard/page.tsx`'s list query
for the same reason. Two sub-options, not chosen here:
- **Minimal**: name every column each page actually reads (e.g. `id, name,
  region, replication, status, node_url` for `page.tsx`'s own usage) —
  smallest diff, but a future new column would need auditing at every call
  site again.
- **Centralizing**: one shared "public project fields" constant/type used
  by every one of these ~10 call sites, so a future privilege change (or
  future column addition) only needs updating in one place. Larger diff,
  more durable.

Either way, **the individual call sites must also stop discarding `error`**
— even with a corrected column list, silently swallowing a genuine DB error
as "not found" is the same latent bug waiting for the next privilege change
to trip it again. Surfacing the error (at minimum logging it server-side,
ideally distinguishing "real error" from "genuinely not found/no access" in
what's shown) is part of the minimum fix, not a nice-to-have.

---

## FINAL VERDICT

```
PROJECT EXISTS IN DB:
PASS — confirmed by the sidebar's own successful query against the same table/role/RLS

PROJECT LIST:
PASS — sidebar (id, name, status) and, by the same mechanism, any other restricted-column query

PROJECT DETAIL:
FAIL — select('*') on 9 per-project pages

AUTH/RLS:
PASS — identical projects_select policy (is_org_member) governs both list and detail; not the differentiator

STATUS FILTER:
NOT ROOT CAUSE — detail query has no status predicate at all

CACHE:
NOT ROOT CAUSE — force-dynamic already set, no caching in this page

PROVISIONING BUTTON:
NOT REACHED — notFound() fires before ProjectActions is ever rendered

ROOT CAUSE:
HYPOTHESIS (not executable-verified — no live Supabase access this session): the 20260811000000_worker_auth_token.sql migration narrowed `authenticated`'s SELECT grant on public.projects to an explicit 16-column list excluding worker_auth_token. PostgREST resolves `select=*` to an explicit column list from its schema cache (including worker_auth_token), which Postgres then rejects for a role without SELECT on that column — surfacing as a query error. Every affected page discards that error (`const { data } = await supabase...`, error un-destructured) and treats data:null identically to "row doesn't exist", so notFound() fires.

MINIMUM FIX:
Replace select('*') with an explicit column list (a subset of the 16 columns authenticated can actually read) on all 9 per-project detail pages plus /dashboard/page.tsx's list query, and stop discarding the query's `error` at each of those call sites. Not implemented in this audit.
```

---

## Implementation status (Phase G2.3.2-D)

**Implemented.** No database/RLS/privilege change — the fix is entirely on
the query side, matching the audit's own conclusion that the grant itself
is correct and must not be weakened.

### Shared selection helper

`ui/src/lib/server/project.ts` gained:
- `SAFE_PROJECT_COLUMNS` — the exact 16-column string from
  `20260811000000_worker_auth_token.sql`'s re-grant, treated as the
  authoritative source (not re-derived independently).
- `SafeProjectRow` — a TypeScript interface for that exact shape.
- `fetchSafeProjectById(supabase, id)` — the one place every per-project
  detail page now fetches its row. Destructures `{ data, error }`
  internally, logs `error.code` (never `error.message` — same reasoning
  the file already uses for `last_active_at` write failures: a message can
  echo back query fragments that don't belong in server logs) via
  `console.error`, and returns `null` on **either** a real error or a
  genuinely-missing/inaccessible row — every caller keeps its existing
  `if (!project) notFound()` shape unchanged. The ambiguity between "does
  not exist" and "exists but errored" is preserved on the client side on
  purpose (matching `not-found.tsx`'s intentionally ambiguous copy); the
  fix makes that ambiguity logged and intentional, not silent and
  accidental.

A shared function was chosen over repeating `{ data, error }` handling
inline at each of the 9 call sites — same DRY reasoning the codebase
already applies elsewhere (e.g. `resolveProjectAccess`), and it means a
future page can't reintroduce this exact bug by copy-pasting `select('*')`
again.

### Affected pages — all updated

| Page | Change |
|---|---|
| `projects/[id]/page.tsx` | `fetchSafeProjectById` |
| `projects/[id]/metrics/page.tsx` | `fetchSafeProjectById` |
| `projects/[id]/cluster/page.tsx` | `fetchSafeProjectById` |
| `projects/[id]/playground/page.tsx` | `fetchSafeProjectById` |
| `projects/[id]/tools/page.tsx` | `fetchSafeProjectById` |
| `projects/[id]/proof/page.tsx` | `fetchSafeProjectById` + one type-narrowing fix (`node_url ?? ''` — see below) |
| `projects/[id]/snapshots/page.tsx` | `fetchSafeProjectById` |
| `projects/[id]/graph/page.tsx` | `fetchSafeProjectById` |
| `projects/[id]/operations/page.tsx` | `fetchSafeProjectById` |
| `dashboard/page.tsx` | `.select('*')` → `.select(SAFE_PROJECT_COLUMNS)` on the list query, plus explicit `error.code` logging that wasn't there before |

**Field-usage audit, confirmed before implementing**: every field these 9
pages actually read (`id`, `name`, `region`, `replication`, `org_id`,
`status`, `node_url`) is inside `SAFE_PROJECT_COLUMNS`. No page needed a
column outside the 16 — no compromise was required.

**One real (and correct) type error surfaced by giving `project` a real
type for the first time**: `proof/page.tsx` passes `project.node_url` into
`<ProofView nodeUrl={string}>`, which requires a non-null `string`, but
`SafeProjectRow.node_url` is `string | null` (the DB column has no NOT
NULL constraint). Fixed with `project.node_url ?? ''`, since this branch
only renders when `status === 'active'` — an invariant `main.rs`'s
`mark_project_active` already upholds (it sets `node_url` and flips status
together) — so the fallback is a type-narrowing safety net, not an
expected runtime path. This is a genuine, if minor, type-safety
improvement this fix incidentally surfaced: every one of these 9 pages was
previously working with an implicitly-`any` `project` object.

### Backward compatibility

- `status === 'error'` still reaches the exact same "Provisioning failed.
  Try creating a new project." branch in `page.tsx` — that conditional is
  unchanged, only reached now because the row actually loads.
- No project status was changed, no re-provisioning was triggered, no RLS
  policy or grant was touched.

### Regression search

```
$ grep -rn "from('projects')" ui/src --include="*.tsx" --include="*.ts" -A1 | grep -B1 "select('\*')"
(no output)
```
**Zero remaining `projects...select('*')` call sites.** Other tables still
use `select('*')` (`subscriptions`, `login_history`, `ip_allowlist_rules`,
`api_keys_public`, `service_accounts`, and one each in
`settings/developer/page.tsx` / `settings/team/page.tsx`) — deliberately
**not** touched; none of them have a column-scoped grant like
`projects` does, and the task was explicit about not blindly modifying
unrelated table queries. Worth a dedicated audit later if any of those
tables ever gets a similarly-scoped grant.

### Verification

```
$ npx tsc --noEmit          → clean, 0 errors
$ npx eslint <every file>   → clean, 0 warnings/errors
$ npm run build             → ✓ Compiled successfully in 8.2s
```

**Live authenticated browser verification: NOT PERFORMED.** Same
limitation as every infrastructure-touching phase in this engagement — no
real Supabase session/credentials available in this session, and the dev
server's own auth + global session middleware blocks reaching any
`/dashboard/*` route without one (confirmed directly in Phase G2.4's own
verification attempt). Static verification (types, lint, build, and direct
reading of the final diff against the confirmed root cause) is what
backs the claims above — not a live render. A human operator with a real
session should confirm:
1. `/dashboard/projects/6d88266a-47f6-42bd-a358-58ea0ae6e557` loads the
   Project Workspace, no 404.
2. Sidebar still shows the project (unaffected by this change — it never
   used `select('*')`).
3. Every project subroute (`/metrics`, `/cluster`, `/playground`,
   `/tools`, `/proof`, `/snapshots`, `/graph`, `/operations`) loads.
4. No `worker_auth_token` value appears anywhere in the rendered
   page/RSC payload/dev tools — structurally guaranteed by
   `SAFE_PROJECT_COLUMNS` never naming that column, not just by absence of
   a UI element that happens not to show it.
5. A genuinely nonexistent project id still renders the Cloud not-found
   page (unaffected — `fetchSafeProjectById` still returns `null`, same as
   before, for a truly missing row).
6. `ProjectActions` (the provisioning-retry UI) now renders, since the
   page itself loads — **do not click it**, per this phase's own scope
   limit.

---

## FINAL VERDICT (implementation)

```
PROJECT DETAIL:
PASS (code-level — tsc/eslint/build clean; live render NOT PERFORMED, no session available)

PROJECT LIST:
PASS (code-level, same caveat)

WORKER_AUTH_TOKEN EXPOSED:
NO — SAFE_PROJECT_COLUMNS structurally excludes it; the grant itself was never touched

RLS UNCHANGED:
PASS — no migration, no policy, no grant modified

STATUS ERROR PAGE PRESERVED:
PASS — the status !== 'active' branch and its per-status copy are byte-for-byte unchanged

NOT-FOUND SECURITY:
PASS — not-found.tsx untouched; ambiguity between "missing" and "errored" preserved on the client, now logged server-side instead of silently lost

ERROR HANDLING:
PASS — every affected call site now surfaces its query error (via fetchSafeProjectById or inline for the list query), logged server-side only (error.code, never error.message), never shown to the client

TYPESCRIPT:
PASS

ESLINT:
PASS

BUILD:
PASS

REMAINING projects.select('*'):
none

FILES CHANGED:
ui/src/lib/server/project.ts
ui/src/app/dashboard/page.tsx
ui/src/app/dashboard/projects/[id]/page.tsx
ui/src/app/dashboard/projects/[id]/metrics/page.tsx
ui/src/app/dashboard/projects/[id]/cluster/page.tsx
ui/src/app/dashboard/projects/[id]/playground/page.tsx
ui/src/app/dashboard/projects/[id]/tools/page.tsx
ui/src/app/dashboard/projects/[id]/proof/page.tsx
ui/src/app/dashboard/projects/[id]/snapshots/page.tsx
ui/src/app/dashboard/projects/[id]/graph/page.tsx
ui/src/app/dashboard/projects/[id]/operations/page.tsx
```

STOP.
