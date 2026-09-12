# Phase SH2 — Cloud control-plane integration for shared Free projects

**Status:**
```
READY FOR STAGING VALIDATION
NOT READY FOR PRODUCTION FREE-TIER ONBOARDING
```
A fourth review round audited the `SECURITY DEFINER` RPC added in the
third correction and the `last_active_at` activity-tracking column; a
fifth found its `search_path` was more permissive than a `SECURITY
DEFINER` function should use. All fixed and live-verified — see "Fourth
correction" and "Fifth correction" below. A full diff review across both
repositories followed (no issues beyond one stale test-count claim in
`backend/README.md`, corrected), then all three commits below. Not yet
pushed or opened as a PR. Before production: the real HTTP flow (now
achievable in staging, where a trusted cert exists), one paid dedicated
deployment, and corrupt-project quarantine (Phase 4A) must all land
first.

**Commit SHAs (staging must deploy all three together — they are one
logical change split across two repositories, not independently
deployable):**

| Repository | Commit | Message |
|---|---|---|
| `Valori-Kernel` | `570259d` | `test(shared-hosting): expand isolation and recovery verification` (SH-H1) |
| `Valori-Kernel` | `08cd497` | `docs(shared-hosting): document cloud integration and staging gates` (SH2 docs) |
| `valori-ui` | `c5ce2e9` | `feat(cloud): provision free projects on shared workers` (SH2 implementation) |

Not pushed to any remote and no PR opened as of these SHAs — both are
local commits on each repo's default branch (`main` for `Valori-Kernel`,
`master` for `valori-ui`). Push and PR creation are separate, explicit
next steps, not implied by committing.

## Goal

Close the gap SH-H1's live verification found: the node-level shared-hosting
runtime (`crates/valori-node/src/shared.rs`) was real and correct, but
nothing in the Cloud control plane (`valori-ui/backend/apps/api`) ever
called it — every project, Free or paid, still got a dedicated container.
SH2 makes Free-project creation actually register on a shared worker
instead, with zero Docker containers created for it.

## Delivered

All changes are in the `valori-ui` repo (`backend/apps/api`,
`supabase/migrations/`) — no `Valori-Kernel`/`valori-node` code changed,
per the constraint that shared.rs's routing/isolation model was already
correct.

- **Migration** (`supabase/migrations/20260912000000_shared_hosting_mode.sql`):
  additive `hosting_mode` column on `public.runtime_profiles` (operational
  concern, same table as `worker_class`/`container_memory_mb`) and
  `public.projects` (what a specific project actually got, frozen at
  provisioning time — same pattern as `dim`/`index_type`), plus
  `assigned_shared_worker`, `provisioning_generation`, and a new
  `'provisioning'` `project_status` enum value (see Findings — added
  after review to make partial-provisioning states correct, not part of
  the original design). Every existing row defaults to `'dedicated'`
  (current reality, not retroactively changed). The Free plan's
  `free-small` profile is flipped to `'shared'` — the actual behavioral
  change this phase delivers. All four new/touched columns are
  `authenticated`-read-only; `authenticated`'s pre-existing table-level
  `INSERT` and `DELETE` on `projects` are both revoked outright (see
  Findings — the DELETE revoke was a review-driven correction, not part
  of the first pass).
- **Config** (`backend/apps/api/src/config.rs`): `SharedWorkerConfig` +
  `validate_shared_worker_url` — `SHARED_WORKER_URL` /
  `SHARED_WORKER_ADMIN_TOKEN` / `SHARED_WORKER_REGION`, all-or-nothing,
  bare-origin URL validation (no path/query/fragment/credentials), 32-char
  admin-token floor matching the worker's own `VALORI_SHARED_ADMIN_TOKEN`
  minimum. 7 unit tests.
- **Shared-worker client** (`backend/apps/api/src/provision/shared_worker.rs`,
  new file): `SharedWorkerClient` trait (not forced into the existing
  `Provisioner` trait, which is shaped entirely around
  `Host`/`container_id`/`DeployedNode`) + `HttpSharedWorkerClient` (real
  reqwest impl, 10s timeout) + `FakeSharedWorkerClient` (in-memory, for
  unit-testing the branch logic without network — same relationship
  `MockProvisioner` has to `Provisioner`). 10 tests, including live HTTP
  round-trips against a real ephemeral axum server proving URL
  construction, bearer-token attachment, and status-code mapping
  (200/201/409/connection-refused).
- **Provisioning branch** (`backend/apps/api/src/main.rs`):
  `provision_project_inner` now checks `plan.hosting_mode` (added to
  `PlanLimits`, sourced from the `runtime_profiles` join) and calls
  `provision_shared_project()` instead of the placement/volume/deploy loop
  when it's `"shared"` — not a fallback path, a completely different one.
  Branches on data (`hosting_mode`), never a hardcoded `plan_id == "free"`
  check — the exact anti-pattern `20260804010000_runtime_profiles.sql`'s
  own header describes retiring. Reuses the existing
  `projects.worker_auth_token` (already generated once per project,
  reused across redeploys) as the per-project shared-worker credential —
  no new token scheme. `provision_shared_project` calls the new
  `SupabaseWriter::begin_shared_provisioning` (records
  `hosting_mode='shared'`, `status='provisioning'`,
  `assigned_shared_worker`) BEFORE contacting the worker, then
  `mark_shared_project_active` after success — added after review found
  the original after-only write left a wrong `hosting_mode` if activation
  failed post-registration (see Findings).
- **Lifecycle routing**: `stop_project`/`start_project`/
  `delete_project_core` now branch on `project_hosting_mode()` and call
  `SharedWorkerClient::set_active`/`delete_project` for shared projects,
  skipping the `instances`/`hosts`/volume-cleanup code entirely (a shared
  project has none of those rows). `restart_project` explicitly rejects
  shared projects (`501 Not Implemented`) rather than ever restarting the
  shared worker process for one tenant.
- **Supabase writer** (`backend/apps/api/src/supabase.rs`):
  `mark_shared_project_active` — a new method, not a parameter added to
  the existing `mark_project_active`, so the dedicated-container call
  site's behavior is provably byte-identical to before this phase.
- **Docs**: `docs/shared-hosting.md` corrected (it previously claimed Cloud
  integration existed when SH-H1 found it didn't; now states SH2's actual
  status — implemented, locally verified, not yet deployed to Azure) and
  given the real root cause + fix for SH-H1's volume-permission finding
  (see Findings below). `backend/README.md` and `backend/.env.example`
  updated with the new config/provisioner row.

## Findings

- **Two P0 security/correctness bugs found by review, then confirmed and
  fixed live** (both in `20260912000000_shared_hosting_mode.sql`):
  1. `authenticated` held a pre-existing, un-narrowed table-level
     `INSERT` grant on `public.projects` — narrowing it for `hosting_mode`
     alone (the first pass of this migration) missed that the SAME grant
     also allowed a raw `DELETE`. For a shared project, an org owner/admin
     issuing a raw PostgREST `DELETE` against their own project would
     remove the row without ever calling `DELETE /shared/projects/{id}`
     — orphaning a resident engine, WAL, snapshot, and credentials on the
     worker with no Cloud-side record. Revoked outright
     (`revoke delete on public.projects from authenticated`); the app's
     real deletion path (`DELETE /v1/projects/:id`) goes through the Rust
     control plane and never needed this grant. Live-verified after the
     fix: a raw authenticated `DELETE` now returns `403 permission denied
     for table projects`; the row survives.
  2. `hosting_mode` was previously written only at the END of shared
     provisioning (`mark_shared_project_active`, after the worker already
     confirmed registration) — if that final write failed, the row stayed
     `hosting_mode='dedicated'`, `status='creating'`, invisible to every
     lifecycle branch and to any future reconciliation job, with a real
     orphaned registration silently sitting on the worker. Fixed by
     recording the INTENDED runtime and target worker BEFORE contacting
     the worker at all: a new `begin_shared_provisioning()` write sets
     `hosting_mode='shared'`, a new `status='provisioning'` value (added
     to the `project_status` enum), and `assigned_shared_worker`, ahead
     of `client.register_project(...)`. Live-verified with an actual
     fault-injection test (`live_partial_shared_provisioning_leaves_
     correct_hosting_mode_for_lifecycle_ops`): after
     `begin_shared_provisioning` runs and `mark_shared_project_active` is
     deliberately never called (simulating the activation write failing),
     the row already correctly says `hosting_mode='shared'`,
     `status='provisioning'` — exactly what `project_hosting_mode()`
     (main.rs) reads to route delete/stop/start to the shared-worker path
     instead of the dedicated no-op path a stale `'dedicated'` value would
     trigger. A subsequent retry of `mark_shared_project_active` then
     completes activation with no error or duplicate state.
  A separate, related, PRE-EXISTING risk was found and NOT fixed (out of
  this migration's scope — it's Next.js error-handling behavior, not a
  grant): `ui/src/app/api/projects/[id]/route.ts`'s `DELETE` handler calls
  the real Rust control-plane endpoint first, but falls back to a
  Supabase `UPDATE status='deleted'` (not a table DELETE — the row stays,
  just marked deleted) if that call errors or the backend is unreachable.
  That fallback equally skips worker/container cleanup. Recorded as a
  follow-up, not fixed here.
- **Root cause of SH-H1's volume-permission failure, and its fix required
  zero code changes.** The production `Dockerfile` already pre-chowns
  `/data` to the distroless nonroot UID/GID (`65532:65532`) specifically
  so Docker's own "seed a fresh named volume from the matching image path"
  behavior gives it correct ownership. SH-H1's test mounted its volume at
  `/data/shared` — a path with no pre-owned counterpart in the image — so
  Docker created it root-owned instead. Verified live: mounting the exact
  same, unmodified `valori-node:shvfy-test` image's volume at `/data`
  (with `VALORI_SHARED_ROOT=/data/shared`, a subdirectory the process
  itself creates via `SharedHost::open()`'s `create_dir_all`, inside the
  now-correctly-owned volume) succeeded on the first `PUT
  /shared/projects/{id}` call — no `chmod 777`, no root user, no
  Dockerfile edit. This is now documented in `docs/shared-hosting.md`.
- Confirmed via `git stash` comparison: `cargo fmt --check` (387 diffs)
  and `cargo clippy --all-targets -- -D warnings` (7 errors) were both
  already failing on `valori-ui`'s `master` before this phase's changes —
  neither is a regression this phase introduced. SH2's own new code
  contributes zero clippy errors and follows the file's existing
  (non-conforming-to-default-rustfmt) style rather than reformatting
  unrelated pre-existing code.
- `valori-ui`'s Next.js app (`ui/`) has no test framework configured at
  all (no jest/vitest in `package.json`) — Phase H's proxy/URL-handling
  verification was done by direct code reading
  (`NodeClient.request()`'s `` `${nodeUrl}${path}` `` string concatenation
  already handles a path-bearing `node_url` correctly, confirmed in
  SH-H1) plus a live behavioral check (a real Node `fetch` against a
  redirecting HTTP server, confirming Node's built-in `fetch` strips the
  `Authorization` header on a cross-origin redirect) rather than an
  automated regression test, since adding a test framework to `ui/` is
  out of scope for this phase.

## Second correction: real local E2E was performed after review pushback

The first version of this phase doc claimed
`LOCAL IMPLEMENTATION COMPLETE — AZURE DEPLOYMENT AWAITING APPROVAL` on the
strength of unit tests and code reading alone, without running the
migration against a real database or exercising two Free orgs through the
real control-plane stack. That was correctly rejected: the plan-resolution
branch and the Supabase write-back are exactly the kind of logic that
looks right in a unit test with a hand-built struct and is wrong against
real SQL/grants — proven by the fact that the live run below found and
fixed a real security bug the unit tests could never have caught.

**Setup:** `Valori-Kernel/e2e/cloud`'s existing `docker compose` stack
(`postgres` + `migrate` + `postgrest` + `rest-shim` — the real migration
chains from both repos, applied to a real disposable Postgres, fronted by
a real PostgREST), a real `valori-node:shvfy-test` shared-worker container,
and a from-scratch seed file (`e2e/cloud/postgres/02_seed_sh2.sql`): two
Free orgs + one Pro org, each with a real owning user and a real
`subscriptions` row, in the same shape a real signup produces.

**Migration, applied and validated live (from an empty volume):**
- `runtime_profiles.hosting_mode`: `free-small` → `shared`,
  `standard-medium`/`large-memory` → `dedicated` — queried directly.
- `projects.hosting_mode`: `not null default 'dedicated'`, both CHECK
  constraints present, confirmed via `pg_constraint`.
- **Found and fixed a real, live security gap this run's own SELECT-grant
  check didn't catch**: `information_schema.role_table_grants` showed a
  pre-existing, un-narrowed `grant insert, delete on public.projects to
  authenticated` — meaning any authenticated org member could POST a raw
  insert setting `hosting_mode` (and the pre-existing `worker_auth_token`)
  to a value of their own choosing via `/rest/v1/projects`, bypassing
  provisioning entirely. Confirmed the app's only real insert path
  (`create_project_with_default_key()`) is `SECURITY DEFINER` and needs no
  such grant; the migration now `revoke insert on public.projects from
  authenticated` outright. Re-verified after the fix: zero INSERT
  privilege remains for `authenticated` on that table.
- `hosting_mode` confirmed selectable by `authenticated`, `worker_auth_token`
  confirmed not — both live-queried via `information_schema.column_privileges`.

**Real database-logic tests** (new `#[ignore]`d live integration tests,
run explicitly against the stack above — `cargo test live_plan_limits --
ignored` / `cargo test live_mark_shared -- --ignored`):
- `billing::tests::live_plan_limits_resolves_hosting_mode_from_real_database`:
  the real `plan_limits()` SQL join resolves the Free org to
  `hosting_mode="shared"` and the Pro org to `"dedicated"` against the
  real seeded rows. **PASS.**
- `supabase::tests::live_mark_shared_project_active_writes_real_row`: the
  real `mark_shared_project_active` — the exact method
  `provision_shared_project` calls on success — PATCHes `hosting_mode`,
  `node_url`, and `status` through real PostgREST into a real row,
  re-read back via a separate connection to confirm. **PASS.** Re-run a
  second time with the same arguments to double as the retry test below.

**Composed component-level local integration** — not a full two-org Cloud
E2E. Both projects were registered using the identical `PUT
/shared/projects/{id}` contract `provision_shared_project` issues and
activated using the identical `mark_shared_project_active` method,
called directly rather than through the provisioning HTTP endpoint (see
the environment blocker below). This is real, valuable evidence that
every component `provision_shared_project` calls behaves correctly, in
the same order it calls them — it is not evidence that the endpoint
itself, wired end to end, produces this outcome. Reserve "full two-org
Cloud E2E" for an actual call through `POST /v1/projects/:id/provision`
(see the P0 follow-up).
- Both registered (`201`); identical-registration retry returns `200`,
  not `409` — real worker-side idempotency.
- Both DB rows: `hosting_mode='shared'`, `status='active'`, same
  `SHARED_WORKER_URL` origin, distinct `/shared/projects/{id}/node` path.
- Each created **two** collections (`documents`, `products`); inserted
  overlapping record id `0` with different values in each.
- Cross-project token access: `401`.
- State roots diverged correctly (`e50a68c5...` shared prefix before any
  writes → `2c976da7...` vs `18bbf029...` after each org's own insert).
- Container inventory: one shared-worker container total throughout;
  zero project-named containers for either org.
- `docker restart` on the shared worker → both `/v1/proof/state` hashes
  byte-identical to pre-restart; container count unchanged.

**Retry-safety, live (updated after review):** re-registered project A on
the worker with identical credentials — `200`, not `409` — then re-ran
the DB activation write, which succeeded again with no error and no
duplicate row. Both operations were independently verified as idempotent,
but the intermediate failure state and lifecycle handling were not
fault-injected by this specific check (that gap is what the new
`begin_shared_provisioning`/fault-injection test above addresses directly
— see Findings).

**Paid project regression:** Pro plan resolution verified live
(`plan_limits()` resolves to `hosting_mode="dedicated"` for the Pro org);
dedicated deployment regression not executed end to end. The Pro
project's row was never registered on the shared worker (`404` on the
worker's data-plane for that id) and remained untouched
(`dedicated`/`creating`, no token) throughout, which is real evidence the
shared path didn't cross-contaminate it — but `provision_project_inner`'s
dedicated branch itself (`MockProvisioner::deploy()` + an `instances` row
being created) was not freshly run this session; that claim rests on code
reading plus the pre-existing, already-passing `MockProvisioner` tests.

**Genuine environment blocker, not a shortcut — disclosed rather than
routed around:** the outer JWT-gated HTTP route
(`POST /v1/projects/:id/provision` and the `/stop`/`/start`/`/delete`
routes) was **not** exercised end-to-end. `Config::try_from_env()`
correctly requires `SUPABASE_URL` to start with `https://`, and this
crate's `reqwest` is built with `rustls-tls-webpki-roots` — a self-signed
local certificate has no path to being trusted without either a real
publicly-trusted certificate (not obtainable offline in this environment)
or adding a custom root store to the client (a code change to
security-relevant networking code this task's own rules say not to make
for the sake of a local test). Built a real ES256 JWKS server plus a
combined ES256+HS256 `PGRST_JWT_SECRET` JWKS to prove real Supabase-shaped
JWT verification is achievable in this harness at all (confirmed: the
ES256 token was accepted by both the intended `JwksVerifier` code path in
principle and by a live PostgREST instance configured with the dual-key
JWKS), but the `https://` requirement on `SUPABASE_URL` itself blocks
routing the real compiled binary's HTTP layer through it. Every piece of
NEW logic `provision_shared_project`/lifecycle routing calls
(`plan_limits`, `mark_shared_project_active`, `HttpSharedWorkerClient`,
the real shared worker) was independently live-verified above, composed
in the same order the real function calls them — but the orchestrating
function itself was not invoked as one HTTP round trip.

- `cargo test` (backend/apps/api), stated separately per review — a
  normal suite run still ignores the live tests, so "137 passed" alone is
  misleading:
  - Normal backend suite (`cargo test`): **135 passed, 3 ignored, 0
    failed.**
  - Explicit live `#[ignore]`d tests (`cargo test live_ -- --ignored`):
    **3 passed, 0 failed** — `live_plan_limits_resolves_hosting_mode_
    from_real_database`, `live_mark_shared_project_active_writes_real_row`,
    `live_partial_shared_provisioning_leaves_correct_hosting_mode_for_
    lifecycle_ops`.
  - Total distinct successful test executions: 137 (no test double-counted
    between the two runs).
- `cargo build`: clean.
- `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings`:
  unchanged from the first pass — pre-existing baseline failures on
  `master` (387 fmt diffs, 7 clippy errors), confirmed via `git stash`,
  not introduced by SH2.
- Real Docker (volume-fix verification, unchanged from first pass):
  registered a disposable project with the volume mounted at `/data`,
  created a collection, inserted a record, restarted, confirmed the state
  hash matched. Cleaned up.
- **Not performed**: the outer JWT/HTTPS-gated HTTP route end-to-end (see
  blocker above); a real signup click-through via the actual Next.js
  dashboard (`ui/`); anything Azure.

## Third correction: one P0 lifecycle bypass remained, plus a real fencing gap

A third review round found the second correction's fixes were real but
incomplete: `DELETE` was left grantable (only `INSERT` had been revoked),
`status` remained directly UPDATEable by `authenticated` (letting a raw
PostgREST call imitate lifecycle completion even with the Next.js
soft-delete fallback removed), `provisioning_generation` existed but
wasn't enforced as a fence, and the "fault injection" test only verified
an intermediate DB state rather than making the real orchestration-level
activation call fail. All four addressed and live-verified:

1. **Next.js soft-delete fallback removed**
   (`ui/src/app/api/projects/[id]/route.ts`). The `DELETE` handler no
   longer falls back to `supabase.from('projects').update({status:
   'deleted'})` when the Rust control plane errors or is unreachable —
   that never deleted anything, it only hid the row while a shared
   project's engine/WAL/credentials or a dedicated project's
   container/volume kept running. Now: backend unreachable → `503`,
   "the project was not deleted, please retry"; backend error → that
   error's status, same message; success → `204`. The project row is
   left unchanged on any failure path.

2. **`status` (and every SH2 column) is now control-plane/service-role
   only.** `authenticated`'s UPDATE grant on `projects` narrowed from
   `(name, status, last_active_at)` to `(name, last_active_at)` only —
   `status`, `hosting_mode`, `assigned_shared_worker`,
   `provisioning_generation`, `node_url`, and `worker_auth_token` were
   never all grantable to begin with; `status` was the one gap, now
   closed. `name` (the real rename feature) and `last_active_at` (the
   real activity-ping) were deliberately left alone — both are real,
   working, authenticated-session features with no reason to touch.
   Live-verified: a raw authenticated `PATCH {"status":"deleted"}`
   returns `403 permission denied for table projects`; a `PATCH
   {"name":"..."}` from the same token still succeeds; the row's
   `status` is confirmed unchanged after the attack.

3. **`provisioning_generation` is now a real, enforced fence, not an
   informational counter.** Added `increment_shared_provisioning(project_id,
   worker)`, a `SECURITY DEFINER` SQL function (service-role-only
   `EXECUTE`) that atomically increments the counter and sets the target
   runtime/worker in one statement — PostgREST's PATCH can only carry
   JSON values, not the `col = col + 1` expression an atomic increment
   needs, and a read-then-write from Rust would race a concurrent retry
   of the same project. `begin_shared_provisioning` now calls this RPC
   and returns the new generation; `mark_shared_project_active` takes
   `expected_generation` + `expected_worker` and its PATCH's own filter
   requires `hosting_mode=shared AND status IN (provisioning, active) AND
   provisioning_generation=$expected AND assigned_shared_worker=$expected`
   — `Prefer: return=representation` is required (not `return=minimal`)
   specifically so zero-rows-matched (a fenced-out activation) is
   distinguishable from one-row-updated, since PostgREST returns `200`
   with an empty array for the former, not an error. A new
   `SupabaseError::ActivationFenced` variant makes that outcome explicit
   to the caller rather than silently swallowed.

   Live-verified with the exact scenario specified: begin generation 1 →
   begin generation 2 for the same project → attempt to activate
   generation 1 → **rejected** (`ActivationFenced`, confirmed `status`
   stays `provisioning` and `node_url` stays unset) → activate generation
   2 → succeeds → retry generation 2 → idempotent success (the `status IN
   (provisioning, active)` clause is what makes a retry of an
   already-active generation still match, rather than being incorrectly
   fenced out just because `status` moved past `provisioning` the first
   time it ran).

4. **Corrected fault-injection framing, as instructed.** The previous
   claim conflated "verified the intermediate DB state" with "injected an
   activation failure." What's actually true, and now stated as such: the
   stale-generation rejection above IS a real orchestration-level
   failure — `mark_shared_project_active`'s conditional PATCH genuinely
   returns `ActivationFenced` for generation 1, a real outcome of the
   real fencing logic, not a skipped step — followed by a real successful
   retry at generation 2. This is a stronger and more honest claim than
   the previous version, though it still doesn't test failures of other
   kinds (a mid-flight network drop, a 500 from PostgREST itself) — those
   remain untested.

Live-verified end to end on a fresh migration apply: table-level
`INSERT`/`DELETE` grants for `authenticated` on `projects` are both empty;
column-level `UPDATE` grants are exactly `{name, last_active_at}`; raw
authenticated `INSERT`, `DELETE`, and `status`-`UPDATE` all return `403`;
`name`-`UPDATE` still succeeds; the full generation-fencing sequence
passes as a live `#[ignore]`d test
(`live_stale_generation_activation_is_rejected_current_generation_succeeds`).
`cargo test`: 135 passed/3 ignored (normal suite), 3 passed/0 failed
(explicit live run) — same counts as the second correction; two of the
three live tests were rewritten in place for the new fenced signatures,
none added or removed. `cargo clippy --all-targets -- -D warnings`: still
exactly 7 errors, the same pre-existing `master` baseline.

## Fourth correction: `SECURITY DEFINER` audit + `last_active_at` bypass

A fourth review round audited `increment_shared_provisioning()` (added in
the third correction) against a full `SECURITY DEFINER` checklist, and
separately flagged that `last_active_at` — left authenticated-writable in
every prior round on the reasoning that it backs "a real, working
activity-ping" — has the same bypass shape as the bugs already fixed.
Both addressed and live-verified:

1. **Explicit per-role revokes, not just `from public`.** Added
   `revoke all ... from anon` and `revoke all ... from authenticated`
   alongside the existing `from public` — a pseudo-target revoke doesn't
   by itself prove no role was ever separately granted execute, and an
   RPC is not subject to the underlying table's column grants at all, so
   a `SECURITY DEFINER` function callable by `authenticated` would
   recreate the exact bypass this migration otherwise closes, through a
   different entry point. Live-verified: `authenticated` calling
   `POST /rpc/increment_shared_provisioning` now returns `403 permission
   denied for function increment_shared_provisioning`.
2. **Ownership boundary documented explicitly, not just implied.** The
   function trusts `p_project_id` as-is and does not re-check
   organization ownership — recorded in the function's own comment why
   that's correct rather than an oversight: it's reachable only via the
   service-role key, i.e. only from `provision_shared_project`, which
   only runs after `provision_project`'s handler has already verified the
   calling user owns this exact project. Re-deriving ownership inside the
   function would be redundant, not additional safety.
3. **Guards against acting outside where the operation is sane.**
   Rewritten from `language sql` to `language plpgsql` specifically to
   add `where ... and status <> 'deleted'` plus an explicit
   `if not found then raise exception`. Before this, a delayed or retried
   provisioning call against an already-deleted project would have
   silently resurrected it (`status='provisioning', hosting_mode=
   'shared'`) — and a call against a nonexistent project would have
   silently returned `NULL` (a `language sql` function's behavior for a
   no-rows `RETURNING`), which the Rust caller could not have
   distinguished from a transport error. Live-verified both: an
   already-deleted project's row is confirmed unchanged
   (`status='deleted', hosting_mode='dedicated'`) after the call raises;
   a nonexistent id raises the same explicit exception.
4. **Confirmed already-satisfied, not changed:** the function never
   selects or returns `worker_auth_token`; `provisioning_generation` is
   read back from the database's own `RETURNING` clause, never computed
   in Rust; the update+return is one atomic statement, so it always
   affects exactly zero or one row, never more.
5. **`last_active_at` moved to service-role-only, closing the same
   bypass shape.** This column feeds the autosuspend sweep
   (`scheduler/jobs/suspend.rs` compares it directly against
   `runtime_profiles.suspend_after_days`, which IS configured for
   `free-small` — confirmed this isn't a hypothetical, autosuspension
   already relies on it today). With a direct `authenticated` UPDATE
   grant, any holder of a valid session JWT could keep a Free project
   "warm" indefinitely with a periodic raw PostgREST PATCH, never making
   a real data-plane request — bypassing the legitimate signal
   (`resolveProjectNodeUrl` in `ui/src/lib/server/project.ts`, reached
   only from an actual resolved data-plane request) entirely. Fixed two
   ways together: that function's deferred write now uses the
   service-role client (`createServiceClient()`, already used elsewhere
   in the same file) instead of the caller's own session, and the
   migration's `authenticated` UPDATE grant is narrowed to `(name)` only
   — narrowing the grant without the code change would just break the
   legitimate ping; making the code change without narrowing the grant
   would leave the raw-PostgREST bypass wide open. Live-verified: a raw
   authenticated `PATCH {"last_active_at": ...}` now returns `403`.

Live-verified on a fresh migration apply (fifth clean rebuild this
phase): the RPC's `EXECUTE` grant is exactly `{postgres, service_role}`;
the deleted-project and nonexistent-project guards both raise as
designed; the `last_active_at`/RPC bypasses are both closed by real
requests against a real PostgREST, not by inspection. `cargo test`: 135
passed/3 ignored (normal suite), 3 passed/0 failed (explicit live run) —
unchanged counts, no test signatures changed this round.
SH2 tests and build pass; SH2 introduces no new Clippy findings.
Repository-wide Clippy remains blocked by seven pre-existing findings
(confirmed via `git stash` comparison against `master`, unchanged from
the first correction) — not "clean," a distinct and narrower claim.

## Fifth correction: `search_path` hardened

A fifth review round found `increment_shared_provisioning()`'s
`set search_path = public` was more permissive than a `SECURITY DEFINER`
function should use — an object created later that happened to shadow
something the function resolves unqualified could hijack execution under
the function owner's privileges if `public` were searched without
`pg_catalog` explicitly ahead of it. Changed to
`set search_path = pg_catalog, public` (the standard hardened pattern).
The function's only table reference was already schema-qualified
(`public.projects`) before this change; the `search_path` fix is defense
in depth on top of that, not a substitute for it. Live-verified on a
sixth clean migration rebuild: `pg_proc.proconfig` shows exactly
`{"search_path=pg_catalog, public"}`; all three live tests
(`live_plan_limits_resolves_hosting_mode_from_real_database`,
`live_mark_shared_project_active_writes_real_row`,
`live_stale_generation_activation_is_rejected_current_generation_succeeds`)
still pass unchanged against the hardened function. `cargo test`: 135
passed/3 ignored (normal suite), 3 passed/0 failed (explicit live run).
`cargo clippy --all-targets -- -D warnings`: still exactly the same 7
pre-existing findings, none new.

## Follow-ups

Per the fifth review's verdict — `READY FOR STAGING VALIDATION`, `NOT
READY FOR PRODUCTION FREE-TIER ONBOARDING` — the next steps are getting
this exact, reviewed code onto staging, not further local verification:

1. Review the complete diff across both repositories one more time
   (`git diff` in each of `Valori-Kernel` and `valori-ui`) before
   committing anything — nothing has been committed yet.
2. Create an SH2 feature branch in each repo and commit these changes.
3. Push and open a PR in each repo.
4. Deploy that exact reviewed commit to staging (not a re-implementation
   from memory — the same SHA that was reviewed).
5. Apply `20260912000000_shared_hosting_mode.sql` to staging.
6. Configure one staging shared worker (a real `valori-node` with
   `VALORI_SHARED_ROOT` set, per `docs/shared-hosting.md`).
7. Configure `SHARED_WORKER_URL`/`SHARED_WORKER_ADMIN_TOKEN`/
   `SHARED_WORKER_REGION` on the staging control plane.
8. Call the real, authenticated `POST /v1/projects/:id/provision` HTTP
   endpoint — this is the one thing local verification in this phase
   could not reach (`SUPABASE_URL` requiring `https://`, and this
   crate's `reqwest` bundling `rustls-tls-webpki-roots` rather than the
   OS trust store, has no path to trusting a local self-signed cert;
   staging has a real trusted certificate, so this is exactly where that
   verification belongs, not a gap to route around locally).
9. Create two Free projects through the real UI/API (not the
   composed-component-level calls this phase used locally).
10. Confirm zero project-specific containers are created.
11. Run the complete isolation and restart matrix (collections, records,
    cross-project rejection, state roots, `docker restart` recovery) —
    already proven at the component level locally; this step proves the
    full endpoint produces the same outcome.
12. Create one paid project and confirm it takes the dedicated
    `Provisioner::deploy()` path — not freshly exercised at any level
    this phase, so this is also this feature's FIRST real dedicated-path
    test, not just a repeat.
13. Test worker-unavailable and activation-failure recovery against the
    real endpoint (the generation-fencing logic is proven at the
    component level; this proves the HTTP layer surfaces it correctly).
14. Save the evidence (logs, container inventories, state hashes) and the
    deployed commit SHA. Roll back the staging deployment and verify the
    rollback procedure itself works.
15. Do not enable public Free signups yet — staging passing is the gate
    for corrupt-project quarantine work to begin, not for production
    onboarding.
- **P0 before public Free-tier onboarding** (not before staging): corrupt-
  project quarantine (`docs/superpowers/specs/2026-09-12-shared-hosting-hardening-design.md`,
  Phase 4A) is still not implemented. Today, one corrupt project's log
  makes `SharedHost::open()` fail entirely, taking every other Free
  project on that worker down with it on the next restart. Acceptable for
  the isolated staging test above (disposable projects, small blast
  radius, operators watching); not acceptable once real Free users are
  onboarded at any volume.
- **P1:** decide and implement the `region` field's actual routing once a
  second shared worker exists (SH2 is deliberately single-worker).
- **P2:** consider a minimal test framework for `ui/` if more
  proxy/URL-handling logic accumulates there — today's verification
  leaned on direct code reading because none exists.
