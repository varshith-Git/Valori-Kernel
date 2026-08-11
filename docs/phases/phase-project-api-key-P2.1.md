# P2.1 — Project-Scoped API Keys: Production Hardening

## Goal

Close the concrete gaps the P2 report itself identified, per the user's
explicit 14-item hardening list, before considering the project-scoped
API-key foundation production-ready. Not a redesign — P2's architecture
stands; this phase fixes real bugs and executes real tests against it.

## Delivered

**Real, live execution — not just static review.** A disposable local
Postgres 16 instance was stood up (`brew install postgresql@16`, no Docker
daemon was available), with a hand-built stub of Supabase's managed
`auth` schema (`auth.users`, `auth.uid()`) alongside the real `infra`
schema (all of `valori-ui/backend/migrations/*.sql`, sqlx's own) and the
real `public` schema (all 37 of `valori-ui/supabase/migrations/*.sql`,
including P2's own migration). This let three real, previously-untested
code paths actually run for the first time:

1. **The full security test matrix** (`supabase/tests/project_scoped_api_keys.test.sql`,
   written in P2, never executed until now) — **all 10 assertions pass**
   against a live database with the real schema and real functions.
2. **FK cascade on project deletion** — new live test proves deleting a
   `projects` row deletes its `api_keys` rows via `on delete cascade`, not
   left dangling.
3. **Transaction atomicity under failure** — new live test forces
   `create_project_with_default_key()` to fail (duplicate slug,
   `unique_violation`) and proves the project row count and key row count
   are unchanged afterward — no orphan project, no orphan key.

**Three real bugs found by actually running the migration, none caught by
review alone:**

1. `api_keys_public`'s `create or replace view` inserted `project_id`
   mid-column-list — Postgres rejects `CREATE OR REPLACE VIEW` reordering
   existing columns (`cannot change name of view column`). Fixed: new
   columns appended strictly at the end, matching every prior addition to
   this view.
2. `create_project_with_default_key()`'s input parameters
   (`project_name`, `project_slug`, etc.) collided with identically-named
   `RETURNS TABLE` output columns — plpgsql rejects this
   (`parameter name ... used more than once`). Fixed: input parameters
   renamed with a `p_` prefix (matching this file's own existing
   convention elsewhere), output column names — and the TypeScript
   call site consuming them — left as originally designed. The RPC's
   *input* keyword-argument names changed accordingly
   (`ui/src/app/dashboard/actions.ts` updated to match).
3. The test fixtures themselves had two errors — an invalid UUID literal
   (a stray `u` character) and a missing `created_by`/wrong column name
   against the real schema — both are exactly the kind of bug "written but
   never run" SQL accumulates; fixed by actually running it.

**Item 1 — the stale `Valori-Kernel/ui` caller, now fixed.** Confirmed
live via `grep` before touching anything: `ui/src/app/cloud/settings/api-keys/actions.ts`
still called `create_api_key` with the pre-P2 3-argument shape. Ported the
identical fix already applied to `valori-ui/ui`'s copy across three files
(`page.tsx`, `actions.ts`, `ApiKeysManager.tsx`) — **plus a fourth call
site not previously known about**, found only by running `tsc`:
`ui/src/app/settings/page.tsx` (Desktop's local Settings page, which
renders the same `ApiKeysManager` component inline for its "Cloud sync"
section) was also calling the old prop shape. Fixed by fetching and
threading a `projects` list through the same way the two server-component
pages already do. `npx tsc --noEmit` and `npm run build` both clean in
`Valori-Kernel/ui` after the fix.

**Item 13 — `duplicateProject()`'s reveal-once gap, fixed.**
`ProjectActions.tsx`'s "Duplicate" button now shows the new project's
Default key in a reveal-once dialog (reusing the same `CopyBtn` pattern as
every other key-reveal UI in this codebase) before navigating to
`/dashboard`, instead of silently creating a key the user was never shown.
Fixing this surfaced a TypeScript narrowing bug in `duplicateProject()`'s
early-return branch (missing `apiKey: null` on the "source project not
found" case, breaking the discriminated union) — fixed alongside it.

**Item 6 — raw-key logging/telemetry sweep, performed.** `grep` across
`valori-ui/ui/src`, `valori-ui/backend/apps/api/src` for `console.log` /
`console.error` / `console.warn` near key/token-shaped variable names:
every hit is `auditError.message` (a Supabase error string), never key
material. `backend/apps/api/src/telemetry*.rs` (pre-existing, unrelated
to this phase) does not reference key data at all.

**Item 8 — reveal-once key never persisted to browser storage,
confirmed.** `grep` for `localStorage`/`sessionStorage` across every file
this phase (P2 and P2.1) touched — zero hits. Every reveal-once key lives
only in transient React `useState`, discarded on dialog close or
navigation.

**Item 7 — `api_keys_public` never exposes secrets, confirmed live** (not
just schema review this time): the security test file's own assertion
queries `information_schema.columns` for `key_hash`/`plaintext_key` on the
view and found neither — this ran against the real, deployed view
definition, not the migration source text.

## Findings

- **No Docker daemon and no `supabase` CLI were available in this
  environment.** Installing `postgresql@16` via Homebrew and hand-building
  the Supabase-equivalent stub schema was the only way to get real
  execution rather than reviewed-but-unrun SQL. This is a legitimate
  disposable local database, not a shortcut — it caught three real bugs
  P2's static review missed entirely.
- **`Valori-Kernel/ui`'s stale caller had a second, undiscovered instance**
  (`settings/page.tsx`) beyond the one flagged in the P2 report. This
  reinforces the P2 report's own point: the drift between
  `Valori-Kernel/ui` and `valori-ui/ui` is an ongoing liability, not a
  one-time fix — grepping for every call site each time is not a
  substitute for the consolidation work flagged (and still not done) two
  conversations ago.
- **Items #3, #4, #5, #12 from the hardening list — real Cloud E2E, real
  cross-project HTTP requests, real expiry/revocation over HTTP, and the
  Python SDK against an actual deployed Cloud endpoint — were NOT
  attempted.** These need a full stack: PostgREST in front of Postgres (to
  give `@supabase/supabase-js` something to actually talk to — it does not
  speak raw Postgres wire protocol), a running Next.js dev server with
  real environment variables, and a running `valori-node` instance, all
  wired together. Standing up PostgREST alone is a further, separate
  infra installation; simulating "a live Cloud environment" convincingly
  enough to trust the result is a meaningfully larger undertaking than
  this phase's SQL-level verification, and attempting a shaky partial
  version of it risked producing a result that *looked* like a real E2E
  test but wasn't one. Reported honestly as not done, per this
  conversation's established standard, rather than faked or approximated.

## Validation

- `psql -f supabase/tests/project_scoped_api_keys.test.sql` — **10/10
  PASS**, against a real Postgres 16 instance with the complete real
  migration chain (37 `supabase/migrations` + 19 `backend/migrations`)
  applied.
- 2 new live SQL tests (FK cascade, transaction rollback/atomicity) — both
  PASS, run the same way.
- `npx tsc --noEmit` — clean in both `valori-ui/ui` and `Valori-Kernel/ui`.
- `npm run build` — clean, full production build, in both
  `valori-ui/ui` and `Valori-Kernel/ui`.
- `grep` sweeps for raw-key logging and browser-storage persistence —
  clean in both repos, scoped to every file either P2 or P2.1 touched.

## Follow-ups

1. **Items #3/#4/#5/#12** (real Cloud E2E, cross-project HTTP isolation,
   expiry/revocation over real HTTP, Python SDK against a live Cloud
   endpoint) — genuinely not done. Needs either a real staging Cloud
   deployment or a properly-provisioned local stack (PostgREST +
   Next.js dev server + `valori-node`), attempted as its own scoped piece
   of work, not squeezed into this phase.
2. **Item #9** (legacy `project_id IS NULL` key policy) — still an open
   product decision, not resolved by this phase; P2's grandfather-window
   behavior remains in effect by default.
3. **Item #14** (`VALORI_AUTH_TOKEN` defense-in-depth) — still an open
   decision, not implemented; every Cloud-provisioned node remains
   unauthenticated at the node level.
4. **`Valori-Kernel/ui` vs `valori-ui/ui` consolidation** — the
   already-flagged duplication problem, made concretely worse this phase
   (a *second* undiscovered stale call site was found). Still not
   scheduled.
