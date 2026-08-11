# P2 — Project-Scoped API Keys: Production Implementation

## Goal

Implement, end-to-end, the project-scoped API-key foundation designed in
P1: `ApiKey → exactly one ProjectId`, Cloud remains the authentication
authority, `valori-node` never becomes a Cloud auth authority, and a
client-supplied project id is never trusted for authorization.

## Delivered

**`valori-ui` (private repo)**

- `supabase/migrations/20260810000000_project_scoped_api_keys.sql` — the
  core schema + function change:
  - `api_keys.project_id uuid references projects(id) on delete cascade`
    (nullable — legacy rows keep `null`) and `api_keys.expires_at
    timestamptz` (nullable — `null` = never).
  - `verify_api_key()` rewritten: a project-scoped key (`project_id is not
    null`) resolves `node_url`/`status` from **its own** `project_id`
    only — the caller-supplied `target_project_id` is never consulted for
    authorization on that path. A legacy key (`project_id is null`) keeps
    its exact pre-migration org-scoped behavior. New return columns:
    `project_id`, `api_key_id`, `key_kind` (`'project' | 'legacy_org'`).
    Also gained an `expires_at` check. Signature (parameter list)
    unchanged from the pre-P2 version — see the architecture doc's Q12 P2
    note for why that was a deliberate deviation from the design's
    "conceptual" simplified signature.
  - `create_api_key()`: `p_project_id uuid` is now a **required**
    parameter (no default) — no code path can create a new
    `project_id = null` row anymore. Default scope changed to
    `array['project:full']`. New `p_expires_at` parameter. Per-project key
    limit (`where project_id = p_project_id`, was `where org_id =
    target_org_id`).
  - New `create_project_with_default_key()` — one Postgres function,
    inserts the `projects` row and its `Default`/`project:full`/
    never-expiring `api_keys` row in a single transaction; either both
    exist or neither does.
  - `api_keys_public` view extended with `project_id`/`expires_at`.
  - Followed this schema's own established convention (`drop function if
    exists` before `create or replace` whenever a signature or return type
    changes) exactly, per the explicit prior-migration comments warning
    about the PostgREST-overload hazard.

- `ui/src/lib/server/project.ts` — `resolveProjectNodeUrlByApiKey()`
  rewritten to read the RPC's new `project_id`/`key_kind` output and
  perform the URL-vs-authenticated-project comparison itself; two new
  error classes, `ApiKeyInvalidError` (401) and `ApiForbiddenError` (403),
  replacing the prior blanket `ProjectNotFoundError` (404) for these cases.

- `ui/src/lib/server/nodeProxy.ts` — maps the two new error classes to
  401/403 respectively; `ProjectNotFoundError` stays 404, now reserved for
  the session-based path only.

- `ui/src/app/dashboard/actions.ts` — `provisionNewProject()` now calls
  `create_project_with_default_key()` instead of a bare table insert;
  returns the plaintext Default key (once) alongside the created project.

- `ui/src/app/dashboard/CreateProjectDialog.tsx` — shows a reveal-once key
  screen immediately after project creation, in place, no extra
  navigation; never persists the key beyond component state.

- `ui/src/app/dashboard/settings/api-keys/{page,actions,ApiKeysManager}.tsx` —
  create-key flow now requires picking a project (dropdown, populated from
  the org's projects), offers an expiry select (never/30/60/90 days),
  dropped the old read/write scope checkbox (every new key is
  `project:full`), and the key list shows Project + Expires columns
  (`project_id = null` rows display as "legacy (org-wide)").

- `supabase/tests/project_scoped_api_keys.test.sql` — new, plain runnable
  SQL covering the required security matrix. **Not executed this session**
  (see Findings).

**`Valori-Kernel` (public repo)**

- `python/valoricore/remote.py` — new `class Valori(SyncRemoteClient)`,
  constructor `Valori(url, api_key, ...)` mapping onto
  `SyncRemoteClient(base_url, token, ...)`. Zero new authorization logic;
  100% of existing transport/retry/error-mapping reused.
- `python/valoricore/__init__.py` — exports `Valori`.

**Untouched, per explicit instruction**: `crates/valori-node/src/api_keys.rs`,
`auth_guard_v2`, every other Rust file in `Valori-Kernel`, `valori-ui/backend`
(the Rust provisioning service), `Valori-Kernel/ui` (the separate,
already-known-to-be-drifted copy of the Cloud dashboard UI — out of scope
for this phase, see the earlier session's ui-consolidation discussion).

## Findings

1. **`nodeProxy.ts`'s 404-collapsing bug was real and is now fixed** —
   confirmed by re-reading the file directly; every `verify_api_key()`
   failure (bad key, revoked, expired, wrong scope) previously mapped to
   404 via `ProjectNotFoundError`. Now 401 (`ApiKeyInvalidError`) for
   authentication failures, 403 (`ApiForbiddenError`) for a valid key
   against the wrong project, 404 reserved for the session path.
2. **Project creation was never atomic before this phase** — a bare
   `.from('projects').insert()` followed by a separate, already-
   non-atomic HTTP call to the Rust provisioner. `create_project_with_default_key()`
   makes exactly the required invariant (project row ↔ default key row)
   atomic via a single Postgres function; the provisioning HTTP call
   remains outside the transaction, unchanged in its own failure
   semantics (deploying infrastructure can't meaningfully be part of a SQL
   transaction).
3. **`verify_api_key()`'s parameter list was deliberately kept unchanged**
   rather than matching the P1 design doc's simplified illustrative
   signature (`verify_api_key(full_key, required_scope)`) — the legacy
   org-scoped path genuinely needs `target_project_id` to know which
   project within the org a request means, and there is no other value
   that could supply it. This is documented as a deviation, not a silent
   substitution, per the phase's own "stop and document" instruction —
   the security property (a project-scoped key can never resolve to
   another project) is fully achieved either way; only the RPC's calling
   convention differs from the design doc's illustrative sketch.
4. **The `VALORI_AUTH_TOKEN` defense-in-depth wiring recommended in P1 was
   not implemented.** It was explicitly optional in the P1 design and the
   P2 instructions said to do "the minimum required wiring" for it; given
   the phase's actual required scope, this was left for a follow-up.
   Every Cloud-provisioned `valori-node` remains unauthenticated at the
   node level after this phase — this is not a regression (it was already
   true before P2) but is worth flagging since P2 had a natural
   opportunity to close it and didn't.
5. **`duplicateProject()`'s call into the now-atomic `provisionNewProject()`
   was not separately wired to surface its new Default key to the UI** —
   duplicating a project does create a fresh Default key for the new
   project (the underlying function guarantees this), but the "Duplicate"
   button's caller wasn't inspected/updated to display it. Flagged as a
   minor completeness gap, not a security issue (the key still exists and
   is retrievable from Settings → API Keys).

## Validation

- `npx tsc --noEmit` in `valori-ui/ui` — clean, no errors, run against
  every file this phase touched.
- `npm run build` in `valori-ui/ui` — succeeded, full production build,
  all routes compiled including the modified `dashboard/settings/api-keys`
  and `dashboard` pages.
- `python3 -c "import valoricore; ...`" in `Valori-Kernel/python` — the
  full package (not just the new class) imports cleanly; `Valori` is
  reachable both as `valoricore.Valori` and via `valoricore.remote`, and
  is present in `__all__`.
- **Not run, honestly**: `cargo fmt --check` / `cargo check --workspace` /
  `cargo test --workspace` / the `dependency_direction` and `architecture`
  Rust tests / `cargo clippy -p valori-studio-storage` — this phase made
  **zero changes to any Rust file**, so these are expected to be unaffected,
  but they were not re-run this session to confirm that expectation. The
  SQL migration and its test file were not executed against a live
  database (no instance available; see the architecture doc's Q16 P2
  note). No real end-to-end smoke test (project creation → search →
  cross-project 403 → revoke → 401 → replacement key → expiry) was run
  against a disposable environment — this phase's verification is limited
  to static type-checking, a production build, and manual SQL/code review
  against the actual schema/RPC conventions already in the repo. This is
  stated plainly per the instruction not to report success based on
  compilation alone.

## Follow-ups

1. **Execute `supabase/tests/project_scoped_api_keys.test.sql`** against a
   real (local or staging) Supabase instance and fix whatever it finds —
   the SQL was written carefully against the actual schema but has never
   run.
2. **Run the full real end-to-end smoke test** (P2 §25's 19-step sequence)
   against a disposable Cloud environment — not simulated, not assumed.
3. **Grep sweep for raw keys in logs/telemetry/crash reports** across
   `valori-ui` — flagged unknown in both the audit and this phase, never
   performed.
4. **Decide and implement `VALORI_AUTH_TOKEN` defense-in-depth** (Q12) if
   wanted — provisioner sets a per-project token, Cloud proxy attaches it,
   `valori-node`'s existing `auth_guard_v2` legacy-token path picks it up
   for free (zero node-side code change needed, confirmed by re-reading
   `config.rs`/`auth_guard_v2` this phase).
5. **`duplicateProject()`'s reveal-once UX gap** (Finding 5) — low
   priority, the key exists and works, just isn't shown inline the way a
   fresh "New Project" now is.
6. **`Valori-Kernel/ui`'s duplicate `cloud/settings/api-keys` copy** was
   not touched — it still calls the OLD (pre-migration) `create_api_key`
   signature and will break the moment this migration is applied to a
   shared Supabase project, since the 3-arg overload was dropped. This is
   a real, concrete consequence of the earlier-flagged UI duplication
   problem, now made urgent by this migration — needs the consolidation
   work discussed earlier, or at minimum a matching patch to that copy,
   before this migration reaches any environment `Valori-Kernel/ui`'s
   cloud pages can reach.
7. **SDK packaging decision** — whether `Valori` should live in a
   separately-published `valori` PyPI package (matching
   `from valori import Valori` literally) rather than inside `valoricore`.
8. **SDK ergonomics** — whether a `client.collections.create(...)`-shaped
   sub-resource API is wanted; not built this phase (flat method names
   inherited from `SyncRemoteClient` unchanged).
9. **Local `ApiKeyRecord` convergence** (`expires_at`, real `revoked_at`
   tombstone instead of hard-delete) — still deferred, per the architecture
   doc's Q13, not part of this phase's required scope.
10. **Legacy `project_id = null` row policy** — still unresolved (P1's
    open decision #2): reject, grandfather window, or forced migration UX.
    Nothing in P2 forces this decision (legacy rows keep working exactly
    as before), but it needs an eventual answer.
