# Valori Current Architecture Audit — Project, Collection & Lifecycle

**FACTUAL AUDIT ONLY. No code was modified. No architecture was redesigned.
No bugs were fixed. No schemas or APIs were changed.** Every claim below
is either code-verified with an exact file:line citation, or explicitly
marked **"Not established from current code."** Where documentation
(phase docs, comments) disagreed with or went beyond what source code
proves, that discrepancy is flagged rather than resolved in the
documentation's favor.

This audit spans **two separate repositories** on disk:
- `/Users/as-mac-0272/Desktop/sass/Valori-Kernel` (this repo) — kernel,
  engine, node, SDK, CLI, MCP, and a local/desktop control-plane `ui/`.
- `/Users/as-mac-0272/Desktop/sass/valori-ui` (a sibling repo, **not**
  checked into or vendored in Valori-Kernel) — the actual multi-tenant
  Cloud SaaS product: Next.js frontend, Supabase/Postgres schema, and a
  Rust provisioning backend. Referenced only by relative path default in
  `e2e/cloud/docker-compose.yml:46-47,137`
  (`${VALORI_UI_PATH:-../../../valori-ui}`) — confirmed absent from this
  repo's own source tree.

**This single fact reshapes the whole audit**: "Project" in the sense the
Cloud UI's user creates (organization, region, plan, provisioning) is
entirely defined in the second repo. Everything downstream of that —
Kernel, Engine, Node — has **no concept of "Project" at all**. It only
knows `NamespaceId`/"Collection." The two halves connect only at the HTTP
boundary: the Cloud control plane deploys a `valori-node` process and
talks to it over the network: it never links against or imports any
`valori-kernel`/`valori-engine`/`valori-node` code.

---

## 0. Rules followed

Every "implemented / partially implemented / declared but unused /
derived / persisted / runtime-only / planned-but-not-implemented"
classification below is backed by a citation, not inferred from a name or
a doc comment. Several genuinely dormant structures were found (§6.4,
§7) — these are called out explicitly as dormant, not silently omitted or
silently assumed active.

---

## 1. Repository structure (actual, not aspirational)

```
Valori (two repositories)
│
├── Valori-Kernel/                          (this repo)
│   ├── crates/                             22 Rust crates (table below)
│   ├── ui/                                 Next.js "ui" app, port 3001 —
│   │                                        LOCAL/DESKTOP control surface,
│   │                                        NOT the Cloud SaaS UI (see §1.1)
│   ├── python/                             PyO3-backed SDK "valoricore"
│   ├── desktop/                            Tauri desktop shell
│   ├── e2e/cloud/                          docker-compose harness that
│   │                                        assembles THIS repo + the
│   │                                        external valori-ui repo
│   ├── embedded/                           Cortex-M firmware crate,
│   │                                        excluded from the workspace —
│   │                                        proves valori-kernel runs
│   │                                        no_std on a microcontroller
│   ├── terraform/{aws,azure}/              IaC: S3/IAM/CloudWatch (AWS),
│   │                                        AKS/storage/Key Vault (Azure)
│   ├── deploy/helm/valori/                 Helm chart
│   └── docs/, rfcs/, benchmarks/, examples/, tests/, ...
│
└── valori-ui/                              (sibling repo, NOT in this tree)
    ├── ui/                                 Next.js Cloud SaaS frontend
    ├── backend/apps/api/                   Rust provisioning service
    ├── supabase/migrations/                Postgres schema (Next.js side)
    └── backend/migrations/                 Postgres schema (`infra.*`,
                                             owned by the Rust service)
```

### 1.1 — `ui/` (in THIS repo) vs. the Cloud SaaS UI: confirmed different apps

- `ui/package.json:2,9` — name `"ui"`, dev script `next dev -p 3001`.
- `ui/src/app/` routes: `projects`, `projects/[name]`, `launch`, `cluster`,
  `playground`, `operations`, `snapshots`, `logs`, `metrics`, plus a
  smaller `cloud/{archived,projects,settings}` sub-area for *connecting
  to* a Cloud project. This is a **local/desktop control surface**.
- `ui/src/lib/server/daemon.ts:1-13`: *"Thin 1:1 wrapper around the
  valori-daemon HTTP API (`crates/valori-daemon/src/http.rs`). This is
  the ONLY place in `ui/` that knows the daemon's URL or wire shape."*
  Default `http://127.0.0.1:8080`, overridable by `VALORI_DAEMON_URL`.
- Consumers: `ui/src/app/api/projects/route.ts`,
  `ui/src/app/api/projects/[name]/{route,status,open,close}.ts` — all
  call through `daemon.ts`, i.e. through `crates/valori-daemon`, a
  process running **locally on the user's machine**, not a multi-tenant
  Postgres-backed service.

### 1.2 — Crate table (spot-checked against actual `lib.rs`, not copied from CLAUDE.md verbatim)

| Crate | no_std/std | Purpose (grounded in code) |
|---|---|---|
| `valori-core` | std, minimal deps | Shared IDs/enums/errors/traits; `NamespaceId`, `CollectionId` alias live here (`valori-core/src/id.rs:34-51`) |
| `valori-domain` | std | Cross-boundary identity vocabulary; holds its own `Project`-family structs (`ProjectName`, `ProjectTopology`, `Project` — `valori-domain/src/project.rs:115,312,442`) |
| `valori-storage` | std | WAL, event log, object store, crash recovery |
| `valori-state` | std | State lifecycle orchestration (bootstrap/recovery) |
| `valori-metadata` | std | Control-plane persistence (redb); has its own `Project` struct and a `Collection`/`CollectionRegistry` pair — **confirmed dormant**, see §6.4 |
| `valori-planner` | std | Operation → `ExecutionGraph` DAG planning/caching |
| `valori-effect` | std-only by design | Effect bus; explicitly forbidden as a dep of `valori-kernel`/`valori-core` |
| `valori-kernel` | **no_std** (feature-gated std) | Deterministic core; file carries an "ARCHITECTURAL INVARIANT" comment forbidding `use std::` (`valori-kernel/src/lib.rs:1-5`) — spot-check confirms no drift |
| `valori-wire` | std | On-disk event-log wire format |
| `valori-node` | std | HTTP server(s) — `server.rs`/`cluster_server.rs` |
| `valori-ffi` | std | PyO3 extension module (embedded Python SDK backend) |
| `valori-verify` | std | Offline event-log verifier |
| `valori-cli` | std | `valori` binary — cluster wizard, forensic inspection (see §1.4) |
| `valori-consensus` | std | openraft Raft layer |
| `valori-mcp` | std | MCP server, 7 tools (see §1.3) |
| `valori-search` | std | Post-retrieval rerank primitives (decay/BM25/metadata filter) |
| `valori-index` | std | BruteForce/HNSW/IVF/BQ vector indexes |
| `valori-rag` | std | GraphRAG, Tree-RAG, Community layer |
| `valori-ingest` | std | chunk → embed → write pipeline |
| `valori-engine` | std | `Engine` (standalone), extracted from `valori-node` |
| `valori-daemon` | std | Owns LOCAL project/workspace lifecycle, supervises `valori-node` processes; doc comment: "the Rust successor to the TypeScript process manager in `ui/src/lib/server/`" |
| `valori-models` | std | Model provider/download/verify management |
| `valori-studio-storage` | std | Studio-local redb store at `~/.valori/studio.redb`, explicitly "Not the project data store" |

### 1.3 — MCP: no collection-management tool

`crates/valori-mcp/src/tools.rs:14-21` defines exactly 7 tools:
`memory_write`, `memory_recall`, `memory_graph_recall` (all three accept
an *optional* `"collection"` string parameter, defaulting to `"default"`
server-side), `memory_why`, `memory_timeline`, `memory_forget`,
`memory_fork` (no collection parameter on these four). **There is no
`collection_create`/`collection_list`/`collection_drop` MCP tool.**

### 1.4 — CLI: no collection-management subcommand, and a third "project" meaning

`crates/valori-cli/src/main.rs:21-144` — `Setup`, `Inspect`, `Verify`,
`Timeline`, `ReplayQuery`, `Diff`, `Cluster{Status,Health,AddNode,
RemoveNode,Upgrade}`, `Import{Qdrant,Jsonl}`. None manage
collections/namespaces. The CLI's own "project" vocabulary
(`crates/valori-cli/src/commands/wizard.rs:5,32`, `struct SavedProject`,
backed by `~/.valori/projects.json`) is a **fourth** distinct concept — a
locally-saved cluster-launch config the wizard uses to resume a session,
unrelated to `valori-metadata::Project`, `valori-domain::Project`,
`valori-daemon::Project`, or the Cloud SaaS `projects` table.

**Net finding, stated plainly**: this repo alone contains at least four
non-identical `Project`/project-adjacent representations
(`valori-metadata::Project`, `valori-domain::Project`,
`valori-daemon::Project`, CLI's `SavedProject`), none of which is the
Cloud SaaS `projects` Postgres table audited in §2-5, which is a fifth,
entirely separate representation in a different repository.

---

## 2. What is a Project today? (traced in `valori-ui`)

**Full chain, traced end to end**:

```
UI (CreateProjectDialog.tsx:39-96)
  ↓  Server Action
dashboard/actions.ts::createProject() (line 122-131)
  ↓  Supabase RPC (SECURITY DEFINER, atomic row+key insert)
create_project_with_default_key
  (supabase/migrations/20260810000000_project_scoped_api_keys.sql:197-258)
  — inserts ONE `projects` row + ONE "Default" `api_keys` row, in one
    Postgres transaction. Comment (lines 177-191): this atomicity does
    NOT extend to provisioning — deploying a node is a SEPARATE, later,
    non-atomic step.
  ↓  HTTP POST (Server Action → Rust backend)
POST /v1/projects/:id/provision  (backend/apps/api/src/main.rs:385,860-962)
  1. check_quota_and_entitlements — plan/limit gate
  2. resolve_worker_auth_token — generates/reads projects.worker_auth_token
     directly via sqlx (bypasses PostgREST/RLS entirely)
  3. worker_service.find_available(region, worker_class, replication)
     — picks host(s) from infra.hosts (Rust's OWN Postgres schema,
       distinct from Supabase's projects table)
  4. provisioner.deploy(host, DeployRequest) — the actual container
     creation (Docker Engine API, Dokploy API, or Mock — see §5)
  5. db::instance::insert — infra.instances row (Rust's own schema)
  6. worker_service.reserve_slot — infra.hosts.used_slots += 1
  ↓  PATCH back into Supabase (service-role key, bypasses RLS)
supabase.mark_project_active(id, node_url)  (backend/apps/api/src/supabase.rs:106-134)
  — sets projects.node_url + projects.status = 'active'
```

**What does a Project actually represent?** — answered directly from the
evidence above, not chosen a priori:

- **Is a project a database?** No single dedicated database — it is a
  **Postgres row** (`public.projects` in Supabase) plus a **separate
  Postgres row** (`infra.instances` in the Rust backend's own schema, a
  *different* database/schema than Supabase's) tracking the deployed
  container(s).
- **Is a project a worker?** Not identically — a project can have
  `replication` ∈ {1,3} instances (`20260721120000_schema.sql:59`,
  `CreateProjectDialog.tsx:250-268`), i.e. potentially *multiple* workers
  per project. "Worker" (§5) is a strictly smaller concept than "Project."
- **Is a project a namespace?** No — the kernel-level `NamespaceId`
  ("Collection," §6) exists entirely *inside* a deployed node and has zero
  representation in the Cloud schema. A Project can (and typically does)
  contain many Collections, but nothing in `valori-ui`'s schema stores or
  tracks which Collections exist inside a given project's node — that
  information lives only inside the running `valori-node` process itself.
- **Is a project a deployment?** Closest single-word answer, but still
  incomplete — a project is the *Cloud-side identity and metadata* row
  that a deployment (`infra.instances`) is provisioned *for*; the two are
  linked by `project_id` but are separate rows in separate schemas,
  created in separate, non-atomic steps (per the RPC's own comment above).
- **Is a project merely Cloud metadata?** No — it has a real, causally
  necessary side effect (an actual container gets created), it's simply
  that the metadata row and the compute resource are two different
  objects, connected by an ID and an eventually-consistent PATCH-back,
  not a single atomic entity.

**Verdict, code-supported**: a Project is **a Postgres row that is the
authorization/billing/identity anchor for zero-or-more provisioned
`valori-node` container instances**, tracked in a second, separate schema
(`infra.instances`) owned by a different service (the Rust backend) than
the one users' RLS policies govern (Supabase). Nothing forces these two
representations to stay in sync beyond the specific write paths audited
in §4 — there is no foreign-key or transactional link between them (they
live in different databases entirely, per `backend/migrations/` vs.
`supabase/migrations/`).

---

## 3. Project database schema (Supabase `public.projects`, current state after all migrations)

| Field | Type | Nullable | Default | FK | Mutable? | Who writes it | Evidence |
|---|---|---|---|---|---|---|---|
| `id` | uuid | not null | `gen_random_uuid()` | PK | No | insert only | `20260721120000_schema.sql:50` |
| `org_id` | uuid | not null | — | `organizations(id)` cascade | **No UPDATE path found anywhere** | insert only (RPC) | `20260721120000_schema.sql:51`; RPC insert `20260810000000_project_scoped_api_keys.sql:239-242` |
| `name` | text | not null | — | — | **Yes** — user-editable | `PATCH /api/projects/[id]` → `ui/src/app/api/projects/[id]/route.ts:25` |
| `slug` | text | not null | — | — | No UPDATE path found | insert only | (unique constraint on `(org_id,slug)` was later **dropped**, see below) |
| `region` | text | not null | — | — | No UPDATE path found | insert only, from UI-selected value | `20260721120000_schema.sql:54` |
| `status` | `project_status` enum | not null | `'creating'` | — | **Yes**, mixed UI + system writers | see §4's transition table | `20260721120000_schema.sql:55` |
| `node_url` | text | nullable | — | — | System-only (not in `authenticated` update grant) | `backend/apps/api/src/supabase.rs:106-134` only | `20260721120000_schema.sql:56` |
| `replication` | smallint | not null | `1` | — | No UPDATE path found | insert only | `20260721120000_schema.sql:59` |
| `created_by` | uuid | not null | — | `auth.users(id)` | No (audit column) | insert only | `20260721120000_schema.sql:60` |
| `created_at` | timestamptz | not null | `now()` | — | No | insert only | `20260721120000_schema.sql:61` |
| `updated_at` | timestamptz | not null | `now()` | — | System, via `projects_set_updated_at` BEFORE UPDATE trigger | trigger, not app code | referenced `20260811000000_worker_auth_token.sql:69` |
| `last_active_at` | timestamptz | not null | `now()` | — | System-controlled, updated on every data-plane call | `ui/src/lib/server/project.ts:73` | `20260722170000_project_suspension.sql:17` |
| `dim` | smallint | not null | `768` | — check `>0` | **No** — UI labels it "permanent — must match your embedding model" | insert only | `20260723000000_project_vector_config.sql:8,17`; `CreateProjectDialog.tsx:201` |
| `index_type` | text | not null | `'brute'` | — check ∈ {brute,hnsw,ivf,bq,auto} | No UPDATE path found | insert only | `20260723000000_project_vector_config.sql:9,13-14` |
| `max_records` | bigint | not null | `1000000` | — check `>0` | No UPDATE path found | insert only (default) | `20260723000000_project_vector_config.sql:10,20` |
| `pinned_image` | text | nullable | — | — | System-only, via backend update/rollback endpoints | `backend/apps/api/src/instance_lifecycle.rs:313` | `20260723070000_project_pinned_image.sql:11` |
| `worker_auth_token` | text | nullable | — | — | System-only; **not even readable** by `authenticated` | `main.rs:851` (sqlx, direct) | `20260811000000_worker_auth_token.sql:34` |

**Unique constraint**: original `unique(org_id, slug)` (`20260721120000_schema.sql:63`) was **dropped** by
`20260722090000_project_slug_reuse_after_delete.sql:9` to permit slug
reuse after soft-delete. **Current schema has no unique constraint on
`(org_id, slug)`.**

**Index**: `projects_org_id_idx on public.projects(org_id)`
(`20260721120000_schema.sql:66`).

**RLS** (`supabase/migrations/20260721120200_policies.sql:52-69`):
select/insert/update/delete policies gated on `is_org_member(org_id)` and
`org_role(org_id) in (...)`. Creation actually happens through the
SECURITY DEFINER RPC, so `projects_insert` is a secondary defense layer,
not the live insert path.

**Column-level grants narrow the RLS-level permissions further**
(`20260811000000_worker_auth_token.sql:55-60,73-74`): `authenticated`'s
SELECT excludes `worker_auth_token` entirely; `authenticated`'s UPDATE is
narrowed to exactly `name, status, last_active_at` — every other
"technically has an UPDATE RLS policy" column is unwritable by any
authenticated client regardless of RLS, confirmed by the grant statement
itself, not by RLS alone.

**RPCs writing to `projects`**: only `create_project_with_default_key`
(insert). `verify_api_key` only *reads* `projects.node_url`/`status`.

---

## 4. Project lifecycle (actual state machine, not assumed)

**States that actually exist** (`public.project_status` enum, built up
across three migrations): `creating` (initial), `active`, `stopped`,
`error`, `deleted`, `archived` (added later), `suspended` (added later).
**No separate `READY` state exists** — `active` is the only
"operational" state.

```
Project lifecycle (actual, code-traced):

  create_project_with_default_key (RPC)
        ↓
     creating  ────────────────────────┐
        │  provision_project_inner        │ (HTTP fetch fails/non-OK)
        │  succeeds → mark_project_active │
        ↓                                 ↓
      active                            error
      │  │  │
      │  │  └──[admin/cron sweep]──→ suspended ──[Start]──→ active
      │  └──[Stop route]──→ stopped ──[Start route]──→ active
      │                        │
      │                        └──[Archive, only if status was 'active']
      └──[Archive route: stop-if-active, then]──→ archived
                                                       │
                                              [Restore route]
                                                       ↓
                                                   stopped
                                          (comment: "doesn't restart
                                           compute automatically")

  any state ──[Delete route → Rust destroys containers/volumes/
               infra.instances rows]──→ deleted
              (projects ROW ITSELF IS NOT HARD-DELETED — status only)
```

**Who changes states, exactly**:
- `creating`→`error`: **UI-side**, on provisioning HTTP failure
  (`dashboard/actions.ts:84,98`).
- `creating`→`active` (+`node_url`): **Rust backend**, via
  `mark_project_active` after successful deploy (`main.rs:955` →
  `supabase.rs:106-134`).
- `active`↔`stopped`: user-initiated Stop/Start routes proxied to Rust
  `/v1/projects/:id/{stop,start}` (`main.rs:1176,1213`).
- `active`→`archived`: **UI route directly**, bypassing the Rust
  `set_project_status` call entirely (`ui/src/app/api/projects/[id]/archive/route.ts:10-46`).
- `archived`→`stopped`: **UI restore route** — explicitly does not
  restart compute (comment, `restore/route.ts:6-8`).
- any→`deleted`: **Rust backend**, `delete_project`
  (`main.rs:1065-1102`).
- `active`→`suspended`: **admin/cron sweep** (free-tier inactivity) or a
  manual admin endpoint (`main.rs:1303-1334`).

**What happens to the worker on deletion** (`main.rs:1065-1102`, exact
sequence): destroy each instance's container/deployment
(`provisioner.destroy`) → decrement host capacity → delete
control-plane-managed volumes → delete `infra.instances` rows → set
`projects.status='deleted'` via service-role PATCH. **The `projects` row
itself survives** (soft delete only).

**What does NOT happen on deletion — confirmed gaps, not inferred**:
- **API keys are not revoked.** No code path in `delete_project` touches
  `api_keys.revoked_at`. Revocation is only ever user-initiated
  (`ui/src/app/dashboard/settings/api-keys/actions.ts:92-120`).
- `project_usage_snapshots` (append-only table,
  `20260813000000_project_usage_snapshots.sql`) has no cleanup/delete
  logic tied to project deletion, found anywhere.
- Whether the node's own remote object-store snapshots
  (`VALORI_OBJECT_STORE_*`, a Kernel-side concept) are purged on project
  deletion: **Not established from current code** — no citation found in
  either repo linking Cloud-side deletion to Kernel-side object-store
  cleanup.
- Billing: **Not established from current code** — no citation found in
  this pass connecting `delete_project`/`archive` to any Stripe/billing
  side effect (consistent with CLAUDE.md's own instruction that Cloud
  billing logic stays out of the kernel repo, but this audit found no
  evidence either way inside `valori-ui` for this specific pass; a
  dedicated billing-code read was out of scope here).

---

## 5. Worker provisioning

**"Worker" = a Docker container** — either directly via the Docker Engine
API, or indirectly via a Dokploy-managed deployment (which itself is a
Docker container). No third abstraction (Kubernetes pod, VM) exists.

- `backend/apps/api/src/provision/docker.rs:1-6`: `Provisioner ->
  DockerProvisioner -> Docker Engine API -> Container`.
- `backend/apps/api/src/provision/dokploy.rs:1-13`: talks to a Dokploy
  REST API (`application.create`, `.saveDockerProvider`,
  `.saveEnvironment`, `.deploy`, `.stop`, `.start`, `.delete`).
- A third implementation, `MockProvisioner`, is **the default** in current
  config (`config.rs:222`: `_ => Some(ProvisionerKind::Mock), // default
  until a real host exists`) — meaning, **in the default configuration, no
  real infrastructure is provisioned at all** unless `PROVISIONER=docker`
  or `PROVISIONER=dokploy` is explicitly set.

**Runtime identity of a project's worker**: three separate pieces of
state, in two separate databases:
1. `projects.node_url` (Supabase) — public/routable URL. Only writer:
   `mark_project_active`.
2. `infra.instances` rows (Rust's own Postgres schema, `db/instance.rs:7-25`)
   — `host_id`, `container_id`, `http_port`, `raft_port`, `node_index`
   per instance.
3. `projects.worker_auth_token` (Supabase, Rust-readable only) — the
   internal bearer token the control plane presents to the node, injected
   as `VALORI_AUTH_TOKEN` at deploy time.

`node_url` form depends on routing mode: `https://{project_id}.{nodes_domain}`
under Dokploy/DNS routing, or raw `http://{host.ip}:{instance.http_port}`
otherwise (`main.rs:941-952`).

---

## 6. What is a Collection today?

### 6.1 — Creation, full trace

**Route registration**, identical URLs on both paths:
`crates/valori-node/src/server.rs:404-406` and
`crates/valori-node/src/cluster_server.rs:367-369` — `POST/GET
/v1/namespaces`, `DELETE /v1/namespaces/:name`.

**Shared handler logic** in `crates/valori-node/src/routes/collections.rs`
via a `CollectionOps` trait — validates name (non-empty, ≤64 chars,
`[a-zA-Z0-9_-]`, lines 60-75), special-cases `"default"` as an idempotent
no-op, otherwise calls `ops.create(&name)`.

**Standalone** (`server.rs:2803-2828`) → `Engine::create_collection`
(`crates/valori-engine/src/engine.rs:956-982`):
```rust
pub fn create_collection(&mut self, name: &str) -> Result<u16, EngineError> {
    let id = self.namespaces.create(name).ok_or_else(...)?;
    self.commit_and_apply_ns(
        &valori_kernel::event::KernelEvent::AutoCreateNamespace { name: String::new() },
        id,
    )?;
    self.flush_namespaces()?;
    Ok(id)
}
```
**There is a `KernelEvent`** (`AutoCreateNamespace`,
`crates/valori-kernel/src/event.rs:145`) — it IS in the canonical
event/log stream via `commit_and_apply_ns`, the same durability path
every mutation uses.

At the kernel level, `AutoCreateNamespace` is a near-no-op
(`crates/valori-kernel/src/state/kernel.rs:566-571`):
```rust
KernelEvent::AutoCreateNamespace { name: _ } => {
    // The name is not stored in KernelState — namespaces are pure integer ids here.
    let ns = namespace_id as usize;
    if ns >= MAX_NAMESPACES { return Err(KernelError::InvalidOperation); }
}
```
**The name itself is allocated in `Engine.namespaces` (the
`CollectionRegistry`) *before* the event is committed** — the canonical
event only reserves the integer id. Name persistence is a **separate,
non-canonical** mechanism: `Engine::flush_namespaces()`
(`engine.rs:421-438`) serializes the whole registry to a JSON sidecar
file, **not** the canonical snapshot/event-log/WAL.

**Cluster** (`cluster_server.rs:544-586`) commits the same
`KernelEvent::AutoCreateNamespace{name}` through Raft — deliberate,
per comment: *"Phase S2: collection creation/drop goes through Raft
... instead of mutating a per-node, unreplicated registry directly."*

### 6.2 — What IS a collection, definitively

**Answer, from direct structural evidence — a collection is purely a
`NamespaceId(u16)`** (`crates/valori-core/src/id.rs:34-51`:
`pub type CollectionId = NamespaceId;` — "the user-facing name for the
same concept"), with the name→id mapping living entirely **outside**
`valori-kernel`, in `Engine.namespaces: CollectionRegistry` (node-process
memory + JSON sidecar).

**Dimension/metric/index-kind are confirmed GLOBAL-TO-THE-NODE, not
per-collection** — re-verified directly, not trusted from any prior
summary:
- `Engine` (`engine.rs:105-152`) has exactly **one** `pub dim: usize`,
  **one** `pub index: Box<dyn VectorIndex...>`, **one**
  `pub index_kind: IndexKind` — all single fields on the single `Engine`,
  with `namespaces: CollectionRegistry` sitting alongside as just the
  name-map.
- `KernelState.dim: Option<usize>` (`kernel.rs:21`) — a single scalar on
  the whole kernel state.
- The Python SDK's own docstring confirms this at the client layer too
  (`python/valoricore/remote.py:2482-2487`): *"Valori's collections do NOT
  have their own dimension/index — those are fixed at the PROJECT level
  ... and shared by every collection inside it."*

### 6.3 — Collection schema across every layer

| Property | Stored where | Persisted? | Mutable today? | Used by |
|---|---|---|---|---|
| `name` | `CollectionRegistry.map` key (live, in `Engine.namespaces`) | JSON sidecar via `flush_namespaces` — **not** canonical snapshot/event-log | No rename API found anywhere | `CollectionOps::resolve/list`, SDK |
| `namespace_id` | `CollectionRegistry.map` value; per-row on `Record`/`GraphNode` | **Yes** — per-row `namespace_id` IS in the canonical snapshot and IS hashed by `hash_state_blake3` | No (fixed at allocation) | Kernel apply path, index, everything |
| `dimension` | `Engine.dim`/`KernelState.dim` — **global, not per-collection** | Yes, canonical snapshot | Locked on first insert; not collection-scoped at all | insert/search validation |
| `metric` | Hardcoded L2 — no field exists | N/A | N/A | N/A |
| `index_type`/`config` | `Engine.index_kind` etc. — **global**, set at node startup | Yes, node-level only | No per-collection API | — |
| `metadata` (on the collection object) | **Does not exist** | N/A | N/A | N/A |
| `description` | **Does not exist anywhere** | N/A | N/A | N/A |
| `status` | **Does not exist anywhere** | N/A | N/A | N/A |
| `timestamps`/`created_at` | Only on the **dormant** `valori-metadata::Collection.created_at` | Never written in practice (DB unopened, §6.4) | N/A | Nothing |
| `record_count` | Does not exist per-collection (only `Project.record_count` on the dormant path, project-wide) | Never written in practice | N/A | N/A |
| `project` | Only on the **dormant** `valori-metadata::Collection.project` | Never active | N/A | N/A |

Live API-level schema is exactly `crates/valori-node/src/api.rs:563-583`:
`CreateCollectionRequest{name}`, `CollectionInfo{name,id}`,
`CreateCollectionResponse{name,id,created}`,
`ListCollectionsResponse{collections}`. **`name` and `id` only —
nothing else.**

### 6.4 — Duplicate representations (flagged explicitly, per rule 9)

Two live-but-different representations plus one confirmed-dormant one:

1. **`valori-kernel`**: no named "Collection" type — purely `NamespaceId`
   plus per-row `namespace_id` fields.
2. **`valori-engine::Engine.namespaces: CollectionRegistry`** — the *one
   representation actually exercised in production* (§6.1/§6.2).
3. **`valori-metadata::Collection` + its own `CollectionRegistry`**
   (`crates/valori-metadata/src/collection.rs:12-23,38-43`) — a second,
   richer struct (`name, project, namespace_id, created_at`) with its own
   near-duplicate `create/drop/resolve/list` methods, backed by a redb
   `COLLECTIONS` table. **Confirmed dormant**: `valori-metadata/src/lib.rs:10-32`
   states in-code: *"`MetadataDb` is not opened by any production binary
   (S7) ... no call to `MetadataDb::open` exists in `valori-node`,
   `valori-daemon`, or `desktop/src-tauri` today ... `PROJECTS`/
   `COLLECTIONS` are not this crate's current, active responsibility,"*
   enforced by test `crates/valori-node/tests/dependency_direction.rs::
   metadata_db_open_stays_out_of_production_binaries`. The file's own
   comment documents the intended migration that never completed
   (`collection.rs:32-36`): *"This is the elevated form of
   `NamespaceRegistry` currently in `valori-node/src/engine.rs`. Future
   phases will replace the engine's inline registry with this type."*
   That replacement never happened — both exist simultaneously today, one
   live, one dormant.
4. **Python SDK**: two separate client-side notions — a thin
   pass-through mixin (`create_collection`/`list_collections`/
   `drop_collection`, `remote.py:733-748`, no local state) and a
   Cloud-facing `Collection` class (`remote.py:2427-2507`) whose
   `__init__` stores *only* `self.name`, documented explicitly as having
   "no local state beyond the collection name."

**No representation, live or dormant, carries dimension/metric/index
config per-collection anywhere** — confirmed absent in all of them.

---

## 7. Dimension (traced precisely)

- **Global-to-the-node**, confirmed structurally (§6.2): single
  `KernelState.dim: Option<usize>`.
- **Lock point**: first insert into **any** namespace.
  `apply_event_ns`'s `InsertRecord` arm (`kernel.rs:324-334`):
  ```rust
  if let Some(dim) = self.dim {
      if d != dim { return Err(KernelError::DimensionMismatch{expected: dim, found: d}); }
  } else {
      self.dim = Some(d);
  }
  ```
  **Not namespace-scoped** — this checks a single field regardless of
  which namespace the insert targets, so the *first* insert into *any*
  collection locks dimension for *every* collection in that node.
- **Not set at collection-creation time** — `CreateCollectionRequest` has
  only a `name` field, no `dim` parameter (`api.rs:563-565`).
- **Validated on insert and search** — exact rejection:
  `engine.rs:849-853,911-915`,
  `Err(EngineError::Kernel(KernelError::DimensionMismatch{...}))`.
- **Part of the canonical snapshot**: yes (`snapshot/encode.rs:69,97,282`;
  `snapshot/decode.rs:176-200`, with an explicit `if dim > MAX_DIM`
  bound check).
- **Part of `hash_state_blake3`**: **not directly** — the documented hash
  input list (`snapshot/blake3.rs:76-107`) enumerates per-record vector
  bytes (whose *length* is implicitly dim) but has no standalone "dim
  (u32)" line. So dim is only **indirectly** committed via vector
  lengths, not hashed as its own field — flagged as an unconfirmed/
  indirect claim, not asserted as a direct one.
- **Encoded in any `KernelEvent`**: no — never a field of any event
  variant; always inferred from `vector.len()` on the first insert.

**Cloud-side, separately** (`valori-ui`): `projects.dim` is a real
Postgres column, defaulted `768`, checked `>0`
(`20260723000000_project_vector_config.sql:8,17`), UI-labeled "permanent"
(`CreateProjectDialog.tsx:201`), and passed through provisioning as an env
var to the deployed node. **This is the Cloud-side declaration of what
the node's `VALORI_DIM` startup config will be — it is not the same
mechanism as the kernel's own first-insert lock**, though in practice a
correctly-provisioned node's `VALORI_DIM` and its kernel's
first-locked-dim should agree by construction (not independently
verified in this pass whether any code path could cause them to diverge).

---

## 8. Metric

Workspace-wide search for cosine/dot-product/inner-product/Manhattan **as
a vector-search metric** (distinguishing from unrelated uses):

- `dot_product`/cosine appear in exactly two **non-search** features:
  (1) C4.3 contradiction detection (`engine.rs:1434-1445`,
  `cluster_server.rs:2677-2685`, `api.rs:618-624`, default threshold
  0.85) — computes cosine similarity between two *specific* records on
  request, not a search-index metric; (2) community-centroid ranking
  (`server.rs:3919`, `valori-rag/src/community.rs:309`) — cosine over
  detected-community centroids, a RAG feature, not the primary ANN index.
- No `inner_product` or `manhattan` matches anywhere in the workspace.
- **Vector-search entry points are named `search_l2`/`search_l2_ns`/
  `search_l2_filtered`** (`engine.rs:834,838,899`) — `SearchRequest` has
  no `metric` field.

**Verdict**: L2/squared-L2 is confirmed as the **only** vector-search
metric implemented, on both standalone and cluster, across all four index
types (BruteForce/HNSW/IVF/BQ, cross-checked against the G1.4.3 audit's
index findings this session). Metric choice is **not exposed via any
API or SDK parameter** — hardcoded in the search method names and each
`Engine`'s single boxed index.

---

## 9-10. Indexes — see the dedicated G1.4.3 audit

Full detail (implemented/standalone/cluster/persisted/rebuilt/config/
runtime-selectable matrix, exact HNSW `M`/`ef_construction`/`ef_search`,
IVF `n_list`/`n_probe`, BQ `pool_factor`/`min_candidates` configuration
surfaces) was already produced this session in
[docs/reviews/graph-g1.4.3-cluster-index-capability-audit.md](graph-g1.4.3-cluster-index-capability-audit.md)
and is not re-derived here to avoid duplicating a just-completed,
independently-citable audit. Summary relevant to *this* document's scope:
index kind is **global-to-the-node** (§6.2, re-confirmed independently
here), never per-collection, on both standalone and cluster paths — this
is the same conclusion the index-capability audit reached from the
opposite direction (auditing indexes) that this document reaches from the
collection-schema direction. Cluster mode implements only BruteForce/BQ
(kernel-native); standalone implements all four via `valori-index`.

---

## 11. Index lifecycle (collection-creation-time framing)

Re-confirmed in this pass, specifically answering "is index selected at
collection creation": **no** — index kind is chosen once, at **node
process startup**, via `VALORI_INDEX` (standalone) or never reaches
cluster mode at all (per the G1.4.3 audit) — never at collection-creation
time. `POST /v1/namespaces` has no index-kind parameter
(`CreateCollectionRequest`, §6.3). `POST /v1/index/rebuild` can switch
the *entire node's* active index kind (standalone only — cluster's
equivalent endpoint is a stub, per G1.4.3), but this affects every
collection in that node uniformly; there is no per-collection index
selection or rebuild anywhere in the code.

---

## 12. Vector storage (traced end to end)

```
POST /records or /v1/vectors/batch-insert
  ↓
crates/valori-node/src/server.rs (or cluster_server.rs)
  ↓
Engine::insert_record* (crates/valori-engine/src/engine.rs)
  ↓
KernelEvent::InsertRecord / AutoInsertRecord (crates/valori-kernel/src/event.rs)
  ↓  commit_and_apply_ns — audit log write, THEN kernel apply (durability order)
KernelState::apply_event_ns → RecordPool::insert (crates/valori-kernel/src/storage/pool.rs)
  ↓  post_apply_derived (Engine-level, std side)
index.on_insert(id, vec)  — the ACTIVE Box<dyn VectorIndex>
  ↓  (periodic / on shutdown)
Snapshot (canonical k_data section + optional derived i_data section, §-refs
the G1.4.3 audit's exact-cited encode.rs/decode.rs locations)
```

Vectors physically live in `RecordPool` (`crates/valori-kernel/src/storage/pool.rs`),
keyed by `RecordId(u32)`, with `namespace_id: u16` on each `Record`
(`crates/valori-kernel/src/storage/record.rs:33`) tying it to its
Collection. Canonical representation is Q16.16 fixed-point
(`FxpVector`); the derived index (standalone only, for non-BruteForce
kinds) additionally stores its own `f32` copy internally, per the
G1.4.3 audit's determinism findings.

---

## 13. Record / Collection / GraphNode / GraphEdge relationship

```
[NO Project awareness at kernel/node level — confirmed absent]

  Collection (name)
       │  resolve (Engine.namespaces: CollectionRegistry, JSON sidecar only)
       ▼
  NamespaceId(u16)
       │
       ├──────────────────────────────┐
       ▼                               ▼
  Record.namespace_id            GraphNode.namespace_id
  (per-vector, canonical          (per-node, canonical
   snapshot, hashed)               snapshot, hashed)
       ▲                               │
       │                               ▼
       └── GraphNode.record: Option<RecordId> ──┘
           (0 or 1 record per node; N nodes MAY
            reference the same record — confirmed
            1:N, not 1:1, by this session's G1.3.1 work)
                       │
                       ▼
                  GraphEdge (from/to: NodeId;
                  namespace derived from endpoint
                  nodes, not stored redundantly
                  on the edge struct itself)
```

**Exact invariant enforced, and exactly where**: `apply_event_ns`'s
`CreateNode` arm (`crates/valori-kernel/src/state/kernel.rs:389-393`):
```rust
if let Some(rid) = record {
    let rec = self.records.get(*rid).ok_or(KernelError::NotFound)?;
    if rec.namespace_id != namespace_id {
        return Err(KernelError::InvalidOperation);
    }
}
```
Enforced **at the kernel/event level**, inside `KernelState::apply_event_ns`
— the single authoritative apply path — not merely at the HTTP/API
layer. This holds uniformly for standalone, Raft-replicated cluster
application, and event-log replay, because all three route through the
same function.

**Record↔GraphNode cardinality**: `GraphNode.record: Option<RecordId>`
(`crates/valori-kernel/src/graph/node.rs:11`) — nothing constrains
uniqueness the other way. This session's own G1.3.1 phase
([phase-G1.3.1-record-graph-cascade-fix.md](../phases/phase-G1.3.1-record-graph-cascade-fix.md))
shipped `nodes_referencing_record(state, record_id) -> Vec<u32>`
specifically because the true relationship is 1:N (one record, N
referencing nodes), replacing a prior `record_to_node: HashMap<u32,u32>`
1:1 cache that was wrong for this exact reason — direct, previously-proven
evidence for the cardinality claim, not a fresh inference.

**Where "Project" would connect to this diagram**: nowhere, today. A
Project (§2-5) is Cloud-side identity/billing/provisioning metadata for
zero-or-more deployed node processes; a Collection (§6-13) is a purely
kernel/node-side integer namespace inside one running node process. The
two connect only in the sense that a Project *causes* a node to exist,
and that node then independently accumulates whatever Collections its
users create via HTTP calls — there is no schema, foreign key, or shared
identifier linking a specific `projects.id` row to the set of
`NamespaceId`s that exist inside its deployed node at any given moment.
The Cloud side has no visibility into which Collections exist inside a
project's node beyond whatever it chooses to query live over HTTP.

---

## Summary of "declared but unused / dormant / discrepancy" findings (consolidated)

1. **`valori-metadata::Collection`/`CollectionRegistry`/`MetadataDb`'s
   `COLLECTIONS` table** — fully coded, tested, dormant. Never opened by
   any production binary. (§6.4)
2. **Four+ distinct `Project`-shaped structs inside this repo alone**
   (`valori-metadata`, `valori-domain`, `valori-daemon`, CLI's
   `SavedProject`), none of which is the Cloud SaaS `projects` table,
   which is a fifth representation in a different repository. (§1.4, §2)
3. **`worker_auth_token`, `node_url`, `pinned_image`** on the Cloud
   `projects` table are correctly system-only by design (confirmed via
   their own migration comments), not an oversight — but worth noting
   for the redesign as columns no client code path can read or write.
4. **`region`** column/query infrastructure is real and load-bearing, but
   only one host/region is provably seeded anywhere in the `valori-ui`
   repo — multi-region is schema-ready, not demonstrated as exercised.
5. **`provider`** is not a `projects` column at all — it lives on
   `infra.hosts`, and the whole deployment runs under a single
   `PROVISIONER` env var at a time, not selectable per-project.
6. **`plan`** is not a `projects` column at all — it lives on
   `subscriptions`, per-organization, joined via `org_id` wherever plan
   limits are enforced.
7. **Project deletion does not revoke API keys** — confirmed gap, no
   code path found linking the two.
8. **Collection dimension/metric/index are confirmed global-to-the-node**
   on both the Kernel side (§6.2/§7/§8) and independently corroborated by
   the Cloud side's own schema design (`projects.dim`/`index_type` are
   project-level columns, not present anywhere at a per-collection
   granularity) — this is the single most load-bearing fact for any
   future Project/Collection redesign discussion, confirmed from two
   independent codebases, not asserted once and assumed.
