# P2.2 — Project API Key End-to-End & Isolation, and Worker Authentication

## Goal

Prove the full request chain (`Python SDK → HTTPS → Cloud API → Supabase
verification → project authorization → node proxy → valori-node`) with two
real, isolated projects, over real HTTP — not just SQL-level review. And
implement `VALORI_AUTH_TOKEN` (Cloud → worker defense-in-depth), closing the
"every Cloud-provisioned node is unauthenticated at the node level" gap the
audit found and P2/P2.1 both left open.

## Delivered

### `VALORI_AUTH_TOKEN` — Cloud → Worker authentication

**Schema** (`supabase/migrations/20260811000000_worker_auth_token.sql`):
`projects.worker_auth_token text` — a Cloud-internal secret, never a
customer-facing credential, generated once per project (backfilled for
existing rows; new rows via `main.rs::resolve_worker_auth_token`, lazily
generated on first provision if somehow still unset). Stored as
**plaintext**, not hashed — unlike `api_keys.key_hash`, Cloud must be able
to read the exact value back to (a) put it in a newly-deployed node's env
and (b) attach it as `Authorization` on every proxied request; it never
verifies a value someone else generated, so hashing would serve no
purpose here.

**Rust control plane** (`valori-ui/backend`):
- `provision::traits::DeployRequest` gained `worker_auth_token: String`.
- `provision::dokploy::DokployProvisioner::deploy()` and
  `provision::docker::DockerProvisioner::build_env()` both add
  `VALORI_AUTH_TOKEN=<token>` to the deployed node's env — this is the
  **same existing env var `valori-node`'s `auth_guard_v2` already reads**
  (`crates/valori-node/src/config.rs:239` in the kernel repo, its existing
  "legacy static token" fallback path); **zero kernel-repo code changed**.
- `main.rs::resolve_worker_auth_token()` — reads (or generates+persists on
  first use) the token for a project, called from `provision_project_inner`
  before building `DeployRequest`.
- `instance_lifecycle.rs`'s blue/green shadow-deploy path reuses the
  project's existing token (not a fresh one — a restart shouldn't require
  every already-provisioned instance to simultaneously rotate).
- `MockProvisioner`/`DockerProvisioner` test fixtures updated with a dummy
  token; a new assertion (`env_carries_the_same_valori_vars_the_dokploy_path_sets`)
  confirms `VALORI_AUTH_TOKEN` is present in the built env.

**Next.js Cloud API** (`valori-ui/ui`):
- New `ui/src/utils/supabase/service.ts` — a service-role Supabase client
  (bypasses RLS/column grants), used for exactly one thing: reading
  `worker_auth_token`. Explicitly documented as the one deliberate
  exception to "every Supabase access goes through the user's session or
  `verify_api_key()`."
- `ui/src/lib/server/project.ts::getWorkerAuthToken(projectId)` — fetches
  it after authorization has already succeeded.
- `ui/src/lib/server/nodeProxy.ts::proxyToNode()` — attaches it as
  `Authorization: Bearer <worker_auth_token>` on the outgoing fetch to
  `node_url`, **replacing** whatever the incoming request's own
  Authorization was (a customer `vlk_...` key or a dashboard session
  cookie never reaches the node — only this Cloud-internal secret does).

### Real end-to-end verification (not simulated)

No Docker daemon or `supabase` CLI was available (same constraint as
P2.1), so the disposable Postgres 16 from P2.1 was reused and extended
with **PostgREST** (`brew install postgrest`) in front of it — this is
the actual component `@supabase/supabase-js` talks to over HTTP in real
Supabase, so requests through it exercise the real REST/RPC/RLS/GRANT
layer, not just plpgsql logic in a psql session.

1. **Worker-token enforcement, against the real `valori-node` binary**
   (`target/debug/valori-node`, already built): started with
   `VALORI_AUTH_TOKEN=test-worker-secret-123`. Live `curl` results:
   - No `Authorization` header → `401`
   - Wrong token → `401`
   - Correct token → `200`, and a real insert (`POST /v1/records`)
     returned a genuine BLAKE3 receipt; search with the wrong token was
     rejected before reaching the handler.
   - (`/health` alone is deliberately unauthenticated — a pre-existing,
     unrelated kernel convention for load-balancer probes, not a gap
     introduced or found this phase.)

2. **Cross-project isolation, over real HTTP through PostgREST** — two
   real project fixtures (A, B) and a real project-scoped key for A.
   `POST /rpc/verify_api_key` with A's key and B's `target_project_id`:
   returns `valid:true` but `project_id`/`node_url` **still resolve to A**,
   never B — the exact security property this whole feature exists to
   guarantee, now proven over an actual HTTP round-trip rather than a
   psql session. Unknown key and revoked key both correctly return
   `valid:false` over the same real HTTP path.

3. **Atomic project+key creation, over real HTTP with a real signed
   JWT** — hand-signed an HS256 JWT (`role: authenticated`, real `sub`
   claim) and called `POST /rpc/create_project_with_default_key` through
   PostgREST: created a real project row and its Default key in one
   round trip, returned the plaintext key, exactly matching what
   `ui/src/app/dashboard/actions.ts` does in production.

### A real, serious bug found only by this live testing

**The `worker_auth_token` column-privacy fix in the original migration
did not work.** `REVOKE SELECT (worker_auth_token) ... FROM authenticated`
does **not** override the pre-existing broad `GRANT SELECT ON TABLE
projects TO authenticated` from `policies.sql` — Postgres ORs table-level
and column-level SELECT privileges together rather than layering them, a
well-known but easy-to-miss gotcha. A live `curl` as an `authenticated`
JWT against a project row with a known `worker_auth_token` value **returned
the real secret in plain JSON.** This would have been a real production
security hole: any signed-in user could read any project's internal
worker token by simply requesting that column, and use it to bypass
Cloud's authorization entirely by hitting the node directly.

**Fixed** the same way this schema's own `api_keys`/`key_hash` already
protects its secret: revoke the broad table-level `SELECT`/`UPDATE`
entirely, then re-grant on an explicit column list that excludes
`worker_auth_token`. The `UPDATE` column list was derived from actually
grepping every real `.from('projects').update(...)` call site in `ui/`
(`name`, `status`, `last_active_at`) rather than guessed. Re-tested live
after the fix: `worker_auth_token` selection now correctly returns
`42501 permission denied`; the legitimate columns (rename, status,
activity ping) still work — confirmed via a plain `PATCH` (204, matches
the real code's un-chained `.update()` calls) and separately confirmed
that `Prefer: return=representation` (equivalent to chaining `.select()`)
would have required broader privileges — verified the real TS call sites
never do that, so this narrowing doesn't break anything in production.

## Findings

1. **The privilege-narrowing bug above is the headline finding.** It is
   exactly the class of bug this phase's real-infrastructure testing was
   for — SQL review, the P2.1 test suite (which runs as the Postgres
   superuser and so never exercises role-based ACLs at all), and even
   `tsc`/`cargo test` would all have missed it. Only a live role-scoped
   HTTP request against PostgREST caught it.
2. **`auth.uid()`'s stub needed correcting mid-phase.** PostgREST 16 sets
   JWT claims via the `request.jwt.claims` JSON GUC, not the older
   per-claim `request.jwt.claim.<name>` GUCs my P2.1 stub assumed —
   without noticing this, `create_project_with_default_key()`'s
   `org_role()` check silently saw no user and rejected every call. Fixed
   the stub to read both (JSON GUC preferred, old-style as fallback) —
   this is purely a **test-fixture** correction, not a change to any real
   Supabase-hosted function, since real Supabase's `auth.uid()` is managed
   by the platform, not by this repo's migrations.
3. **The `VALORI_AUTH_TOKEN` wiring required zero `valori-kernel`/`valori-node`
   code changes**, exactly as the P1 architecture predicted — the env var
   and the middleware that enforces it already existed; this phase only
   had to start *populating* it.

## Validation

- `cargo check -p valori-cloud-api` — clean.
- `cargo test -p valori-cloud-api` — **105/105 pass**, including the new
  `VALORI_AUTH_TOKEN` env-var assertion.
- `npx tsc --noEmit` + `npm run build` — clean in `valori-ui/ui`.
- Real `valori-node` binary, live `curl`: worker-token enforcement proven
  (401/401/200 for no-token/wrong-token/correct-token; real insert +
  receipt with the correct token).
- Real PostgREST + disposable Postgres, live `curl`/signed JWT: cross-project
  isolation proven, revoked-key rejection proven, atomic project+key
  creation proven, **and** the worker-token privacy leak found and its fix
  verified — all over actual HTTP, not psql sessions.

## Follow-ups

1. **Full Next.js dev server integration** — this phase proved the
   Postgres/PostgREST layer and the `valori-node` layer independently over
   real HTTP, and the Rust/TS code paths compile and unit-test cleanly,
   but did **not** run the actual Next.js server end-to-end (would need
   real environment variables and either a fake or real Supabase Auth
   flow for session-based routes). The API-key path (the one this whole
   feature is about) doesn't need a session at all, so this remains the
   most valuable next increment if still wanted.
2. **Python SDK against a live Cloud endpoint** — still not done; needs
   the Next.js server from #1 running.
3. **`resolveNodeOrThrow()`'s callers** (`why.ts`, `namespace-audit`) do
   **not** get the worker-token attached — only `proxyToNode()` was
   updated. These routes make their own direct `fetch()` calls to
   `node_url` and were out of this phase's scope; flagged as a real,
   specific gap, not silently left inconsistent.
4. **Legacy `project_id IS NULL` keys** — still unresolved, per P1/P2/P2.1.
5. **`Valori-Kernel/ui` vs `valori-ui/ui` consolidation** — still not
   scheduled.
