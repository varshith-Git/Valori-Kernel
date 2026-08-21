# Shared Studio — architectural guardrails

`@valori/studio` (`ui/studio/`) is the **source of truth** for normal Valori
product UI — the customer-facing feature set shared by every host
application: Desktop Local, Desktop Cloud, and Cloud Web. This document is
the guardrail for keeping it that way as the codebase grows.

Background: [Phase C](../phases/) extracted the package; Phase D and Phase E
migrated Desktop Local and Desktop Cloud onto it; Phase F-prep hardened it
for cross-repository distribution ahead of Cloud Web's migration; Phase F
migrated Cloud Web; Phase G/G2/G3 closed the remaining feature gaps (Studio
0.2.0) and finished the rollout; Phase H deleted every resulting orphaned
duplicate (58 files across both repos — the actual count ended up far
larger than the initially-scoped list, once the transitive dead subgraph
behind two abandoned host `ToolsWorkspace` copies was traced and verified).

## Rules

1. **Do not duplicate Studio features in host applications.** If a feature
   already exists as a Studio component (`MetricsView`, `ClusterView`,
   `ToolsWorkspace` and its tabs, etc.), a host consumes that component. It
   does not re-implement the same feature locally, even with small
   variations — variations belong inside the Studio component, driven by
   props/capabilities, or in a runtime adapter (see rule 2).

2. **Host-specific differences belong in runtime adapters or host pages,
   not in Studio.** A `Transport`/`CredentialStore`/`StudioCapabilities`
   implementation (e.g. `LocalRuntime`, `CloudRuntime`) is where a host's
   own routing prefix, credential storage, and capability flags live.
   Genuinely host-unique pages with no Studio equivalent (e.g. Desktop
   Cloud's provisioning-aware project overview) stay as host-specific pages
   — they are not forced into Studio just to raise a migration percentage.

3. **Do not import Tauri or Supabase into Studio.** Studio must build and
   run inside any React host, including ones with no Tauri runtime and no
   Supabase account. `@tauri-apps/*`, `@/lib/native`, `@supabase/*`, and any
   `next/server`-only API (`cookies()`, `headers()`, Next.js server
   components) are forbidden inside `ui/studio/src`. This is enforced by
   scanning source and the dependency tree before every distribution change
   (see `ui/studio/README.md` and the Phase F-prep report for the current
   scan).

4. **Do not put Super Admin / platform-operator features in Studio.**
   Studio is the *customer's* product surface. Platform administration
   (worker fleet, provisioning internals, platform billing, platform
   analytics, platform DR) never becomes a Studio export, regardless of
   which host might technically be able to render it.

5. **New normal product features are implemented once, in Studio.** A new
   feature that every host should eventually offer (a new tool tab, a new
   view, a new hook) is built in `ui/studio/src`, exported from
   `src/index.ts` if it's part of the intended public contract, and then
   wired into each host's pages — never built directly inside a host's
   `src/app/**` tree first "to ship faster" and migrated later.

## When something is genuinely host-specific

Not everything belongs in Studio. A page stays host-specific when it:
- Composes multiple concepts in a way specific to one host's data model
  (e.g. Desktop Cloud's project overview, which mixes provisioning status
  with search — no other host has a "provisioning status" concept).
- Depends on a capability no other host has or will have (Desktop Local's
  local-filesystem WAL/snapshot browser).
- Doesn't yet have Studio parity and migrating would silently drop
  functionality (see each phase's report for the current deferred-route
  list — migration is only declared once behavior is verified equivalent,
  not once it compiles).

## Where the contract is enforced

- `ui/studio/src/index.ts` is the only import surface hosts are meant to
  use — deep imports (`@valori/studio/components/...`) are not part of the
  contract.
- Every export in `index.ts` should have at least one real consumer (a host
  page, or another Studio component) — see the Phase F-prep report for the
  pruning pass that removed `useProvisionerStatus` on exactly this basis.
- `README.md` inside `ui/studio/` documents the versioning policy (PATCH /
  MINOR / MAJOR) so a host can reason about upgrade risk without reading
  the diff.
- **CI enforces rule 1 mechanically (Phase I).** `scripts/check-studio-boundary.mjs`
  scans a host's `src/` tree and fails the build if a filename from
  `scripts/studio-boundary.json` (the canonical Studio view/tab component
  list) exists outside `ui/studio/src/` — wired into `ui-typecheck` in this
  repo's `.github/workflows/ci.yml`, and into `valori-ui`'s own
  `.github/workflows/ui-checks.yml` (that repo consumes Studio as a
  published npm package, so its check has no local Studio tree to exclude).
  It's deliberately dumb — filename matching only, no import-graph analysis
  — so a host wrapper (`CloudToolsWorkspace.tsx`, `LocalFilesPanel.tsx`,
  `ReceiptCard.tsx`, ...) never triggers it; only an exact-filename copy of
  a canonical module does. Shared hooks (`useHealth`, `useProof`, etc.) are
  deliberately **not** in the protected list — see the comment in
  `studio-boundary.json` for why: several host repos still legitimately
  keep local copies consumed directly by genuinely host-specific composite
  pages (Project Overview and friends), and a filename-only check can't
  distinguish that from a real accidental duplicate. Whether those hooks
  should eventually be consolidated onto `@valori/studio`'s own exports is
  an open, deliberate migration question for a future phase — not something
  this guard silently adjudicates either way.
