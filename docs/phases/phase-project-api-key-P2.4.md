# P2.4 — Legacy-Key Cleanup and SDK URL Correction

## Goal

Close two of P2's open decisions per explicit direction from the user:
(1) no real legacy org-scoped keys exist anywhere (Cloud isn't deployed
yet), so remove the compatibility path protecting data that doesn't
exist, and (2) `api.valori.systems` doesn't exist and isn't needed — the
SDK should target the existing `app.valori.systems` domain instead.

## Delivered

**Legacy-key removal** (`supabase/migrations/20260810000000_project_scoped_api_keys.sql`,
edited in place — confirmed with the user this migration has never been
applied to a real Cloud deployment, so there's no compatibility window to
preserve):

- `api_keys.project_id` is now `not null` from this migration's first
  real application onward (was nullable, specifically to protect
  hypothetical legacy rows that turned out not to exist).
- `verify_api_key()`: removed the `target_project_id` parameter entirely
  (it only ever existed to let the legacy org-scoped path know which
  project within an org was meant — with no legacy path, nothing consumes
  it) and the `key_kind` return column (always `'project'` now, so it
  carried no information). The function now has exactly one code path:
  resolve `node_url`/`status` from `key_row.project_id`, unconditionally.
  Old signature `(text, uuid, text, inet)` dropped and replaced with
  `(text, text, inet)` — same drop-before-replace convention this schema
  already uses for every breaking change to this function.
- `ui/src/lib/server/project.ts`: `resolveProjectNodeUrlByApiKey()` no
  longer passes a target project into the RPC (nothing to pass); the
  URL-vs-authenticated-project 403 check is unchanged in behavior, just
  simpler to read. `resolveOwnProject()` lost its `NIL_PROJECT_ID`
  workaround entirely — with no target parameter to satisfy, there's
  nothing to work around. Both now share one small `verifyApiKey()`
  wrapper instead of duplicating the RPC call shape.
- `supabase/tests/project_scoped_api_keys.test.sql`: removed the
  legacy-key test case (nothing left to test); the old "project A key
  presented against project B's target" adversarial test no longer
  applies either — there's no target parameter left to attack, so the
  test was replaced with a determinism check (the same key always
  resolves to the same project) plus a note that the actual cross-project
  guarantee now lives entirely at the application layer, already proven
  live in P2.3. Also split the old combined scope test into two clearer
  cases: `project:full` correctly acting as a wildcard, and a genuinely
  narrow-scoped key correctly failing a scope it doesn't have (the
  original test only exercised the wildcard case, which doesn't prove
  scope checking rejects anything).

**SDK URL correction** (`python/valoricore/remote.py`,
`docs/architecture/project-api-key-architecture.md`): example code and
docstrings changed from `https://api.valori.systems` to
`https://app.valori.systems` — the SDK's routes (`/api/projects/{id}/...`,
`/api/me`) are Next.js routes inside the same app that serves the
dashboard, not a separate deployment, so there's no new subdomain to
create. Checked for other `api.valori.systems` references first — found
several in `valori-ui/backend`'s deploy configs and this repo's older
audit docs, all of which correctly refer to the **Rust control plane's
own domain** (provisioning, telemetry ingest) — a real, different, already
-correct service, deliberately left untouched.

## Findings

- The cleanup was net simplification everywhere it touched: fewer
  parameters, fewer branches, fewer things to explain in comments. Nothing
  about it was structurally difficult — the hard part of this feature was
  always the project-scoping fix itself (P2/P2.2/P2.3), not the legacy
  scaffolding around it.
- Confirmed via a fresh grep sweep that `api.valori.systems` appearing
  elsewhere in the codebase is a real, unrelated, correctly-named service
  (the Rust backend's own domain) — worth checking before any blanket
  find/replace on a domain name, since two legitimately different
  services can share a plausible-looking prefix.

## Validation

- Re-ran the full migration chain (37 `supabase/migrations` + `backend/migrations`)
  against a fresh disposable Postgres, including this rewritten migration —
  applies cleanly.
- Re-ran `supabase/tests/project_scoped_api_keys.test.sql` live: **9/9
  PASS** (was 10/10 pre-cleanup; one case removed as no longer applicable,
  one split into two more precise cases — net same coverage, more precise).
- `npx tsc --noEmit` / `npm run build` — clean in `valori-ui/ui`.
- `python3 -c "from valoricore.remote import Valori"` — clean.
- No Rust files touched this phase (`valori-cloud-api`'s test suite
  unaffected, not re-run).

## Follow-ups

Unchanged from P2.3, minus the two items this phase resolved (legacy-key
policy, `api.valori.systems`):

1. Full Next.js session-path E2E, Python SDK against a genuinely deployed
   Cloud environment (still simulated via disposable infra, never a real
   deployment).
2. `resolveNodeOrThrow()`'s replacement still session-only for
   `why.ts`/`namespace-audit`.
3. `AuthenticationError`'s message text for the Cloud 401 case.
4. Async Python SDK, packaging (`valori` vs. `valoricore`).
5. `VALORI_AUTH_TOKEN` production rollout decision (still not required to
   be enabled, node-level auth stays off by default until it is).
6. `Valori-Kernel/ui` vs `valori-ui/ui` consolidation.
