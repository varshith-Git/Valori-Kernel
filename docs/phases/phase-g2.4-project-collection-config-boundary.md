# Phase G2.4 — Project / Collection Configuration Boundary

## Goal

Correct the Cloud UI's project-creation flow: `dimension`, `metric`, and
`index` are Collection-level configuration, not Project-level. Move that
config off the New Project form and onto Collection creation, without
inventing a fake compatibility layer or redesigning anything else.

## Previous model (assumed by the UI)

```
Project
├── name / region / cluster topology
├── dimension        ← WRONG — one vector config per project
├── index type       ← WRONG
└── (no real Collection-level config UI)
```

## New model (authoritative, confirmed against source before any change)

```
Project
├── organization / ownership
├── billing / plan
├── region
├── cluster / deployment topology (replication: 1 or 3)
└── Collections
      ├── dimension   (permanent)
      ├── metric      (permanent)
      └── index       (mutable via a future rebuild/swap — not built here)
```

---

## Part 1 — Project creation audit (before any change)

Traced `valori-ui`'s New Project path end-to-end, classifying every
`dim`/`index`/`metric`/`max_records` occurrence found:

| Location | Field | Classification |
|---|---|---|
| `ui/src/app/dashboard/CreateProjectDialog.tsx` (form UI) | `dim`, `indexType` state + model-preset buttons + dimension/index-type sections | **SHOULD BE REMOVED** — this is exactly what the task targets |
| `ui/src/app/dashboard/actions.ts` `provisionNewProject()` | `dim`/`index` params, RPC call, fallback insert | **LEGACY, MUST PARTIALLY REMAIN** — see Part 4/9 below; the params still exist internally to satisfy NOT NULL DB columns, but are no longer user-chosen |
| `ui/src/app/dashboard/actions.ts` `duplicateProject()` | reads `source.dim`/`source.index_type`, forwards to `provisionNewProject` | **LEGACY, MUST REMAIN** — not user-facing config, just carries forward a stored (now-inert) value; no UI exposes it as a choice |
| `backend/apps/api/src/main.rs` `ProvisionBody { dim, index, max_records }` | `#[serde(default = ...)]` on all three | **LEGACY, MUST REMAIN AS-IS** — already optional/defaulted; the Rust API layer was not modified this phase (out of scope — see Part 4) |
| `backend/apps/api/src/provision/traits.rs` `DeployRequest { dim, index, ... }` | feeds `DockerProvisioner::build_env()` | **LEGACY, INERT** — see Part 5 |
| `backend/apps/api/src/provision/docker.rs` `build_env()` | sets `VALORI_DIM=`, `VALORI_INDEX=` container env vars | **LEGACY, INERT — confirmed the deployed node no longer reads either** (see Part 5) |
| `supabase/migrations/20260723000000_project_vector_config.sql` `projects.dim`/`.index_type`/`.max_records` | `NOT NULL DEFAULT ...` columns | **LEGACY, MUST REMAIN** — not migrated/dropped this phase (Part 9 — explicit "do not mutate production data" instruction) |
| `ui/src/app/dashboard/page.tsx` project list table | `{p.dim}d · {p.index_type}` "Vector config" column | **SHOULD BE REMOVED** — displays a per-project value that's no longer coherent once a project can hold many differently-configured collections |
| `ui/src/lib/hooks/useHealth.ts` / `types/valori.ts` `HealthResponse.dim`/`.index` | read from `GET /health` | **LEGACY, PARTIALLY DEAD** — see Part 8 |
| `ui/src/app/dashboard/projects/[id]/ProjectWorkspace.tsx` | "Dimension"/"Index" `MetricCard`s, `{dim}D` query-vector hint | **SHOULD BE REMOVED** — see Part 8 |
| `ui/src/app/api/projects/[id]/metrics/ping/route.ts` | `const dim = health.dim ?? 128` | **LEGACY, PRE-EXISTING, NOT TOUCHED** — a latency-benchmark utility that already always falls back to a hardcoded 128 in the real (standalone) deployment path; flagged, not fixed — see Known limitations |
| `ui/src/lib/server/quota.ts` `dimension: QuotaDimension` | quota-check axis name (`'collections'`/`'searches'`), unrelated to vector dimension | **MUST REMAIN — false positive**, not vector config at all |
| `ui/src/lib/server/embed.ts`/`reranker.ts` `.index` | array index in embedding/reranker result arrays | **MUST REMAIN — false positive**, not vector-config `index` |
| `ui/src/lib/hooks/useEmbeddingConfig.ts` `MODEL_DIMS`/`PROVIDER_DEFAULTS[...].dim` | embedding-model output dimension, for the ingest/embed tool | **MUST REMAIN — unrelated feature**, not project/collection creation |

No API keys or SDK code in this repo assume a project-level dimension —
the Python SDK's own contract (documented in `CLAUDE.md`) already requires
`dimension`/`metric` on every `create_collection()` call.

---

## Part 2 — Collection creation audit (before any change)

Confirmed against the actual node source (`Valori-Kernel`, not assumed):

- `crates/valori-node/src/api.rs`'s `CreateCollectionRequest { name, dimension: Option<u32>, metric: Option<String>, index: Option<String> }` — `dimension`/`metric` are `Option` in Rust only so a missing value reaches `parse_collection_config` and gets a named 400, not a generic deserialize failure; both are **contractually required** (`schema(required = true)`). `index` is genuinely optional — omitting it means exact search, a first-class state, not a gap.
- `crates/valori-node/tests/collections.rs`'s `missing_dimension_rejected`/`missing_metric_rejected` tests prove this is enforced today, and explicitly assert the error never mentions the removed `VALORI_DIM` env var.
- `crates/valori-engine/src/engine.rs`'s `create_collection_with_config()` validates `dimension` only against `MAX_DIM` (a hard cap) and stores it per-namespace via `KernelEvent::ConfigureNamespace { namespace_id, dim, metric, index_kind }` — genuinely independent per collection, not a single process-wide value.
- `POST /v1/namespaces` (route confirmed in `crates/valori-node/src/server.rs`) is the real endpoint.

**Found a real, pre-existing mismatch, not invented by this phase**:
`ui/src/lib/hooks/useCollections.ts`'s `create(name)` sent **only** `{name}`
to this endpoint — no dimension, no metric. Given the node's own
`missing_dimension_rejected` test, **every collection-creation attempt
through the old UI would have failed with a 400** before this phase. This
is exactly the gap Part 6 of the brief asks to close — not a different bug
to route around.

Collection semantics themselves were **not** touched, redesigned, or rolled
back — only the UI that was missing entirely for them.

---

## Part 3/6 — UI changes

**`ui/src/app/dashboard/CreateProjectDialog.tsx`** — removed the Dimension
section (model presets + dropdown) and the Index type section entirely.
Also moved off a hand-rolled `fixed inset-0` modal onto the shared
`Dialog`/`DialogContent`/`DialogHeader`/`DialogTitle`/`DialogFooter`,
`Input`, and `Button` components (`src/components/ui/*`) — see **Shared UI**
below. Remaining form: Name, Region, Cluster (Single node / 3-node
cluster). Added one line of copy: *"Dimension, metric, and index are chosen
per Collection, after the project is created."*

**`ui/src/app/dashboard/projects/[id]/CollectionsPanel.tsx`** — this is now
where Dimension/Metric/Index are chosen. Creation moved from an inline
one-field row to a `Dialog` with: Name (`Input`), Dimension (`<select>` —
see Shared UI gap below), Metric (`Button` toggle — currently one option,
`squared_l2`, matching the node's real contract), Index (`Button` toggle,
5 options — Auto/Brute/HNSW/IVF/BQ). Existing collections now display their
real config (`{dimension}D`, index badge, live record count) via `Badge`,
sourced from the node's own `ListCollectionsResponse`.

**`ui/src/lib/hooks/useCollections.ts`** — `create()` now takes
`{name, dimension, metric, index?}` and sends the full body; `collections`
changed from `string[]` to the full `CollectionInfo[]` shape (name, id,
dimension?, metric?, index?, record_count?, max_records?).

**`ui/src/types/valori.ts`** — `Collection` interface extended to match the
real `CollectionInfo` response shape.

---

## Shared UI

Per the explicit shared-UI requirement: inspected `src/components/ui/*`
(17 files, `@base-ui/react` + `cva`-based, no separate npm package) before
writing anything.

```
SHARED UI COMPONENTS FOUND:
Dialog/DialogContent/DialogHeader/DialogTitle/DialogFooter → src/components/ui/dialog.tsx
Input          → src/components/ui/input.tsx
Button (cva variants: default/outline/secondary/ghost/destructive/link) → src/components/ui/button.tsx
Badge (cva variants) → src/components/ui/badge.tsx
Skeleton       → src/components/ui/skeleton.tsx
Card, EmptyState, StatusBadge, StatusPanel, MetricCard, Tabs, Separator,
Table, PageHeader, CopyBtn, MiniChart, Toaster, Textarea → src/components/ui/*

SHARED UI COMPONENTS REUSED:
Dialog/DialogContent/DialogHeader/DialogTitle/DialogFooter — both New
  Project and Create Collection modals now use these, replacing
  CreateProjectDialog.tsx's previous hand-rolled `fixed inset-0 bg-black/60`
  div, which was itself an undetected duplicate of Dialog.
Input   — every text/number field in both forms.
Button  — every button, including the "Cluster"/"Metric"/"Index" toggle
  groups (variant={active ? 'default' : 'outline'} instead of a hand-rolled
  active/inactive className string).
Badge   — per-collection dimension/index display in CollectionsPanel.
Skeleton — CollectionsPanel's loading placeholder, replacing a hand-rolled
  `animate-pulse` div (same duplicate-primitive issue, fixed while in the
  file for this phase anyway).

NEW LOCAL COMPONENTS REQUIRED:
NONE. Confirmed gap: no Select or Label component exists anywhere in the
shared set. Region (pre-existing) and the new Dimension field keep a plain
native <select>, styled by reusing Input's own className tokens (border/
radius/bg/focus-ring), not a new component file. Labels keep the existing
inline `<label className="text-xs text-muted-foreground uppercase
tracking-widest">` pattern already repeated multiple times in
CreateProjectDialog before this phase. Neither is a new invention — both
are the pre-existing local idiom, applied consistently.
```

Verified after implementation: no new generic primitive files were added
under `src/components/`; `git status` shows only the specific feature files
listed below modified.

---

## Part 4 — Project API contract

**Backend was inspected, not modified.** `ProvisionBody` (`backend/apps/api/
src/main.rs`) still has `dim`/`index`/`max_records`, but all three carry
`#[serde(default = ...)]` — the request already tolerates omission, and the
frontend's `provisionNewProject()` request body (`{region, replication}`
after this phase — `dim`/`index` are no longer sent by the New Project
form) will simply hit those defaults server-side. This is **not** the
"invent a fake compatibility default to hide an architectural bug" case the
brief warns against — the defaults already existed before this phase, at
every layer (Rust `ProvisionBody`, the `create_project_with_default_key`
Postgres RPC's `p_project_dim smallint default 768` / `p_project_index_type
text default 'brute'`, and the `projects.dim`/`.index_type` `NOT NULL
DEFAULT` columns). Nothing new was added to make omission safe; it already
was.

`actions.ts`'s `provisionNewProject()` keeps its own `dim: number = 768,
index: string = 'brute'` parameter defaults internally, used only to (a)
satisfy the NOT NULL DB columns via the RPC/fallback insert and (b) still
build a syntactically valid `ProvisionBody` for the Rust API call. The
public `createProject()` function — the one the New Project form actually
calls — no longer accepts `dim`/`index` parameters at all.

---

## Part 5 — Provisioning

Traced `DeployRequest.dim`/`.index` (`backend/apps/api/src/provision/
traits.rs`) through to `DockerProvisioner::build_env()` (`backend/apps/api/
src/provision/docker.rs`), which still sets `VALORI_DIM=`/`VALORI_INDEX=`
as container environment variables on every deploy.

**Checked whether the deployed node still reads either — it does not.**
`crates/valori-node/src/config.rs`'s `NodeConfig` struct has **no `dim`
field and no `index` field at all** — grepping the entire `Valori-Kernel`
repo for `VALORI_DIM` turns up zero references in any `.rs` source file
(only a test asserting it was *removed*, `crates/valori-node/tests/
collections.rs:256-257`, and doc comments in READMEs/other crates
referencing the old contract). `crates/valori-daemon/src/domain_adapter.rs:
25` independently documents the same fact: *"`dim`, `index` — Removed from
`valori_domain::Project` entirely (collection-index-lifecycle phase) —
vector configuration is Collection-scoped, not Project-scoped."*

**Classification: (A) genuinely obsolete.** `VALORI_DIM`/`VALORI_INDEX` are
inert environment variables on the node side — setting them has no
observable effect on the deployed container. They were **not removed from
`DockerProvisioner`/`DeployRequest`/`ProvisionBody`** in this phase — that
is a Rust backend change, explicitly out of scope for a UI phase, and
removing a still-defaulted, harmless (if pointless) field from a live
provisioning payload without being asked carries real risk with no
verification path available here. This is flagged as a clean, low-risk
follow-up (see Next Phase), not implemented.

**This is not a Part 14 STOP condition.** The brief's stop condition is
"provisioning still requires Project-level vector configuration" — it does
not; the values are accepted-but-ignored, which is different from required.

---

## Part 7 — Immutability

No change needed or made. Dimension/metric were already immutable in
principle (the node has no edit endpoint for either), and this phase adds
no edit UI for them. Index rebuild/swap was explicitly out of scope and
none was added — `CollectionsPanel.tsx` only ever sets index once, at
creation.

## Part 8 — Project settings / detail pages

Searched every settings page (`ui/src/app/dashboard/settings/**`) — none
reference `dim`/`index`/`metric`; nothing to remove there. The only real
project-level display was on `ProjectWorkspace.tsx` (project detail
overview) and `dashboard/page.tsx` (project list):

- `ProjectWorkspace.tsx` — removed the "Dimension" and "Index" `MetricCard`s
  and the `{dim}D` hint on the "Query vector" label. These were already
  effectively dead in the real deployment path: `crates/valori-node/src/
  server.rs`'s standalone `health_check` hardcodes `dim: None`, and
  `HealthResponse` has no `index` field at all — confirmed by reading the
  Rust struct directly, not assumed. Kept `dim` on the `useHealth.ts` hook
  itself (not deleted) since it's a real, if legacy, cluster-mode-only wire
  field per the kernel's own doc comment (*"kept... because a live consumer
  depends on it"*, referring to this exact hook) — removed only the display
  and the genuinely-nonexistent `index` field.
- `dashboard/page.tsx` — removed the "Vector config" table column
  (`{p.dim}d · {p.index_type}`). Replacing it with something else (e.g. a
  live collection count) would need a new per-project query and was judged
  out of scope ("do not redesign the entire dashboard") — removal, not
  addition, is the minimal correct fix.

Legitimate project-level settings verified to remain untouched: name,
region, deployment/cluster topology, billing, project status, API keys.

## Part 9 — Old data / compatibility

**No production data touched.** `public.projects.dim`/`.index_type`/
`.max_records` remain `NOT NULL DEFAULT`-backed columns, still populated by
every new project (via the still-present defaults), still read by
`duplicateProject()`. Not migrated, not dropped, not made nullable in this
phase. A future phase could reasonably deprecate them once/if the Rust
`ProvisionBody`/`DeployRequest` fields are also removed (see Next Phase) —
that is a coordinated backend + schema change, not a UI-only one, and is
explicitly deferred.

## Part 11 — Cleanup sweep

Final grep for `project.dimension` / `project.dim` / `project.index` /
`project.index_type` / `project.metric` across `ui/src` after
implementation found only: `actions.ts`'s internal `provisionNewProject`
defaults and `duplicateProject`'s source-row read (both classified LEGACY/
MUST REMAIN above), and unrelated false positives (`quota.ts`'s
`QuotaDimension`, `embed.ts`/`reranker.ts`'s array `.index`,
`useEmbeddingConfig.ts`'s embedding-model dimension). No zod schemas exist
for project creation. No stale dead code left behind.

---

## Part 13 — Tests / verification

```
$ npx tsc --noEmit                     → clean, 0 errors
$ npx eslint <every changed file>      → clean, 0 warnings/errors
$ npm run build                        → ✓ Compiled successfully in 7.9s
```

A live dev server (`valori-cloud-dashboard`, port 3002) was started and
confirmed to boot cleanly (landing page renders, no console errors). Direct
navigation to `/dashboard` correctly redirected to `/login` (real Supabase
auth, no credentials available in this session — not bypassed). A temporary
scratch route rendering `CreateProjectDialog`/`CollectionsPanel` directly
(to sidestep auth) was also blocked — `src/middleware.ts` intercepts every
non-asset path for session refresh, redirecting unrecognized routes the
same way. The scratch file was deleted immediately (`git status` confirms
no trace left). Given that, the checklist below is verified against the
diffed source + the static build output, not a live authenticated render:

1. New Project form contains no dimension field — confirmed, section
   deleted from `CreateProjectDialog.tsx`.
2. New Project form contains no index selector — confirmed, deleted.
3. New Project request contains no dimension/index/metric — confirmed,
   `createProject(orgId, name, region, replication)` — 4 args only, no
   dim/index params exist on the public function signature anymore.
4. Collection creation contains dimension/metric/index — confirmed, new
   `Dialog` form in `CollectionsPanel.tsx` collects all three, sent via
   `useCollections.ts`'s `create()`.
5. Existing project route still works — no route changed, no breaking
   change to `/dashboard/projects/[id]`; `ProjectWorkspace.tsx`'s edit is
   subtractive-only (two `MetricCard`s + one hint removed).
6. Existing collection route still works — `/api/projects/[id]/namespaces`
   proxy route unchanged (still a transparent passthrough), only its
   comment was corrected.
7. Production build passes — confirmed above.
8. No stale Project-level form fields remain — confirmed by the Part 11
   sweep.

No frontend test suite exists for these components (no `*.test.tsx` files
found alongside them) — none was added, per "do not introduce a new test
framework."

---

## Part 14 — Stop conditions

None were hit. Specifically checked and cleared:

- Project creation backend still requires dimension/index? **No** —
  defaulted at every layer (Rust struct, Postgres RPC, DB column), verified
  before writing any code.
- Provisioning still requires Project-level vector configuration? **No** —
  the values are accepted-but-ignored by the deployed node (VALORI_DIM/
  VALORI_INDEX were already removed from `NodeConfig`).
- Collection evolution incomplete? **No** — `dimension`/`metric` are
  contractually required per-collection today, enforced by real tests.
- Existing Projects depend on Project-level dim/index for runtime
  correctness? **No** — confirmed the node ignores those env vars entirely;
  nothing in the runtime path reads `projects.dim`/`.index_type` except the
  now-removed display code and `duplicateProject`'s internal carry-forward.
- Removing these fields would break current production Projects? **No
  UI-level removal breaks anything** — no destructive DB change was made,
  and existing projects' stored `dim`/`index_type` rows are untouched.
- Cluster topology encoded together with vector config? **No** —
  `replication` (1 vs 3) is a wholly separate `ProvisionBody`/`DeployRequest`
  field from `dim`/`index`, confirmed by reading both structs; kept exactly
  as the task instructed.

---

## Files changed

```
ui/src/app/dashboard/CreateProjectDialog.tsx          — removed dim/index UI, moved to shared Dialog/Input/Button
ui/src/app/dashboard/actions.ts                       — createProject() no longer accepts dim/index
ui/src/app/dashboard/page.tsx                         — removed "Vector config" column
ui/src/app/dashboard/projects/[id]/CollectionsPanel.tsx — NEW: dimension/metric/index creation form + config display
ui/src/app/dashboard/projects/[id]/ProjectWorkspace.tsx — removed Dimension/Index MetricCards + dead hint
ui/src/lib/dimensions.ts                              — repurposed for Collection creation; added METRICS/DEFAULT_METRIC
ui/src/lib/hooks/useCollections.ts                    — create() now takes full vector config; typed CollectionInfo
ui/src/lib/hooks/useHealth.ts                         — removed dead `index` field
ui/src/types/valori.ts                                — extended Collection; removed dead HealthResponse.index
ui/src/app/api/projects/[id]/namespaces/route.ts      — corrected stale doc comment only, no behavior change
```

No `Valori-Kernel` (node/kernel) source was touched — this phase is UI-only
by design.

## Known limitations

1. `ProvisionBody`/`DeployRequest`/`DockerProvisioner.build_env()` still
   carry `dim`/`index` fields and still set inert `VALORI_DIM`/
   `VALORI_INDEX` container env vars. Harmless (the node ignores them) but
   dead weight — a clean Rust-side follow-up, not done here (out of scope,
   and this session cannot verify a live-provisioning change against real
   infrastructure — see the G2.3.1 phase docs for why).
2. `public.projects.dim`/`.index_type`/`.max_records` columns remain,
   unmigrated, per Part 9's explicit instruction.
3. `ui/src/app/api/projects/[id]/metrics/ping/route.ts`'s latency-ping tool
   still falls back to a hardcoded 128-dim synthetic query vector — a
   pre-existing, now-more-visible consequence of `health.dim` always being
   null in standalone mode. Flagged, not fixed (unrelated feature, not a
   creation-flow config boundary issue).
4. The Query/Search tool on the project workspace page still searches
   without letting the user pick which collection to query, and (per Phase
   3.3) there's no implicit "default" collection anymore for it to fall
   back to. Pre-existing, unrelated to this phase's scope, worth a
   dedicated follow-up.

## Next phase

If the dead `VALORI_DIM`/`VALORI_INDEX` container env vars are worth
cleaning up: remove `dim`/`index` from `DeployRequest`/`ProvisionBody`/
`build_env()` on the Rust side, live-verify against a real provisioned
node (needs the same infrastructure access this session has repeatedly
lacked — see the G2.3.1 phase docs), then drop the now-fully-unused
`projects.dim`/`.index_type` columns in a coordinated schema migration.

---

## FINAL VERDICT

```
PROJECT DIMENSION FIELD:
REMOVED

PROJECT INDEX FIELD:
REMOVED

PROJECT METRIC FIELD:
REMOVED (was never present as a project-level field — Collection is its first home)

COLLECTION DIMENSION:
ACTIVE

COLLECTION METRIC:
ACTIVE

COLLECTION INDEX:
ACTIVE

PROJECT API CONTRACT:
CORRECT — dim/index/max_records already defaulted at every layer (Rust struct, Postgres RPC, DB column); frontend no longer sends them; nothing broke

COLLECTION API CONTRACT:
CORRECT — dimension/metric required, index optional, matching crates/valori-node/src/api.rs exactly; fixed a pre-existing bug where the UI never sent them at all

BACKWARD COMPATIBILITY:
PASS — no destructive DB change, no route removed, existing projects/collections/API keys/provisioning untouched

TYPESCRIPT:
PASS

ESLINT:
PASS

BUILD:
PASS

FILES CHANGED:
ui/src/app/dashboard/CreateProjectDialog.tsx
ui/src/app/dashboard/actions.ts
ui/src/app/dashboard/page.tsx
ui/src/app/dashboard/projects/[id]/CollectionsPanel.tsx
ui/src/app/dashboard/projects/[id]/ProjectWorkspace.tsx
ui/src/lib/dimensions.ts
ui/src/lib/hooks/useCollections.ts
ui/src/lib/hooks/useHealth.ts
ui/src/types/valori.ts
ui/src/app/api/projects/[id]/namespaces/route.ts

BLOCKERS:
none
```

STOP.
