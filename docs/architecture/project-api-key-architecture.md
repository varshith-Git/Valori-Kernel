# Project-Scoped API Key Architecture

Status: **P2 — implemented**, per
[`docs/phases/phase-project-api-key-P2.md`](../phases/phase-project-api-key-P2.md).
The design below is preserved as originally written (P1); P2 followed it
closely with a small number of documented, deliberate deviations — each
noted inline as a **P2 note** immediately below the relevant section rather
than by rewriting the original design prose. Source of truth for the
"current state" (pre-P2) claims below is
[`docs/reviews/project-api-key-audit.md`](../reviews/project-api-key-audit.md)
(P0, approved); every fact repeated here traces back to a file:line citation
in that document rather than being re-asserted from memory.

Architectural decision, as directed: **option (a)** — Cloud (the
`valori-ui` control plane) is the authentication and authorization
authority for project-scoped API keys. `valori-node` is **not** made a
Cloud authentication authority in this phase. The data plane must, however,
never trust a client-supplied project id — it must receive an already-
authenticated project context it did not have to derive itself.

---

## 1. Current architecture (recap, see audit for full detail)

```
                    TWO UNRELATED KEY SYSTEMS TODAY

Local (Studio)                          Cloud
─────────────────                       ─────────────────────────────
vk_<64 hex>                             vlk_<8hex>_<48hex>
KeyStore (JSON file,                    api_keys table (Postgres)
per-node-process)                       scoped to ORG_ID, not project_id
scope: ReadOnly|ReadWrite|Admin         scopes: text[] ('read'/'write'/...)
no project_id field at all              no project_id column at all
no expires_at                           no expires_at
revoke = hard delete                    revoked_at = soft flag (kept)
enforced in valori-node's               enforced in verify_api_key() RPC,
auth_guard_v2 middleware                called from Next.js before proxying
                                         to node_url — caller supplies
                                         target_project_id per-request;
                                         RPC only checks org membership
```

The critical fact driving this whole design (audit §18/§20): **Cloud
requests eventually reach the exact same `valori-node` binary** that Studio
uses, via a plain `fetch()` from `valori-ui/ui`'s Next.js proxy layer
(`nodeProxy.ts::proxyToNode`). The node itself has zero Supabase/Cloud
awareness. A second, concrete fact found while preparing this design (not
in the P0 doc, verified against `valori-ui/backend/apps/api/src/provision/dokploy.rs:175-221`):
**the provisioner's deploy env for a new node never sets `VALORI_AUTH_TOKEN`
or any key**. Every Cloud-provisioned node is unauthenticated at the node
level today — `auth_guard_v2` skips its entire check because
`AuthState::has_any_auth()` is false. Anyone who learns a project's
`node_url` can currently talk to it directly, bypassing Cloud's
`verify_api_key()` check entirely. This is an existing gap, not something
this design introduces — but the new architecture should close it as a
byproduct rather than leave it open, since answer to Q12 below hands us a
natural fix.

---

## 2. Chosen architecture

```
Python SDK / curl / application
        │
        │ Authorization: Bearer vri_live_<secret>
        ▼
Valori Cloud API  (Next.js API routes, valori-ui/ui — PRIVATE)
        │
        ▼
API Key Authentication  (Postgres RPC, valori-ui/supabase — PRIVATE)
        │
        ├── hash lookup (sha256, unchanged convention)
        ├── revoked_at IS NULL ?
        ├── expires_at IS NULL OR expires_at > now() ?
        ├── resolve api_key_id
        ├── resolve project_id  ← DIRECTLY from api_keys.project_id,
        │                          never from a client-supplied value
        └── scope satisfies required_scope ?
        │
        ▼
AuthenticatedRequestContext  { project_id, api_key_id, scopes }
        │
        ▼
Project Router  (existing resolveProjectAccess()/proxyToNode(),
                 valori-ui/ui/src/lib/server/ — PRIVATE)
        │
        ├── look up projects.node_url WHERE id = context.project_id
        │   (never WHERE id = <url param> — see §7 below)
        └── attach a per-request, Cloud-issued, short-lived node
            credential (§7) so the node itself can also refuse
            cross-project traffic, defense-in-depth, without
            becoming a Cloud auth authority itself
        │
        ▼
valori-node  (Correct Worker/Node — PUBLIC, valori-kernel)
        │
        ▼
Collections / Vectors
```

Everything above the "Project Router" line lives in `valori-ui` (private).
Everything from `valori-node` down is `Valori-Kernel` (public) and is
**unaware this new system exists** except for one narrow addition
described in §12/§13.

---

## Q1 — Project identity: can `valori_domain::ProjectId` become canonical?

**Yes, for the public-repo side. Not directly for Cloud's existing rows —
needs a backfilled column, not a new type.**

Per the audit (§1/§2), `ProjectId(Uuid)` already exists in `valori-domain`,
is unused in production, and is explicitly documented as the intended
unification point. It should become the type `valori-daemon` (local
projects) mints and stores instead of the current bare
`uuid::Uuid::new_v4().to_string()` (`valori-daemon/src/lib.rs:47-49`) — this
is a small, additive change (the field is already a `String` on the wire;
`ProjectId`'s `Display`/`FromStr` round-trip, per its own doctest, is
already string-compatible) but is **out of scope for this phase** per the
stated stop condition (no Rust code changes yet) and is called out as a
prerequisite for P2.

For Cloud: `public.projects.id` is already a `uuid` (Postgres native type,
`gen_random_uuid()` default). **The bytes are directly compatible with
`ProjectId`'s inner `Uuid` — no transcoding, no new type needed.** The only
question is whether Cloud's `projects.id` and a future local
`ProjectManifest.id: ProjectId` are ever meant to be *the same value* for
the same logical project (e.g. a project that started local and later syncs
to Cloud). Today they are not connected at all (audit §2) and this design
does not need to connect them — a Cloud project's `id` simply *is* its
`ProjectId` value from the moment it's created via `gen_random_uuid()`; no
separate "local↔Cloud project" mapping problem exists because Cloud
projects aren't provisioned from a pre-existing local project id anywhere
today (`backend/apps/api/src/main.rs`'s `provision_project` handler takes
project config, not an existing id). **Do not create a second `ProjectId`
type.** The minimum adapter needed is: (a) Cloud's `uuid` columns are
already wire-compatible with `ProjectId`; (b) when `valori-domain::ProjectId`
is eventually threaded through `valori-daemon` (P2+), no schema change is
needed on the Cloud side to match it, since Cloud was never storing a
different kind of id.

## Q2 — Cloud project mapping

**No new mapping table. `public.projects` (already the source of truth for
`node_url`) becomes directly authoritative for `ProjectId` too, because its
`id` column already *is* one.**

```
Cloud Project row (public.projects)
   id            uuid  ← THIS is the canonical ProjectId, unchanged
   org_id        uuid
   node_url      text  ← already resolved from here today
        │
        ▼
   (no separate mapping needed — id column serves both roles)
```

This directly satisfies "do not invent a new mapping table if an existing
table can safely become the source of truth" — `projects.id` was already
functioning as the de facto project identity everywhere in `valori-ui`
(`node_url` lookups, RLS policies, the existing `verify_api_key(...,
target_project_id uuid, ...)` parameter). Nothing about this design changes
that column's meaning or type. What changes is **which table gets to
authorize access to it** (see Q3) — today `api_keys` has no path back to a
specific `projects.id` at all; that's the actual gap being closed.

## Q3 — API key database model

**Extend the existing `public.api_keys` table — do not create a parallel
table.** New columns, matching the exact conventions already used elsewhere
in this schema (`timestamptz`, `uuid`, `security definer` functions,
`sha256`/`digest` hashing already used by `verify_api_key`, `revoked_at`
already a nullable timestamp on this same table):

```sql
alter table public.api_keys
  add column project_id  uuid references public.projects(id) on delete cascade,
  add column expires_at  timestamptz;
```

Resulting effective shape (existing columns per audit §6b, unchanged,
shown for completeness):

| Column | Type | Status | Note |
|---|---|---|---|
| `id` | `uuid` | existing | PK |
| `org_id` | `uuid` | existing | **retained**, not removed — still needed for billing/plan-limit joins (`create_api_key`'s `max_api_keys` check) and RLS (`org_role()`), but **no longer used for authorization** in `verify_api_key()` |
| `project_id` | `uuid` | **new** | `not null` (see migration note below) — the sole authorization scope going forward |
| `name` | `text` | existing | e.g. "Production", "Default" |
| `key_prefix` | `text` | existing | unique index already exists |
| `key_hash` | `text` | existing | sha256 hex, unchanged |
| `scopes` | `text[]` | existing | see Q10 |
| `created_by` | `uuid` | existing | |
| `created_at` | `timestamptz` | existing | |
| `expires_at` | `timestamptz` | **new** | `null` = never expires |
| `revoked_at` | `timestamptz` | existing | already correct — soft flag, never deleted |
| `last_used_at` | `timestamptz` | existing | |
| `request_count`, `rate_limit_window_*` | existing | unchanged, orthogonal to this design |

**`project_id MUST be authoritative`, exactly as required.** The rewritten
`verify_api_key()` (P2, not written yet) must derive the accessible project
*only* from `key_row.project_id`, never re-deriving it from `org_id`, a
caller-supplied `target_project_id`, or any request/body/URL field. The
**only** legitimate use of a client-supplied project id anywhere in the new
flow is as a value to *compare against* the authenticated
`context.project_id` for a 403 check (Q11) — never as an input to the
authorization decision itself.

**Migration-ordering note** (not a code change, a sequencing fact for P2):
existing rows have `org_id` but no `project_id`, and are org-scoped by
construction — they cannot be safely backfilled to a single project without
either (a) picking one arbitrary project per org (silently narrowing what
that key can do, a behavior change no current holder consented to) or (b)
leaving legacy rows as `project_id null` and having `verify_api_key()`
reject them outright (breaking existing integrations with no warning).
This is a **genuine unresolved decision**, listed in §12 of this document
and in the final report — not something this design can silently pick.

> **P2 note**: implemented exactly as designed — `project_id uuid
> references projects(id) on delete cascade` and `expires_at timestamptz`,
> both nullable, added via `alter table` in
> `supabase/migrations/20260810000000_project_scoped_api_keys.sql`. The
> migration-ordering decision above was resolved per the P2 instructions'
> explicit direction: legacy rows keep `project_id = null` and
> `verify_api_key()` preserves their exact pre-migration org-scoped
> behavior (see the Q11 P2 note) rather than being backfilled, narrowed, or
> rejected. `create_api_key()` was changed so `project_id` is a **required**
> parameter going forward — no code path can mint a new row with
> `project_id = null` after this migration; only rows that already existed
> before it can have that value. The per-project key limit (§16) was
> implemented as specified: `create_api_key()`'s count query changed from
> `where org_id = target_org_id` to `where project_id = p_project_id`.

## Q4 — Key format

**`vri_live_<secret>`, chosen over continuing `vlk_...`, for one concrete
reason: distinguishability from the two formats that already exist.**
Inspected first, per the requirement:

- Local: `vk_<64 hex>` (`valori-node/src/api_keys.rs:296` — `generate_token()`)
- Existing Cloud: `vlk_<8 hex>_<48 hex>` (`prefix = 'vlk_' + gen_random_bytes(4)`,
  `raw_secret = gen_random_bytes(24)`,
  `20260721120100_functions.sql:151-153`)
- Worker heartbeat tokens (a third, unrelated system, same reveal-once/
  hash-only-at-rest contract): `wtk_<64 hex>`
  (`backend/apps/api/src/db/worker_token.rs:14-21`)

A fourth near-identical prefix (`vri_`) risks confusion with `vlk_` in
support conversations and grep searches across a codebase that already has
three `v*_` prefixes. **Recommendation: keep `vlk_` as the prefix family,
not introduce `vri_`**, and change only the *scope* it carries (project
instead of org) — the prefix already means "Valori Cloud key" to anyone
who's seen it, and preserving it means existing tooling that pattern-matches
`vlk_` (any exists — not verified, flagged unknown) keeps working. This is
a deviation from the request's example (`vri_live_...`) and is flagged
explicitly for the final report as a decision needing sign-off, not
silently substituted.

Regardless of which prefix wins, the format keeps the existing, already-
correct construction:

```
<prefix>_<8-hex-identifier>_<48-hex-secret>
     │         │                  │
     │         │                  └─ 24 random bytes (192 bits) —
     │         │                     cryptographically secure
     │         │                     (extensions.gen_random_bytes,
     │         │                     Postgres pgcrypto, already used)
     │         └─ public identifier, safe to log/display, becomes
     │            key_prefix column (already unique-indexed)
     └─ family tag, human-identifiable at a glance
```

Requirements checklist against the existing convention:
- ✅ cryptographically secure randomness — `pgcrypto`'s `gen_random_bytes`,
  already in use, no new dependency
- ✅ identifiable prefix — `key_prefix` column already exists and is shown
  in the UI, unchanged
- ✅ sufficient entropy — 192 bits, already the case, no change needed
- ✅ raw secret shown only once — already the contract
  (`create_api_key`/`rotate_api_key` return `plaintext_key`; no `SELECT` on
  `key_hash` exposed anywhere; `api_keys_public` view already omits it,
  schema.sql:108-111)
- ✅ hash stored server-side — `sha256`, unchanged
- ✅ prefix stored for identification — unchanged
- ✅ no reversible encryption — correctly a one-way hash today, no change

**No new cryptographic mechanism needs inventing.** This section's only
real decision is the prefix naming question flagged above.

> **P2 note**: per the P2 instructions' explicit decision ("Keep the
> existing Valori Cloud `vlk_` prefix... do not introduce `vri_live_` in
> this phase"), the prefix-naming question is resolved: `vlk_` stays. Every
> new key (including project-scoped ones, the atomically-created Default
> key, and legacy org-scoped ones) uses the exact existing format
> (`vlk_<8-hex>_<24-random-bytes-hex>`, sha256 hash) unchanged.

## Q5 — Automatic first API key

Design, matching the requested UX exactly:

```
POST /v1/projects   (existing provisioning entry point,
                      backend/apps/api/src/main.rs:385, or the
                      Next.js action that calls it — exact call site
                      TBD in P2, not re-derived here since main.rs's
                      provision_project signature wasn't opened this
                      pass)
        │
        ▼
projects row created (id = new ProjectId, status='creating')
        │
        ▼
SAME transaction (or immediately-following RPC call, atomic w.r.t.
the project becoming visible to the user) creates one api_keys row:
   project_id = the just-created project's id
   name       = 'Default'
   scopes     = project:full  (see Q10)
   expires_at = null (never, matching "sensible default")
        │
        ▼
Response to the project-creation call includes the plaintext key
ONCE, inline — same reveal-once contract as manual key creation,
but surfaced on the project-creation success screen itself, not
requiring a navigation to Settings → API Keys.
```

Concretely: extend `create_api_key`'s existing `security definer` pattern
into a paired function (e.g. `create_project_with_default_key(...)`, exact
naming a P2 detail) that does the `insert into projects` and the `insert
into api_keys` in one Postgres function, returning both the project row and
the plaintext key in a single round trip — avoiding a window where the
project exists but has no usable credential yet, and avoiding the
`create_api_key` function's existing `org_role in ('owner','admin')` check
needing a second call (the creating user already just proved they can
create a project, which is an equal-or-greater permission).

The Next.js "project created" success page (whichever page that is today —
not re-derived from source this pass) needs a UI addition: display the
plaintext key inline with a copy button, dismiss-once framing, same
component already built for the existing manual key-creation flow (audit
§12 — the `api-keys/actions.ts` reveal-once UI already exists and can be
reused, not rebuilt).

> **P2 note**: found and used the real call site —
> `ui/src/app/dashboard/actions.ts::provisionNewProject`, invoked from
> `ui/src/app/dashboard/CreateProjectDialog.tsx`. Implemented as a single
> new `create_project_with_default_key()` Postgres function (project insert
> + `Default`/`project:full`/never-expiring key insert, one transaction —
> replacing the prior bare `.from('projects').insert()`), called instead of
> the old raw insert. `CreateProjectDialog.tsx` now swaps to a reveal-once
> screen in place immediately after creation succeeds — no extra
> navigation — reusing the same `CopyBtn` component the existing manual
> key-creation UI already uses, exactly as this section predicted. The
> subsequent provisioning HTTP call (deploying the actual node container)
> remains outside the transaction, as this section already anticipated —
> its failure semantics are unchanged from before P2 (project + key rows
> persist, `status` flips to `'error'`, retryable).

## Q6 — Multiple keys

**No architecture change needed beyond Q3's schema — this falls out of it
for free.** `api_keys.project_id` is a plain foreign key, not a unique one;
nothing prevents multiple rows sharing a `project_id`:

```
Project A  (id = 'proj_abc')
 ├── api_keys row: project_id=proj_abc, name='Python Development'
 ├── api_keys row: project_id=proj_abc, name='Production'
 ├── api_keys row: project_id=proj_abc, name='CI/CD'
 └── api_keys row: project_id=proj_abc, name='Testing'
```

Each row independently authenticates to exactly that one `project_id` —
the `ApiKeyId → ProjectId` relationship required by the spec is exactly
what the FK expresses. The existing `max_api_keys` plan-limit check
(`20260722210000_max_api_keys_enforcement.sql`) would need its counting
query changed from "per org" to "per project" (or kept per-org if that's
the intended billing unit — **unresolved decision**, flagged in the final
report) but that's a one-line `WHERE` clause change, not an architecture
change.

## Q7 — Expiration

New column `expires_at timestamptz` (Q3). UI options map directly to a
computed value at creation time, no new enum needed:

| UI option | Computed `expires_at` |
|---|---|
| Never | `null` |
| 30 days | `now() + interval '30 days'` |
| 60 days | `now() + interval '60 days'` |
| 90 days | `now() + interval '90 days'` |
| Custom | caller-supplied `timestamptz`, validated `> now()` |

Authentication check (inside the rewritten `verify_api_key()`, P2):

```sql
if key_row.revoked_at is not null
   or (key_row.expires_at is not null and key_row.expires_at <= now())
then
  return query select false, null::text, null::project_status;
end if;
```

Both conditions collapse to the same `valid=false` response shape the
function already returns for "bad key" (matching the existing documented
"ambiguous-by-design on failure... not leaking existence" convention,
`verify_api_key`'s own comment, audit §7b) — the HTTP layer above it maps
this to `401 Unauthorized` (`resolveProjectAccess` already throws
`ProjectNotFoundError` on any `!result.valid`, which the existing
`proxyToNode` catch block already turns into a 401-equivalent... **actually
verify this maps to 401 not 404** — audit §7b showed `ProjectNotFoundError`
today maps to `404` in `nodeProxy.ts:41`, which is wrong for "key expired"
(a 404 tells an attacker the project doesn't exist; an expired-key case
should be `401`). **This is a required behavior change, not a preservation
of existing behavior** — flagged explicitly, not silently kept as 404.
**Do not silently regenerate expired credentials** — matches the design as
stated; no auto-renewal path exists or should exist here.

> **P2 note**: implemented as a distinct early-return block inside
> `verify_api_key()` (checked right after the hash/revoked lookup, before
> scope/IP/rate-limit checks), collapsed to the same `valid=false` shape as
> every other rejection — matching this section's own prediction exactly.
> The 404-vs-401 bug called out above **was fixed**: `nodeProxy.ts` now
> distinguishes a new `ApiKeyInvalidError` (401 — bad/expired/revoked key)
> from the pre-existing `ProjectNotFoundError` (404, now reserved for the
> session-based path only). See the Q11 P2 note for the full 401/403/404
> mapping.

## Q8 — Revocation

Already correctly modeled on the Cloud side — `revoked_at timestamptz`
already exists and is already a soft flag, never a delete
(`schema.sql:99`, confirmed by audit §6b/§23). **No schema change needed
for revocation itself** — only the authorization check needs to start
consulting it in the *rewritten* `verify_api_key()` (it already does today,
audit §7b line `and revoked_at is null` — this part of the existing
function is already correct and should be preserved as-is, just recombined
with the new `project_id` check). Every subsequent request after
`revoked_at` is set fails the `key_row.id is null or revoked_at is not
null` branch — immediate, no caching layer to invalidate (the RPC is
called fresh on every request, per audit §7b, no client-side or edge
caching of validity exists today).

**Local system note (audit §6a)**: local `KeyStore::revoke()` currently
hard-deletes — no `revoked_at` tombstone. Per Q13 below, this is
**explicitly not being changed in this phase**.

## Q9 — Rotation

Already correctly modeled and already implemented — `rotate_api_key()`
(`20260722200000_api_key_verify_and_rotate.sql:48-93`) already does "same
identity, new secret, old secret invalidated immediately." **This directly
contradicts the requested "old key remains valid until explicitly revoked"
rotation semantics** — the existing function's own comment says *"no grace
period — simplest correct model... 'rotate' meaning 'replace' rather than a
dual-key overlap window."*

Per the explicit instruction ("Do NOT automatically revoke the previous key
when creating another key unless the user explicitly chooses
rotation/revoke-old behavior"), the correct design is: **the existing
`rotate_api_key()` behavior should be renamed/reserved for an explicit
"regenerate this key's secret" action** (still immediate, still no grace
period, still opt-in), and the *primary* multi-key workflow the spec wants
("Create new key → old key remains valid → deploy new key → revoke old
key") is **already fully satisfied by Q6 alone** — create a second
`api_keys` row for the same `project_id`, both valid simultaneously, revoke
the old one's row explicitly when ready. No new database mechanism is
needed for this — it was conflating "rotation" with "multiple independent
keys," and the schema already supports the latter cleanly. This is called
out as a **naming/UX clarification**, not a schema gap.

## Q10 — Scopes

**Recommendation: ship `project:full` as the only value in this phase,
inside the existing `scopes text[]` column** (no schema change — the
column already accepts arbitrary text values; today's values are
`'read'`/`'write'`-shaped strings per `PROGRESS.md`, not independently
re-verified against every call site this pass, audit §9). This satisfies
"do not over-engineer" while leaving the door open for `collections:read` /
`vectors:write`-style granular scopes later purely as *new string values*
the same column can already hold — `verify_api_key()`'s existing
`required_scope = any(key_row.scopes)` check pattern already generalizes to
multiple scope strings per key without a migration. No architecture change
needed to add granular scopes in a future phase; only new scope-string
values and new `required_scope` call sites in the proxy layer.

> **P2 note**: `create_api_key()`'s default scope changed from
> `array['read']` to `array['project:full']` for newly-created rows
> (existing rows/other defaults unaffected). The dashboard's create-key
> dialog no longer exposes a read/write checkbox — every project-scoped key
> gets `project:full` — matching "do not over-engineer scopes in P2." No
> schema change, exactly as this section predicted.

## Q11 — Authenticated request context

```rust
// Illustrative shape — NOT a code change in this phase. This would live
// as a TypeScript type in valori-ui (private), since option (a) keeps
// authentication at the Cloud boundary, not in valori-node.
struct AuthenticatedRequestContext {
    project_id: Uuid,   // from api_keys.project_id, never client input
    api_key_id: Uuid,
    scopes: Vec<String>,
}
```

**The rule, exactly as specified**: after `verify_api_key()` returns
`valid=true`, every downstream step (project router, worker selection, the
eventual proxied request) uses `context.project_id` exclusively.
`req.project_id`/`body.project_id`/`query.project_id`/URL path segments are
**never** trusted as the authorization input.

Where a URL *does* carry a project id for routing purposes (it does today —
every `ui/src/app/api/projects/[id]/*/route.ts` route has `[id]` in its
path, and `resolveProjectAccess(req, projectId, scope)` already takes that
`projectId` as a parameter, audit §8), the correct new behavior is:

```
authenticated_project_id := verify_api_key(...).project_id   // from the RPC now
url_project_id            := route params' [id]

if authenticated_project_id != url_project_id:
    return 403 Forbidden
```

This is the literal fix for "the client must never be able to use a valid
key belonging to Project A to access Project B" — a key for project A,
presented against `/api/projects/B/...`, authenticates successfully (the
key itself is valid) but fails this equality check and is rejected with
403, not silently redirected to project A or allowed into project B.
**This check does not exist today** — today `target_project_id` is passed
straight into `verify_api_key()` as the thing being authorized *against*,
so a key was implicitly "correct" for whatever project id the URL happened
to name, as long as that project belonged to the key's org (the exact bug
this whole design fixes). Under the new model, `verify_api_key()` stops
taking `target_project_id` as an input to trust and instead **returns**
`project_id` as an output the caller must independently compare.

> **P2 note — one deliberate implementation deviation, documented per the
> phase's own "stop and document" rule**: `verify_api_key()`'s parameter
> LIST was kept unchanged (`target_project_id` is still accepted) rather
> than being removed, because the legacy org-scoped path genuinely still
> needs it — there is no other way for a legacy key to know which of its
> org's several projects a request is for. What changed is that
> `target_project_id` is now IGNORED for the authorization decision on a
> project-scoped key (the WHERE clause resolving `node_url`/`status` uses
> `key_row.project_id`, never the parameter) and is only used to interpret
> the result on the legacy path. This achieves the exact same security
> property this section describes (a project-scoped key can never be made
> to resolve to a different project) with a smaller, lower-risk diff than
> changing the RPC's positional signature. The 403 check itself was
> implemented exactly as designed: `ui/src/lib/server/project.ts`'s
> `resolveProjectNodeUrlByApiKey()` now throws a new `ApiForbiddenError`
> when `result.project_id !== projectId` (the URL's id), and
> `nodeProxy.ts` maps that to `403`. Full status mapping after this phase:
> `401` = `ApiKeyInvalidError` (bad/revoked/expired key), `403` =
> `ApiForbiddenError` (valid key, wrong project), `404` =
> `ProjectNotFoundError` (session-based path only, unchanged), `409` =
> `ProjectNotReadyError`, `429` = `ApiRateLimitedError`.

## Q12 — Cloud → Worker routing

Traced from source, not assumed (audit §18/§20, re-confirmed while writing
this document):

```
Cloud API (Next.js route, e.g. ui/src/app/api/projects/[id]/search/route.ts)
        │
        ▼
resolveProjectAccess(req, url_project_id, scope)      [EXISTS TODAY]
        │
        ├── verify_api_key(full_key, url_project_id, scope)  [RPC, EXISTS,
        │                                                      needs rewrite]
        │       → today: returns {valid, node_url, status}
        │       → NEW: returns {valid, node_url, status, project_id}
        │         (project_id now authoritative — see Q11's 403 check,
        │          performed by the route handler right after this call)
        │
        ▼
proxyToNode(url_project_id, path, init, {req, scope})  [EXISTS TODAY]
        │
        ▼
fetch(`${node_url}${path}`, init)          [EXISTS TODAY — plain HTTP,
                                             no auth header of the node's
                                             own kind is currently attached]
        │
        ▼
valori-node  (has ZERO knowledge this request came through Cloud, or
              which project_id Cloud believes it's for)
```

**Where `AuthenticatedRequestContext` must be converted into trusted
routing information**: entirely inside `resolveProjectAccess()`/
`proxyToNode()` in `valori-ui/ui/src/lib/server/` — this is already the
single choke point every per-project route goes through (audit §7b, §18).
No new choke point needs to be introduced; the existing one needs its
internals rewired to trust the RPC's *returned* `project_id`, not the
caller-supplied one, per Q11.

**"Do not send raw API keys from Cloud to `valori-node`" and "do not
expose Cloud API keys to workers unnecessarily"**: already true by
construction in the current code — `proxyToNode`'s `fetch()` call forwards
`init` (the original request's method/body), not the `vlk_...` bearer
token itself (`nodeProxy.ts:33-36`, no `Authorization` header is
constructed or forwarded from the incoming request into the outgoing
`fetch`). This is good and should be preserved, not "fixed" — the node
should never see a Cloud key.

**"If the existing worker protocol needs a project identity, determine the
safest existing mechanism for carrying it"**: the closest existing analog
in this codebase is `backend/apps/api/src/db/worker_token.rs` — a
Cloud-issued, hash-stored, reveal-once, individually-revocable credential
(`wtk_<64hex>`) that "authenticates AND identifies its host in one step...
never a client-supplied field" (the file's own doc comment, verified this
pass). **Recommendation**: apply the identical pattern to close the gap
found in §1 (deployed nodes currently have no `VALORI_AUTH_TOKEN` at all) —
the provisioner (`dokploy.rs`) should set a per-project, Cloud-generated
`VALORI_AUTH_TOKEN` in the node's deploy env at provision time (this env
var already exists and is already read by `valori-node`'s
`config.rs:239`, per audit §7a — it is the existing "legacy static token"
fallback path in `auth_guard_v2`, currently just never populated for Cloud
nodes). The Cloud proxy layer would then attach that **project-specific,
Cloud-internal** token (never the customer's `vlk_...` key) as the
`Authorization` header on its own `fetch()` to `node_url`. This gives
defense-in-depth (a compromised `node_url` alone, without the matching
internal token, is no longer sufficient to reach the node) **without**
making `valori-node` a Cloud authentication authority — the node still
only ever understands its own existing local-token concept (Q13), and
Cloud is simply now a *legitimate holder* of one such token per project,
generated at provision time, never shown to the end customer. This is
flagged as a **recommended P2 addition**, not required to satisfy the
stated architectural decision, but directly closes a real gap found while
researching this design.

> **P2 note — NOT implemented, deliberately deferred.** The `VALORI_AUTH_TOKEN`
> wiring above was explicitly marked "recommended, not required" in this
> design, and the P2 instructions themselves said "implement only the
> minimum required wiring" for this piece. Given the phase's actual scope
> (project-scoped keys as the primary deliverable) and the amount of
> already-required work, this defense-in-depth addition was left
> unimplemented — **every Cloud-provisioned `valori-node` remains
> unauthenticated at the node level after P2**, exactly as the audit found
> it. This is called out explicitly in the P2 phase report's unresolved
> risks, not silently dropped.

## Q13 — Existing local API keys: converge or stay independent?

**(A) — remain independent for now.** Justification, directly from the
audit: local `KeyStore` already correctly enforces "this token can touch
this node" and a node already equals exactly one project by construction
(audit §17) — there is no cross-project leakage risk to fix locally, unlike
Cloud's org-scoping bug. Forcing Cloud's Postgres-based verification model
into `valori-node` would violate the explicit instruction *"Do NOT make
`valori-node` the Cloud authentication authority in this phase"* and would
also violate this repo's own invariant that `valori-kernel`/`valori-node`
must not gain a dependency on Cloud-only infrastructure (Supabase, `org_id`,
etc. — `docs/architecture/layers.md`'s "never do this" list, not
independently re-read this pass but consistent with everything else found).

**"Same conceptual contract" without forced convergence** means: the local
`ApiKeyRecord` should eventually (P2+, not this phase) gain the two fields
Cloud already models correctly — `expires_at` and a real `revoked_at`
tombstone instead of hard-delete — purely because those are good practices
independently of Cloud, not because the two systems need to share code or
a database. They can converge on *shape* without converging on
*implementation*. The one genuine interaction point between the two
systems is Q12's recommendation (Cloud becoming a legitimate holder of a
local-format token) — everything else about `valori-node`'s auth stays
untouched in this phase.

> **P2 note**: confirmed — `crates/valori-node/src/api_keys.rs` and
> `auth_guard_v2` were not touched. Zero changes were made anywhere in the
> `Valori-Kernel` Rust workspace for this phase; `expires_at`/real
> `revoked_at` convergence for the local `ApiKeyRecord` remains deferred to
> a future phase, exactly as this section anticipated.

## Q14 — Python SDK

Audited (`python/valoricore/remote.py`, already read extensively earlier
this session):

- **Current base URL handling**: `base_url: str` constructor arg, stored on
  `_SyncTransport`/`_AsyncTransport`, no concept of "which project" beyond
  "whatever this URL points at."
- **Current authentication**: `_BearerAuth` (`remote.py:39-46`), a
  `requests.auth.AuthBase` subclass setting `Authorization: Bearer {token}`.
  Purely mechanical — no validation, no interpretation of the token's
  contents, no retry-on-401-refresh logic.
- **Current project handling**: none. One `SyncRemoteClient`/
  `AsyncRemoteClient` instance = one implicit project (whatever node
  `base_url` happens to be).
- **Current collection/vector methods**: `create_collection`,
  `list_collections`, `drop_collection`, `insert`, `insert_batch`,
  `search`, `delete`, `soft_delete`, etc. — all call `/v1/...` paths
  directly against `base_url`, no project-scoping parameter anywhere in
  their signatures.
- **Error handling**: `post_rpc()` (`remote.py:98-152`) already maps HTTP
  status → typed exceptions: `404 → NotFoundError`, `401/403 →
  AuthenticationError`, `400/409/413/422 → ValidationError`, `307/503 →
  retry`. **This mapping already fits the new design without change** —
  `401` (expired/revoked/unknown key) and `403` (wrong-project, Q11) are
  already distinct, already-handled cases; no new exception type is
  required, `AuthenticationError` already covers both by HTTP code, and
  callers that want to distinguish "bad key" from "wrong project" can
  already do so via the exception's captured status code (not independently
  verified whether the status code is exposed on the exception object — a
  **P2 detail**, not re-derived from source this pass).
- **Retry behavior**: exponential backoff on `_Retryable`/`ConnectionError`,
  capped by `max_retries` (`post_rpc`, same location) — this already
  correctly does **not** retry on 401/403 (only on 307/503), which is the
  correct behavior for auth failures (retrying an expired key never
  succeeds) and needs no change.

**Design, matching the requested constructor exactly:**

```python
client = Valori(
    url="https://api.valori.systems",
    api_key="vlk_..."   # or vri_..., pending Q4's naming decision
)
```

This maps directly onto the *existing* `SyncRemoteClient(base_url, token=...)`
shape — `url` → `base_url`, `api_key` → `token` (passed straight into the
existing `_BearerAuth`, unchanged). **No new SDK authorization logic is
needed or wanted** — per the explicit instruction, "the SDK should not
implement authorization logic; server-side authentication remains
authoritative." The SDK's only job is to keep sending the bearer token
exactly as it does today; every scope/expiry/revocation/project-match
decision happens server-side, and the SDK surfaces the resulting HTTP
status through its already-correct exception mapping. **The one open
question**: does the SDK need a project id anywhere in its own API surface
(e.g. `Valori(url=..., api_key=..., project_id=...)`), or is the project
now implied entirely by which key was presented (since a key is now
project-scoped by construction, Q3)? Given the new invariant "one key = one
project," **the project id should not need to be a separate SDK
parameter at all** — the key alone determines it server-side. This
simplifies the SDK surface versus today's local-mode
`SyncRemoteClient(base_url)` pattern, not complicates it. Whether `url`
needs to change from "a specific node's address" (local mode) to "the
Cloud API's fixed address, same for every project" (per the example,
`https://api.valori.systems`) is a real SDK-surface decision — it likely
means `Valori(...)` (Cloud) and `SyncRemoteClient(...)` (local/direct-node)
become two distinct constructors/classes rather than one, since their
`url` argument means fundamentally different things (a fixed Cloud API
gateway vs. a specific node's own address). Flagged for the final report,
not resolved silently.

> **P2 note**: implemented as `class Valori(SyncRemoteClient)` in
> `python/valoricore/remote.py` — the "two distinct classes" resolution
> this section predicted, but as a thin subclass reusing 100% of
> `SyncRemoteClient`'s transport/auth/retry/error-mapping machinery, not a
> parallel implementation. Constructor is exactly
> `Valori(url, api_key, max_retries=3, retry_backoff=0.5, timeout=10)`,
> mapping straight onto `base_url`/`token`. No project-id parameter was
> added, confirming this section's reasoning. **Two things deliberately
> NOT done**, flagged for the final report: (1) the requested
> `from valori import Valori` top-level package name — `Valori` lives in
> the existing `valoricore` package (`from valoricore import Valori` /
> `from valoricore.remote import Valori`) since publishing a second PyPI
> package is a distribution decision, not a code change, and wasn't
> something this phase could responsibly invent; (2) the
> `client.collections.create(...)`-shaped sub-resource ergonomics from the
> request's example — `Valori` inherits `SyncRemoteClient`'s existing flat
> method names (`create_collection(...)`, etc.) unchanged, since building a
> new ergonomic wrapper layer is a separate SDK-design effort orthogonal to
> project-scoped auth.

## Q15 — Security requirements

Test matrix, to be implemented in P2 (not written yet, this phase is
design-only):

| Case | Expected |
|---|---|
| Valid key → its own project | 200 (success) |
| Valid key → a different project (Project B, key belongs to A) | **403 Forbidden** (Q11's mismatch check — this is the core fix) |
| Unknown key (no matching `key_hash`) | 401 |
| Revoked key (`revoked_at` set) | 401 |
| Expired key (`expires_at <= now()`) | 401 |
| Wrong scope (key has `project:full` missing a hypothetical future scope) | 403 |
| Key with `project_id = null` (legacy row, pre-migration — see Q3's unresolved decision) | Must be explicitly decided, not left ambiguous — either 401 (safe default) or a documented, time-boxed grandfather window |

**Raw key must never appear in**: database (already true — only
`key_hash` stored, verified schema), logs (not independently verified this
pass whether any `console.log`/tracing call ever logs a full
`Authorization` header — **flagged unknown, needs a grep sweep in P2
before implementation**), telemetry (same, unverified), crash reports
(same, unverified), audit logs (the existing `log_audit_event` call after
key creation, `actions.ts:32-38`, logs `{ name, scopes }` as metadata —
confirmed **not** the raw key, verified this pass), API list responses
(`api_keys_public` view already omits `key_hash`, confirmed, and never
selects a "full key" column since one doesn't persist), `project.json`
(local-only file, has no concept of Cloud keys at all — not applicable).

> **P2 note**: the security test matrix above is implemented as plain,
> runnable SQL at `valori-ui/supabase/tests/project_scoped_api_keys.test.sql`
> — every row of the matrix (own project, wrong project, unknown/revoked/
> expired key, wrong scope, legacy key, no-secrets-in-listings/database)
> has a corresponding assertion. **This file has NOT been executed** — no
> pgTAP/`supabase test db` harness exists in this repo and no live
> Supabase/Postgres instance was available this session to run it against.
> This is reported honestly in the P2 phase report rather than claimed as
> passing. The flagged "grep sweep for raw keys in logs/telemetry" was also
> not performed this phase — still an open item.

## Q16 — Future extensibility

None of the following are implemented in this phase; the schema/flow above
is checked against each to confirm it doesn't foreclose them:

- **Organization API keys**: still representable — a future `api_keys.project_id
  nullable` + `scope_type` discriminator, or a parallel `org_api_keys` table,
  neither blocked by this design (the FK is additive, not exclusive).
- **Service accounts**: `PROGRESS.md` already describes these as "a label
  over API keys, not a parallel credential system" — directly compatible,
  a future `service_account_id` nullable FK column, same pattern already
  used for `created_by`.
- **Personal access tokens**: already a separate table
  (`personal_access_tokens`, audit §10) — unaffected by this design.
- **Collection-scoped keys**: a future `collection text` nullable column on
  `api_keys` (mirroring local `ApiKeyRecord.collection`, audit §6a) — purely
  additive, doesn't change `project_id`'s role as the primary scope.
- **Fine-grained scopes**: Q10 already designs for this (arbitrary strings
  in the existing `text[]` column).
- **IP restrictions, rate limits, usage quotas**: already exist as
  extensions to this same table (`rate_limit_window_*`, audit §22; an
  IP-allowlist mechanism referenced in `PROGRESS.md:234` though not
  independently re-verified this pass) — this design's new columns sit
  alongside them without conflict.
- **Key rotation policies**: Q9 already clarifies rotation is a UX/naming
  concern layered on top of "multiple independent keys per project," not a
  schema concern — a future "policy" (e.g. "require rotation every 90
  days") is an additional table/column referencing `api_keys.id`, not a
  redesign of authentication → authorization → project routing.

None of these require replacing `verify_api_key()`'s core shape
(hash → validity checks → resolve project_id → resolve scopes) — they all
add columns/tables around it.

---

## Summary: exact request flow (for quick reference)

```
1. Client sends: Authorization: Bearer <key>, to https://api.valori.systems/v1/projects/{url_id}/...
2. Next.js route extracts {url_id} from the path, the key from the header
3. resolveProjectAccess() calls verify_api_key(full_key, required_scope)
   — NOTE: no longer passes target_project_id in; the RPC now RETURNS it
4. RPC: hash lookup → revoked? → expired? → scope check → returns
   {valid, project_id, node_url, status}
5. Route handler: if !valid → 401. if project_id != url_id → 403.
6. proxyToNode() looks up node_url (already have it from step 4,
   or re-fetches from projects table keyed by the AUTHENTICATED
   project_id, never url_id)
7. fetch(node_url + path, ...) — no Cloud key forwarded; optionally
   (P2 recommendation, Q12) a Cloud-internal, project-specific
   VALORI_AUTH_TOKEN is attached here
8. valori-node handles the request exactly as it does today — no
   changes to valori-node in this phase beyond the optional Q12 token
```
