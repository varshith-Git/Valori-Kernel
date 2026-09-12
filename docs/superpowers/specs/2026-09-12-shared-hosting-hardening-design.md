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
  coming back up after a restart.
- No capacity-aware placement across a fleet of shared workers — today's
  control-plane placement (`backend/apps/api/src/provision/placement.rs`)
  targets dedicated per-project hosts, not shared-worker admission.
- Pro-tier shared hosting doesn't exist yet — only Free.

## Roadmap

### Phase 1 — Audit the existing shared runtime (implementation-ready now)

Pure measurement, no code changes. Build a benchmark harness (extends
`benchmarks/`) that runs against a real `SharedHost` instance and reports:

- Resident memory of one empty project's `Engine`.
- Marginal memory per record, broken out by vector dimension and index kind
  (brute/hnsw/ivf/bq) — reuses the existing `VALORI_INDEX` matrix.
- `SharedHost::open()` wall-clock time recovering 10, 50, and 100 registered
  projects from disk (varying per-project record counts).
- Concurrent read/write behavior across projects sharing one process: p50/p95/p99
  latency under N concurrent projects each issuing search+insert traffic.
- Event-log flush contention: does one project's flush (`shared.rs:344-348`,
  the per-request flush in `dispatch()`) measurably stall another project's
  request on the same worker?
- File-descriptor growth as registered-project count increases.
- Failure behavior when one project's `events.log` is corrupted or
  truncated: confirm today's fail-closed startup behavior, and measure how
  many healthy projects it takes down (currently: all of them on that
  worker).

**Deliverable:** a written report with the above numbers plus recommended
per-project memory/CPU budgets. This report is the required input to Phase 2
— Phase 2 does not proceed until it exists.

**Acceptance gate:** benchmark harness runs reproducibly against a local
`SharedHost` instance (documented `cargo bench`/script command); report
committed to `docs/`; explicitly answers "is one corrupt log's blast radius
(currently: the whole worker) acceptable for a production Free/Pro pool, or
does that need fixing before Phase 4."

### Phase 2 — Free/Pro pool classes (design proposal, pending Phase 1 data)

**Goal:** decide whether Free and Pro should run as separate worker pools
(`shared-free`, `shared-pro`) with independent oversubscription and priority
policy, versus sharing one pool class with per-plan limits only.

**Open question Phase 1 must answer first:** what per-project memory/CPU
footprint and what blast-radius-of-failure are acceptable to co-locate Free
and Pro on the same process, if at all. Do not fix the pool topology (one
shared pool vs. two) before that's known.

**Dependencies:** Phase 1 report.

**Not decided yet:** worker-class naming beyond the placeholders above,
oversubscription ratios, how many projects per worker at each tier.

### Phase 3 — Per-project limits

**Goal:** extend the existing project manifest
(`shared.rs:53-58`, currently `{token_hash, max_records, active}`) with
`max_storage_bytes`, `reads_per_minute`, `writes_per_minute`,
`max_concurrent_requests`, `max_resident_memory_bytes` (if Phase 1 shows it's
enforceable), `idle_timeout_seconds`, `plan_class`.

**Invariant:** limits are enforced **in `dispatch()` at the shared worker**,
not only in `valori-ui`'s `quota.ts`. Control-plane-only checks can be
bypassed or go stale relative to the worker's actual state.

**Dependencies:** Phase 1 data informs default limit values; Phase 2 decides
whether limits differ by pool class or just by `plan_class` within one pool.

**Acceptance gate:** a project that exceeds any configured limit is rejected
at the worker with a clear status/error, without affecting other projects'
availability or latency on the same worker.

### Phase 4 — Lazy loading and eviction

**Goal:** replace `SharedHost::open()`'s eager load-everything with
manifest-only startup; load an `Engine` on first request; evict idle
`Engine`s after `idle_timeout_seconds`, flushing and snapshotting before
release. Introduce lifecycle states per project entry (unloaded / loading /
resident / evicting / failed) so a project's state is always well-defined
mid-transition, including under concurrent requests.

**Invariant:** eviction must drain in-flight requests before releasing an
`Engine` — never drop a live request's state out from under it. A corrupt
project's log must fail that project's own load, not the whole worker's
startup (fixes the Phase 1-confirmed blast-radius gap, if Phase 1 shows it's
worth fixing before this phase).

**Ownership boundary:** use `valori-planner` for multi-step lifecycle
operations (relocation, tier upgrade); ordinary search/record requests stay
on the direct `dispatch()` path and never round-trip through Cloud
orchestration for a synchronous read or write.

**Dependencies:** Phase 3's lifecycle/limit plumbing; Phase 1's corrupt-log
finding.

**Acceptance gate:** `SharedHost::open()` startup time is no longer a
function of registered-project count (only of however many manifests exist);
a newly-idle project's memory is measurably released; a corrupted project's
log blocks only that project, not the worker.

### Phase 5 — Capacity-aware placement

**Goal:** replace project-count-only admission (`shared.rs:223-228`) with a
shared worker reporting real capacity signals — resident memory, available
memory, resident engine count, active requests, throughput, disk
usage/free-space, recovery-queue depth, p95/p99 latency, worker
version/health — and have the control-plane's placement logic
(`provision/placement.rs`) use weighted capacity with a hard memory safety
margin, instead of `projects.len() < max_projects`.

**Dependencies:** Phase 4 (accurate resident-memory accounting requires
lazy loading/eviction to be meaningful — a worker that loads everything
eagerly can't distinguish "busy" from "just has many idle tenants").

**Acceptance gate:** the control-plane can place a new project on the
least-loaded eligible worker in a multi-worker pool using real reported
metrics, and refuses placement (rather than silently overloading a worker)
when no worker has a safe memory margin.

### Phase 6 — Extend shared mode to Pro

**Goal:** Pro projects run on the same hardened shared-hosting runtime as
Free, but on a separate Pro pool (if Phase 2 concludes pools should be
separate) with: higher rate/storage limits, longer residency / fewer cold
starts, higher scheduling weight, lower oversubscription, more available
shared-mode features than Free, and a faster support/SLA tier. Enterprise
is unaffected — still dedicated containers/clusters.

**Dependencies:** Phases 2-5 must be in production first; this phase is
primarily a control-plane change (route new Pro signups to the Pro pool)
plus whatever shared-mode feature restrictions
(`docs/shared-hosting.md:59-65`, e.g. no on-node ingestion, no snapshot
admin) are judged unacceptable for a paying tier and get individually
un-restricted.

**Acceptance gate:** a new Pro signup provisions onto a shared Pro-pool
worker with no dedicated container, at the higher limit/SLA tier, with the
existing dedicated path still available as the Enterprise-only fallback.

### Phase 7 — Migration

**Goal:** extend the existing export/import path
(`docs/shared-hosting.md:71-91`, already state-hash-verified) to cover:
dedicated-Free → shared-Free, shared-Free → shared-Pro, shared-Pro →
dedicated-Enterprise, and worker-to-worker pool rebalancing.

**Invariant:** every migration compares the expected state hash before
switching `projects.node_url` — same rule the existing paid-upgrade path
already follows. No migration path is allowed to cut traffic over before
the destination's replayed state hash matches the source's expected hash.

**Dependencies:** Phase 5 (rebalancing needs capacity-aware placement to
choose a destination) and Phase 6 (Pro pool must exist as a migration
target).

**Acceptance gate:** each of the four migration directions above completes
with a verified state-hash match and, on mismatch, leaves the source
project untouched and serving (matching the existing "failed transfers
retain the source" behavior).

## Out of scope for this spec

- Concrete numeric defaults for any limit, timeout, or oversubscription
  ratio — Phase 1's measurements set those; guessing them now would be
  exactly the mistake this correction is meant to avoid.
- Any change to per-project isolation guarantees (independent `Engine`,
  WAL, state root per project) or to the project-ID-in-path +
  per-project-token routing model — both are correct as built and are
  invariants for every phase above, not decisions up for revision.
- The RAG demo work (separate spec:
  `docs/superpowers/specs/2026-09-12-rag-demo-design.md`).
