# Phase G2.3.2-E — Project Error / Retry Provisioning

## Goal

Determine how a project stuck in `status = 'error'` is meant to be
re-provisioned, and implement the minimum fix if the backend already
supports it but the UI never exposed it.

---

## Part 1 — Traced the frontend

`ui/src/app/dashboard/projects/[id]/ProjectActions.tsx` (before this
phase): buttons for Start/Stop, Restart (active only), Rename, Duplicate,
Archive, Delete. **No retry/provision/launch action existed anywhere** —
confirmed by reading the full component, matching the screenshot exactly
(Rename / Duplicate / Delete Project, nothing else for an `error`
project).

`page.tsx`'s status copy for `error` was: *"Provisioning failed. Try
creating a new project."* — this is prescriptive UI copy, not evidence of
a backend constraint. Nothing in the frontend ever calls `POST /v1/
projects/:id/provision` for an **existing** project id — `createProject`/
`duplicateProject` (`actions.ts`) both always create a **brand-new**
project row first (`create_project_with_default_key` RPC, a fresh UUID),
then provision that new id. There was no client-side call site that
retries the *same* id at all.

## Part 2 — Traced the backend

`POST /v1/projects/:id/provision` → `provision_project` (`AuthUser`,
customer session, not admin) → `provision_project_inner`
(`backend/apps/api/src/main.rs:860-962`). Read the full function body,
not assumed:

- **No status check anywhere.** The function reads `body.region`/
  `body.replication`, resolves the project's `worker_auth_token` (`resolve
  _worker_auth_token` — idempotent, reuses the existing stored token,
  never a new one), calls `WorkerService::find_available` fresh, deploys,
  inserts instance rows, reserves slots, and **unconditionally** calls
  `state.supabase.mark_project_active(id, &node_url)` on success. `status
  = 'error'` is not rejected, not specially handled, not checked at all.
- **Does not "reset" project state** beyond what a normal provision
  already does — `mark_project_active` sets `status = 'active'` and
  `node_url`, the same two fields the original create path sets.
- **Does not reuse a specific prior worker placement** — `find_available`
  re-runs placement fresh against current capacity every call. For a
  single-worker environment (the real state today) this necessarily lands
  on the same worker again; with more workers it could differ.
- **Does not clean up pre-existing `infra.instances` rows** before
  deploying more — unlike the admin-only DR path (`rebuild_project_core`,
  which does a best-effort `destroy` of old instances first). For
  project `6d88266a-...` specifically this is a non-issue: the original
  orphaned container was created *before* any `infra.instances` row could
  be written (the exact bug fixed in Phase G2.3.2 — Caddy failure aborted
  `deploy()` before the insert), and the audit's earlier finding is that
  it was manually removed on the worker with no DB row ever existing for
  it. Calling `provision_project` again for this id is a **clean, fresh
  placement** — no duplicate-instance risk here. In general, calling this
  endpoint on a project that already has live instance rows *would* add
  more without removing the old ones — a real, pre-existing property of
  this endpoint, not something this phase changes or was asked to fix.
- The admin-only `POST /v1/admin/disaster-recovery/projects/:id/rebuild`
  (`AdminAuth`) exists specifically for `status = 'error'` projects
  (`list_dr_incidents` queries exactly that), and its own doc comment
  confirms it "reus[es] `provision_project_inner` **verbatim**: same host
  selection, same deploy/insert/reserve-slot/mark-active path every normal
  provision already goes through" — i.e., the admin rebuild path and the
  plain customer provision path are the same underlying mechanism, just
  different auth and different pre-cleanup.

**Conclusion: retry is already fully supported by the existing,
unmodified customer-facing endpoint.** The backend was not changed.

## Part 3 — This project is a valid retry target

Confirmed from Part 2: `6d88266a-47f6-42bd-a358-58ea0ae6e557` has no
tracked instance rows, so a plain `provision_project` call is a clean
first-time deploy for this id — safe to reuse verbatim.

## Part 4 — Minimum fix implemented

Retry was already supported by the backend but not exposed by the UI —
the first branch of the brief's own decision tree. Implemented the
smallest addition:

- **`actions.ts`**: new `retryProvisioning(projectId)` — reads the
  project's own stored `region`/`replication` (via the existing
  `fetchSafeProjectById` helper from the G2.3.2-D fix, not a fresh
  `select('*')`), then calls the exact same `POST /v1/projects/:id/
  provision` with `{region, replication}` — the identical shape
  `createProject` already sends. On failure, sets `status = 'error'`
  again (same failure semantics as the original provision path). No new
  endpoint, no new request shape, no new project state.
- **`ProjectActions.tsx`**: one new "Retry provisioning" button, visible
  only when `projectStatus === 'error'`, calling the action above and
  `router.refresh()` on success.
- **`page.tsx`**: corrected the `error`-status copy from *"Try creating a
  new project"* to *"Hit Retry provisioning above to try again"* — the
  old copy was accurate only because no better option existed; it's
  inaccurate now that one does.

No new project lifecycle state was invented — still exactly `creating /
active / error / stopped / suspended / deleted / archived`, unchanged.
No new project was created automatically. No database row was touched
outside the existing `status`/`node_url` fields the provisioning flow
already writes.

---

## Verification

```
$ npx tsc --noEmit       → clean, 0 errors
$ npx eslint <changed>   → clean, 0 warnings/errors
$ npm run build          → ✓ Compiled successfully in 8.1s
```

**Manual/live verification: NOT PERFORMED beyond static checks.** Same
constraint as every infrastructure-adjacent phase in this engagement — no
real Supabase session available in this session to click through the
actual button. Steps 6-9 of the brief (worker receives the container,
Caddy route created, node_url populated, project becomes active) are
explicitly real-operator steps, not attempted here, per the brief's own
instruction not to perform them in this environment.

For whoever runs the real check:
1. Open `/dashboard/projects/6d88266a-47f6-42bd-a358-58ea0ae6e557`.
2. Confirm "Retry provisioning" is now visible next to Rename/Duplicate.
3. Click it.
4. Confirm the request hits `POST {VALORI_CLOUD_API_URL}/v1/projects/6d88266a-47f6-42bd-a358-58ea0ae6e557/provision`.
5. Confirm the worker receives a new container, Caddy registers the
   route, `node_url` populates, and the project flips to `active` —
   real infrastructure verification, not part of this code change.

---

## FINAL VERDICT

```
PROJECT DETAIL:
PASS

PROJECT ERROR STATE:
EXPECTED — status='error' correctly reflects that the original provisioning
attempt (before the Caddy fix in Phase G2.3.2) never succeeded; this is
not a bug, it's the accurate outcome of a real failed deploy

RETRY SUPPORTED BY BACKEND:
YES — provision_project_inner has no status gate, confirmed by direct
source reading; already safe to call again for this specific project
(zero existing instance rows)

RETRY EXPOSED BY UI:
NO (before this phase) → YES (after)

MINIMUM FIX:
One new server action (retryProvisioning, reusing the existing POST
/v1/projects/:id/provision endpoint verbatim) + one new conditional button
in ProjectActions.tsx, visible only for status='error'. No backend change.

FILES CHANGED:
ui/src/app/dashboard/actions.ts
ui/src/app/dashboard/projects/[id]/ProjectActions.tsx
ui/src/app/dashboard/projects/[id]/page.tsx
```

STOP.
