# P1 — Project-Scoped API Key Architecture & Implementation Plan

**Status: implemented in P2** — see
[phase-project-api-key-P2.md](phase-project-api-key-P2.md) and the
"P2 note" callouts added throughout
[`docs/architecture/project-api-key-architecture.md`](../architecture/project-api-key-architecture.md).
This document is preserved as originally written (the approved design);
it is not itself updated line-by-line.

## Goal

Design (not implement) a project-scoped API key authentication system for
Valori Cloud, replacing the currently org-scoped `verify_api_key()` model,
per the explicit invariant `ApiKey → exactly one ProjectId`. Chosen
architecture: option (a) — Cloud is the authentication/authorization
authority; `valori-node` is not made a Cloud auth authority in this phase,
but must never trust a client-supplied project id.

## Delivered

- [`docs/reviews/project-api-key-audit.md`](../reviews/project-api-key-audit.md)
  (P0, prior phase, approved) — read-only inventory of every existing
  key/auth/project-identity mechanism across both repos, with verified /
  inferred / unknown markers throughout.
- [`docs/architecture/project-api-key-architecture.md`](../architecture/project-api-key-architecture.md)
  (this phase) — full design answering all 17 required questions: project
  identity, Cloud project mapping, API key schema, key format, automatic
  first-key UX, multi-key support, expiry, revocation, rotation, scopes,
  authenticated request context, Cloud→worker routing, local-key
  convergence stance, Python SDK contract, security test matrix, future
  extensibility.
- This phase report.

**No production code, migrations, or UI were modified.** Per the stated
stop condition, P1 is design-only.

## Findings

Beyond what P0 already surfaced, researching the design itself found two
new concrete facts (not in the P0 audit, verified while writing this
phase):

1. **Cloud-provisioned nodes ship with no `VALORI_AUTH_TOKEN` at all**
   (`valori-ui/backend/apps/api/src/provision/dokploy.rs:175-221` — the
   deploy env list has no auth-related var). Combined with P0's finding
   that `auth_guard_v2` skips its check entirely when no auth is
   configured, this means **every Cloud-provisioned `valori-node` is
   currently fully unauthenticated at the node level** — reachable by
   anyone who learns its `node_url`, bypassing Cloud's key check entirely.
   This is a pre-existing gap, not introduced by this design, but the
   architecture's Q12 answer (a Cloud-issued, project-specific,
   `VALORI_AUTH_TOKEN`-shaped internal credential attached by the proxy)
   closes it as a natural byproduct rather than a special-cased fix.

2. **The existing `rotate_api_key()` RPC's "no grace period" semantics
   directly contradict the requested rotation UX** ("old key remains valid
   until explicitly revoked"). Resolved in the design: the *existing*
   function is reserved for an explicit "regenerate this key's secret"
   action; the *requested* rotation workflow is already fully satisfied by
   the multi-key model (Q6) with no new mechanism needed. This is a
   naming/UX clarification, not a schema gap — worth flagging since it
   could otherwise look like conflicting requirements.

## Validation

Design-only phase — no tests to run, no code to build. "Validation" here
means every claim in the architecture document traces to a specific
file:line citation in either the P0 audit or this phase's own additional
source reads (both `dokploy.rs` and `rotate_api_key`/`create_api_key`'s
full SQL bodies were read directly, not inferred from `PROGRESS.md`
alone).

## Follow-ups (P2+)

Everything in the architecture doc's schema/flow sections is a P2
implementation task. The following are genuinely **unresolved decisions**
that need sign-off before P2 starts, not just "things P2 will figure out":

**Resolved by the P2 instructions' explicit decisions, then implemented:**

1. ~~**Key prefix naming**~~ — resolved: kept `vlk_`, per explicit P2
   instruction.
2. ~~**Legacy org-scoped rows with `project_id = null`**~~ — resolved:
   preserved exactly as-is, never backfilled/narrowed/rejected, per
   explicit P2 instruction. `verify_api_key()` branches on `key_kind` to
   keep their behavior identical to before the migration.
3. ~~**`max_api_keys` limit**~~ — resolved: per-project, per explicit P2
   instruction. Implemented in `create_api_key()`.
4. ~~**`project_id` nullability**~~ — resolved: nullable, no `not null`
   constraint added, per explicit P2 instruction ("do not enforce NOT NULL
   in the first migration").
6. ~~**404 vs 401**~~ — resolved and implemented: `ApiKeyInvalidError`
   (401) and `ApiForbiddenError` (403) added, `nodeProxy.ts` updated.

**Still open after P2** (see phase-project-api-key-P2.md's own Follow-ups
for the current, authoritative list):

5. **SDK surface split** — implemented as `class Valori(SyncRemoteClient)`
   subclass (not a from-scratch parallel client). Whether it should live
   in a separate `valori` PyPI package, and whether
   `client.collections.create(...)`-style ergonomics are wanted, remain
   open.
7. **Raw-key-in-logs/telemetry sweep** — flagged unknown in the security
   section; needs an actual grep pass across `valori-ui` before P2 ships,
   not assumed clean.

P2 should resolve #1–#6 with the user before writing migrations, since
several are irreversible once real keys exist against a chosen schema.
