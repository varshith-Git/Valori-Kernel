# Shared hosting: production hardening roadmap

## Correction to prior research

An earlier pass on this topic proposed building a new in-process
"EngineRegistry" multiplexer and token-only routing, on the mistaken premise
that no shared-tenancy runtime existed. That premise was wrong. Confirmed by
reading `crates/valori-node/src/shared.rs` and `docs/shared-hosting.md` in
full:

- Shared hosting already exists (`VALORI_SHARED_ROOT`, `SharedHost`). One
  process hosts multiple projects; **each project owns an independent
  `Engine`, collection registry, graph, metadata, and event log**
  (`shared.rs:150-195`). State roots are per-project, not shared
  (`shared.rs:395-398`, `docs/shared-hosting.md:3-6`).
- Routing is project-ID-in-path plus per-project-token, not token-only:
  `http://worker:3000/shared/projects/{id}/node/...`, and a project's token
  only authorizes that exact project (`shared.rs:293-330`,
  `docs/shared-hosting.md:45-50`). This is deliberate — explicit tenant
  identity in the URL is easier to audit, revoke, and debug than inferring
  identity from a token alone. **Do not change this.**
- Registration, activation/suspension, deletion, and export already exist
  (`shared.rs:197-291, 363-400`), as does a state-hash-verified paid
  upgrade path (shared → dedicated) via `POST
  /v1/admin/orgs/{org-id}/dedicate` and `POST /internal/shared-import`
  (`docs/shared-hosting.md:71-91`, `shared.rs:404-503`).
- Today this is wired for **Free only**; paid plans use the existing
  dedicated-container provisioner (`docs/shared-hosting.md:8-10`).

This spec is about hardening and extending what's there — not replacing it.

## Second correction (this revision)

A review of the first revision of this spec found further inaccuracies,
verified against the code before being accepted here:

- **`VALORI_INDEX` is obsolete.** Confirmed: `grep` across
  `crates/valori-node/src` shows it is only read once, cosmetically, in
  `cluster_server.rs:3603` for a status field — never to select an index
  implementation. Index configuration is collection-scoped
  (`CreateCollectionRequest`, `server.rs:3937-3949`). Any benchmark
  "reusing the `VALORI_INDEX` matrix" would be measuring nothing real; the
  harness must create collections explicitly per index kind instead.
- Several Phase 4/6/7 claims below stated capabilities or guarantees the
  rest of the roadmap doesn't actually build (uniform startup complexity,
  weighted Pro scheduling, an SLA, symmetric migration directions,
  fencing-free consistency). These are corrected in place below rather than
  listed separately — see each phase's revised text.

## Real gaps (confirmed against `shared.rs` and the doc)

- `SharedHost::open()` loads **every** registered project's `Engine` eagerly
  at startup (`shared.rs:85-106`, `open_project()` call at line 104) — no
  lazy loading.
- `set_active(active: false)` only flips a manifest flag
  (`shared.rs:249-265`); it does not release the project's `Engine` from
  memory. Stopping a project saves no RAM.
- No idle eviction exists at all.
- `VALORI_SHARED_MAX_PROJECTS` (`shared.rs:223-228`) is a project-*count*
  admission limit — not memory-, CPU-, or throughput-aware.
- No per-project reads/min, writes/min, or concurrent-request limits.
- No per-project CPU/memory attribution or resource accounting.
- No global shared-worker metrics (deliberately, per
  `docs/shared-hosting.md:60-61` — a design choice to audit, not assume is a bug).
- `SharedHost::open()` fails the entire worker's startup if any one
  project's log is corrupt (`shared.rs:100-104`, propagates `Err` up through
  `main.rs`) — one bad tenant can block every tenant on that worker from
  coming back up after a restart. **This is a mandatory production
  blocker, not merely a Phase 1 finding to weigh** — see Phase 4A.
- `dispatch()` holds a single global `projects.read()` guard for the full
  duration of every request (`shared.rs:293-298`), and lifecycle operations
  (`create_project`, `set_active`, `delete_project`) take the paired
  `.write()` guard. A write-lock waiter (e.g. a delete) queues behind every
  currently in-flight read/write request on *any* project on that worker,
  and a new write-lock acquisition then blocks all new requests to all
  projects until it's granted.
- No capacity-aware placement across a fleet of shared workers — today's
  control-plane placement (`backend/apps/api/src/provision/placement.rs`)
  targets dedicated per-project hosts, not shared-worker admission.
- Pro-tier shared hosting doesn't exist yet — only Free.

## Phase ordering

```text
Phase 1:  Measurement and failure audit
Phase 2:  Pool topology decision (design proposal, pending Phase 1)
Phase 3:  Limits and concurrency protection
Phase 4A: Failure quarantine
Phase 4B: Lazy loading and eviction
Phase 5:  Metrics and capacity-aware placement
Phase 6:  Pro pool rollout
Phase 7:  Migration and rebalancing
```

Quarantine (4A) is split from lazy loading/eviction (4B): they touch the
same registry but address different risks, and quarantine is a production
blocker regardless of whether eviction has landed yet.

## Phase 1 — Measurement and failure audit (implementation-ready now)

**No production-runtime behavior changes.** This phase adds benchmark,
workload-generation, and reporting code only — it does not touch
`shared.rs` or any request-serving path. ("No code changes" was imprecise
in the prior revision; a benchmark harness is code, and that distinction
matters when the eventual PR is reviewed.)

### Baseline: dedicated vs. shared

Every relevant workload below is measured in **both** configurations, not
shared mode alone — the point is to quantify what pooling actually saves,
not just shared mode's absolute cost:

- **Dedicated baseline:** N separate `valori-node` processes, one project
  each (today's real Free/paid topology).
- **Shared baseline:** one `SharedHost` process containing the same N
  projects.

Compared across both: total RSS, startup/recovery time, idle CPU, file
descriptor count, search throughput, insert throughput, p50/p95/p99
latency, disk-write volume, graceful-shutdown time.

### Memory measurement method

"Memory per project" is ambiguous under a shared allocator unless the
method is pinned down:

- Metric: process RSS, and PSS where the platform provides it.
- Sampled at each of these points, in order, for the same project:
  before registration → after manifest registration → after engine
  recovery → after collection creation → after insertion → after index
  build → after deletion → after engine eviction (once Phase 4B exists to
  produce that state).
- Minimum 5 repeated runs per data point; report median and variance, not
  a single sample.
- A fixed warm-up/stabilization period is excluded from sampling.
- Record allocator, OS, Rust toolchain version, and machine spec alongside
  every result set — these numbers are meaningless without that context.

Data shapes to cover, per the shape axis in the benchmark matrix below:
empty project; vectors only; vectors + metadata; vectors + graph edges;
soft-deleted records; multiple collections per project; mixed dimensions
and index kinds within one project.

### Index benchmarking (corrected)

`VALORI_INDEX` is not read by `valori-node` for index selection (only
cosmetically in one cluster status field). The harness must create
collections explicitly via `CreateCollectionRequest` for each index kind —
brute, HNSW, IVF, BQ, and (if auto-selection behavior itself is being
measured) auto — and record every index parameter used (e.g. HNSW `m`,
`ef_construction`; IVF `n_list`, `n_probe`) alongside each result. Without
recorded parameters, results are not reproducible.

### Fixed benchmark matrix

No selective testing — every cell below is either measured or explicitly
marked `NOT MEASURED` with a reason, never silently skipped.

| Axis | Test points |
|---|---|
| Projects | 1, 10, 50, 100 |
| Records/project | 0, small, medium, plan maximum where feasible |
| Dimensions | Representative supported dimensions |
| Index | Brute, HNSW, IVF, BQ |
| Workload | Read-only, write-only, mixed |
| Concurrency | Per-project and cross-project |
| Data shape | Vector-only, +metadata, +graph, mixed |
| Recovery | Clean, large WAL, snapshot+WAL, corrupt tenant |

Methodology, fixed for every run: deterministic random seed; deterministic
vectors; fixed request sequences; warm-up excluded from reported numbers;
minimum repeat count as above; raw results saved as machine-readable
JSON/CSV with a summary generated from the raw data (not hand-written);
hardware/software manifest recorded once per report.

### Concurrency-safety findings (registry lock)

Measure, under `dispatch()`'s current single global `RwLock`:
- A delete request queued behind a long-running search on a different
  project.
- A suspend (`set_active`) queued behind a bulk insert on a different
  project.
- An export queued behind active traffic.
- A new project registration queued behind load on existing projects.
- Whether one stuck/slow request measurably delays lifecycle operations
  (create/suspend/delete/export) across *all* projects on the worker, not
  just its own.

This does not fix anything in Phase 1 — it quantifies the problem Phase 4B
must resolve (see Phase 4B's revised text below).

### Corrupt-log isolation testing

The code already proves one invalid project can fail `SharedHost::open()`
and block every healthy project on that worker from starting — this isn't
in question. Phase 1 quantifies its blast radius and confirms the taxonomy
of failures a quarantine mechanism (Phase 4A) must handle. Minimum cases:
truncated final event; corrupted final event; corrupted middle event;
invalid chain hash; invalid manifest JSON; missing event log; unreadable
project directory; snapshot corruption with an otherwise-valid event log;
one corrupt project planted among 10, 50, and 100 healthy ones.

**Deliverable:** a written report with all of the above, plus recommended
per-project memory/CPU budgets, committed to `docs/`. Phase 2 does not
proceed until this report exists.

**Acceptance gate:** benchmark harness runs reproducibly against a local
`SharedHost` instance via a documented command; every matrix cell has a
result or an explicit `NOT MEASURED` with reason; the report states the
measured blast radius of a corrupt project (today: the whole worker) and
the measured dedicated-vs-shared savings.

## Phase 2 — Free/Pro pool classes (design proposal, pending Phase 1 data)

**Goal:** decide whether Free and Pro should run as separate worker pools
(`shared-free`, `shared-pro`) with independent oversubscription and
priority policy, versus sharing one pool class with per-plan limits only.

**Open question Phase 1 must answer first:** what per-project memory/CPU
footprint and what blast-radius-of-failure are acceptable to co-locate Free
and Pro on the same process, if at all. Do not fix the pool topology (one
shared pool vs. two) before that's known.

**Dependencies:** Phase 1 report.

**Not decided yet:** worker-class naming beyond the placeholders above,
oversubscription ratios, how many projects per worker at each tier.

## Phase 3 — Limits and concurrency protection

**Goal:** extend the existing project manifest (`shared.rs:53-58`,
currently `{token_hash, max_records, active}`) with two distinct kinds of
limit, not one undifferentiated bag — a Rust process cannot strictly
enforce per-project CPU or RAM the way cgroups can, so hard quotas and
resource-estimate signals need separate models:

```text
ProjectLimits (hard, worker-enforced quotas)
├── max_records
├── max_storage_bytes
├── reads_per_minute
├── writes_per_minute
└── max_concurrent_requests

ProjectPlacementBudget (admission/scheduling signals, estimates)
├── estimated_resident_bytes
├── workload_weight
├── residency_priority
└── cold_start_budget
```

`max_resident_memory_bytes`-style fields belong in the placement budget as
an *estimate* used for admission decisions, not a hard boundary — the
worker process cannot evict memory it doesn't control the way an OS/cgroup
can.

**VRAM is out of scope.** It does not belong in this roadmap: the current
shared runtime disables on-node local model inventory and on-node
ingestion (`docs/shared-hosting.md:59-65`), so Free/Pro vector serving has
no meaningful per-project VRAM allocation to budget. Reintroduce it only if
a future phase enables on-node GPU models in shared mode.

**Rate-limiting semantics** (reads/min, writes/min) must be defined, not
left implicit:
- Explicit route classification: which routes count as a read, which as a
  write (e.g. `/v1/search`, `/v1/graphrag` = read; `/v1/records`,
  `/v1/namespaces` mutations = write).
- Bulk insert counts per-record, not per-request, against the write quota.
- Rejected (429'd) requests do not themselves consume quota.
- Algorithm: project-level token buckets at the shared worker (not fixed
  window) — burst allowance and refill rate are Phase 1-informed constants,
  not guessed here.
- Wall-clock source and behavior across a process restart (bucket resets,
  does not persist) must be stated explicitly in the implementation, not
  left to accidental behavior.
- Scope: per project, at the shared worker. API-key-level abuse control
  remains a separate concern in the Cloud proxy — this phase does not
  duplicate that.
- Response: `429` with `Retry-After`.
- `health` and other management/internal routes (already excluded from
  tenant dispatch per `shared.rs:311-330`) are never rate-limited as tenant
  traffic.

**Storage-quota accounting** (`max_storage_bytes`) must define what counts:
event log, snapshot, metadata, namespace sidecars, graph data, shred log —
explicitly excluding transient temp files and export artifacts (those are
operational, not tenant data, and are cleaned up independently). Checked
both **before** a write (a conservative size estimate, to avoid ever
exceeding disk capacity) and **after** (to correct the running total) —
after-only checking can overshoot; before-only checking without a
follow-up correction can drift.

**Invariant:** limits are enforced **in `dispatch()` at the shared
worker**, not only in `valori-ui`'s `quota.ts`. Control-plane-only checks
can be bypassed or go stale relative to the worker's actual state.

**Dependencies:** Phase 1 data informs default limit *values*; Phase 2
decides whether limits differ by pool class or just by `plan_class` within
one pool.

**Acceptance gate:** a project that exceeds any `ProjectLimits` quota is
rejected at the worker with `429`/`Retry-After` (or the applicable status),
without affecting other projects' availability or latency on the same
worker; storage accounting never allows a confirmed write past the
pre-write conservative estimate.

## Phase 4A — Failure quarantine (production blocker, independent of 4B)

**Goal:** one project's corrupt or unreadable on-disk state must fail only
that project's own load — never `SharedHost::open()` as a whole. This is
not conditional on what Phase 1 finds; the code already proves the current
behavior is a whole-worker outage triggered by one bad tenant.
**Production Free/Pro pool rollout is blocked until this quarantine exists
and is verified**, using the corrupt-state taxonomy Phase 1 tested against.

**Behavior:** a quarantined project is reported as failed (visible to
control-plane health checks and to `SharedHost` operators), the worker
continues serving every healthy project normally, and the worker never
silently starts a corrupt project as empty state.

**Dependencies:** Phase 1's corrupt-log taxonomy and blast-radius findings.

**Acceptance gate:** in every corrupt-state case from Phase 1's taxonomy,
planted among a mix of healthy projects, only the corrupt project fails to
load; all healthy projects start and serve normally; the corrupt project
is never silently replaced with empty state.

## Phase 4B — Lazy loading and eviction

**Goal:** replace `SharedHost::open()`'s eager load-everything with
manifest-only discovery at startup; load an `Engine` on first request;
evict idle `Engine`s after `idle_timeout_seconds`, flushing and
snapshotting before release. Introduce lifecycle states per project entry
(unloaded / loading / resident / evicting / failed) so a project's state is
always well-defined mid-transition, including under concurrent requests.

**Corrected startup-complexity claim:** startup cannot become independent
of registered-project count — manifest discovery alone is at least O(n) in
registered projects. The defensible claim is: **startup performs manifest
discovery only and does not recover every project's engine; startup cost
scales with manifest parsing, not with total records, indexes, or WAL sizes
across registered projects.**

**Registry-lock redesign:** `dispatch()` must not hold the global registry
lock across request execution, per Phase 1's concurrency-safety findings.
Resolve or clone a project handle under a short-lived registry lock,
release that lock, then operate through the project-local lifecycle state
for the duration of the request. Lifecycle operations (create/suspend/
delete/export) must not queue behind arbitrarily long in-flight data
requests on unrelated projects.

**Invariant:** eviction must drain in-flight requests before releasing an
`Engine` — never drop a live request's state out from under it.

**Ownership boundary — planner scope stated explicitly:**
```text
Data-plane operations:      request → project router → engine
Complex lifecycle operations: operation → valori-planner → execution graph → executor
```
This phase's lifecycle operations (load/evict) execute entirely within one
worker process and use the planner (or an equivalent in-process state
machine) locally — they do not require Cloud-orchestrated execution.
Ordinary search/record requests never round-trip through Cloud
orchestration for a synchronous read or write. Cross-*worker* lifecycle
operations (relocation, migration) are a separate ownership question,
addressed in Phase 7.

**Dependencies:** Phase 3's limit/lifecycle plumbing; Phase 4A must land
first (quarantine and eviction touch the same registry; quarantine is the
higher-priority, independent fix).

**Acceptance gate:** startup time scales with manifest count, not with
recovered-engine work; a newly-idle project's memory is measurably
released; a lifecycle operation (delete/suspend/export) on one project does
not measurably delay in-flight requests on other projects.

## Phase 5 — Metrics and capacity-aware placement

**Goal:** replace project-count-only admission (`shared.rs:223-228`) with a
shared worker reporting real capacity signals — resident memory, available
memory, resident engine count, active requests, throughput, disk
usage/free-space, recovery-queue depth, p95/p99 latency, worker
version/health — and have the control-plane's placement logic
(`provision/placement.rs`) use weighted capacity with a hard memory safety
margin, instead of `projects.len() < max_projects`.

**Dependencies:** Phase 4B (accurate resident-memory accounting requires
lazy loading/eviction to be meaningful — a worker that loads everything
eagerly can't distinguish "busy" from "just has many idle tenants").

**Acceptance gate:** the control-plane can place a new project on the
least-loaded eligible worker in a multi-worker pool using real reported
metrics, and refuses placement (rather than silently overloading a worker)
when no worker has a safe memory margin.

## Phase 6 — Extend shared mode to Pro

**Goal:** Pro projects run on the same hardened shared-hosting runtime as
Free, on a separate Pro pool (if Phase 2 concludes pools should be
separate) with: higher rate/storage limits, longer residency / fewer cold
starts, lower oversubscription, and more available shared-mode features
than Free. Enterprise is unaffected — still dedicated containers/clusters.

**Corrected scheduling claim:** "higher scheduling weight" is removed as a
Pro promise unless a fair-admission scheduler with weighted concurrency
permits is actually designed and built — nothing in Phases 3-5 introduces
one, and token buckets plus placement cannot by themselves prioritize two
simultaneous resident tenants inside one Tokio process. Either (a) rely on
separate Free/Pro pools to make weighting unnecessary (the default
assumption here), or (b) a future phase explicitly adds a weighted
admission scheduler before this claim can be made. Do not advertise
weighted scheduling without one of those two in place.

**Corrected SLA claim:** a separate Pro pool does not by itself create an
availability SLA. Shared mode cannot run with Raft, has a process-level
failure domain, and stores project state on one worker's disk — it
requires recovery or relocation after host failure, none of which this
phase builds. Replacing "faster support/SLA tier": **product-level support
response targets may be defined separately, but this phase must not claim
an infrastructure SLA (uptime commitment) unless failover, backup recovery
objectives, and operational response are implemented and tested.** If an
uptime commitment for Pro is wanted, it requires its own later phase
covering worker-failure detection, durable remote backups, RTO/RPO
definitions, replacement-worker restoration, routing cutover, and periodic
disaster-recovery drills.

**Dependencies:** Phases 2-5 must be in production first; this phase is
primarily a control-plane change (route new Pro signups to the Pro pool)
plus deciding which shared-mode feature restrictions
(`docs/shared-hosting.md:59-65`, e.g. no on-node ingestion, no snapshot
admin) are unacceptable for a paying tier and get individually
un-restricted.

**Acceptance gate:** a new Pro signup provisions onto a shared Pro-pool
worker with no dedicated container, at the higher limit tier, with the
existing dedicated path still available as the Enterprise-only fallback,
and with no scheduling or SLA claim made that isn't backed by a shipped
mechanism.

## Phase 7 — Migration and rebalancing

**Goal:** extend the existing export/import path
(`docs/shared-hosting.md:71-91`, already state-hash-verified) to cover
migration in more directions than the one it handles today.

**Corrected symmetry claim:** the existing implementation covers exactly
one direction — shared source → dedicated destination — via
`POST /internal/shared-import`, which is standalone-only. The other three
directions are **not** simple extensions of that path and each needs a
distinct primitive:

| Direction | Required capability |
|---|---|
| Dedicated → shared | Dedicated export + a new shared-admin import |
| Shared Free → shared Pro | Shared export + a new shared-to-shared import |
| Shared Pro → dedicated | Existing direction, generalized by tier |
| Shared worker → shared worker (rebalancing) | Destination admission, import, and source fencing |

This phase must explicitly design an operator-only shared-destination
import endpoint (or safely adapt project registration to import-before-
activation) before any direction other than shared→dedicated is
attempted.

**Corrected consistency claim:** "leaves the source untouched and serving"
on failure is necessary but not sufficient — it does not by itself
describe a consistent migration. **The source remains authoritative
throughout migration. Writes are fenced during the final transfer window.
On failure, the destination is discarded or quarantined, the source is
reactivated (if it was paused), and `projects.node_url` remains unchanged.**
State-hash comparison alone does not prevent writes reaching the old node
after cutover, or two migrations racing on the same project — fencing is
required in addition to the hash check.

**Idempotency and fencing, required per migration:** a `migration_id`;
source project ID; source generation/version; expected source state hash;
destination state; a write-fence token or generation number; idempotent
retry behavior (a retried migration request must not double-apply);
a cutover record; cleanup status; and a defined recovery procedure if the
control-plane crashes mid-migration.

**Invariant (unchanged from before):** every migration compares the
expected state hash before switching `projects.node_url` — the existing
paid-upgrade path's rule, extended to every direction above, not relaxed
for any of them.

**Planner ownership, stated explicitly:** this phase's operations are
inherently cross-*worker*, unlike Phase 4B's in-process lifecycle
operations. Before implementation, decide and document one of:
(a) extend `valori-planner` with Cloud-executed lifecycle capabilities, or
(b) introduce a control-plane adapter that executes planner-style tasks
against workers it doesn't itself run. This decision must be made and
recorded before Phase 7 implementation starts — "use `valori-planner`"
alone does not answer which process owns and persists the execution graph
for an operation spanning two separate worker processes.

**Dependencies:** Phase 5 (rebalancing needs capacity-aware placement to
choose a destination) and Phase 6 (a Pro pool must exist as a migration
target).

**Acceptance gate:** each of the four migration directions completes with
a verified state-hash match under write fencing; a migration_id is
idempotent under retry; on failure or control-plane crash mid-migration,
the source is recoverable to serving with `node_url` unchanged and no data
loss.

## Out of scope for this spec

- Concrete numeric defaults for any limit, timeout, or oversubscription
  ratio — Phase 1's measurements set those; guessing them now would be
  exactly the mistake this correction is meant to avoid.
- Any change to per-project isolation guarantees (independent `Engine`,
  WAL, state root per project) or to the project-ID-in-path +
  per-project-token routing model — both are correct as built and are
  invariants for every phase above, not decisions up for revision.
- A weighted fair-admission scheduler and an infrastructure SLA for Pro —
  explicitly deferred, not implied by Phase 6 (see its corrected claims).
- The RAG demo work (separate spec:
  `docs/superpowers/specs/2026-09-12-rag-demo-design.md`).
