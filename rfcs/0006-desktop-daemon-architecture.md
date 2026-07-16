# RFC-0006: Desktop Daemon Architecture

**Status:** Draft
**Owner:** `valori-node` (daemon), `ui` → future `desktop` (Tauri control plane)
**Stability:** Proposed — no code committed against this yet
**Last reviewed:** 2026-07-13
**Branch:** Node-scaleup

---

## Summary

Introduce a long-running **Valori Daemon** that owns everything *outside* the
execution engine — workspaces, projects, models, plugins, updates, process
supervision, and a single API gateway — while **reusing `valori-node` as-is**
for execution. A Tauri desktop app is the control-plane UI on top; it is the
*last* component built, not the first.

The daemon is **not a new backend**. It replaces the ~780 lines of TypeScript
process management currently living in the Next.js UI
(`ui/src/lib/server/{process-manager,projects,cluster-config}.ts`) with a Rust
supervisor, and puts a single front door in front of the existing HTTP server.

> Mental model: **Docker Desktop for AI Memory.** `dockerd` supervises
> containers and exposes one API; you never talk to a container's port
> directly. The Valori Daemon supervises `valori-node` instances and exposes
> one API; the desktop never sees per-project ports.

---

## Motivation

Today a **project = one `valori-node` process on its own port** (`projects.ts`
allocates 3010–3999 for single-node projects; `process-manager.ts:115` sets
`VALORI_BIND`). This works but has three problems for a desktop product:

1. **Ports leak into the UX.** The user (and the UI) must know which port a
   project lives on.
2. **Process management lives in TypeScript**, coupled to the Next.js server —
   it cannot be reused by the CLI, and it disappears if the frontend becomes a
   static Tauri bundle.
3. **No unifying context.** There is no "workspace" — the layer a user actually
   opens and works within (cf. VS Code workspaces, Postman workspaces).

This RFC keeps the parts that work (process-per-project isolation, one backend
implementation) and fixes the parts that don't (ports in the UX, supervision
stuck in TS, no workspace).

---

## Non-goals / invariants preserved

- **`valori-node` stays a standalone binary.** CLI, Docker, Kubernetes,
  benchmarks, and tests keep running the exact same server. The daemon
  *composes* it; it does not replace or fork it.
- **`Engine` stays memory-only.** It knows Memory / Index / Snapshots /
  Receipts / Audit / Kernel / Persistence — and nothing about workspaces,
  users, plugins, auth, or desktop. (Already true post-RFC-0005 / the
  `valori-engine` extraction; this RFC must not regress it.)
- **Collections remain namespaces.** No process or port per collection.

---

## Deployment modes

The daemon abstracts *where execution happens* behind one internal interface.
The UI talks only to the daemon and is identical across all three.

| Mode | Path | Status today | Use case |
|---|---|---|---|
| **Embedded** | Daemon hosts `valori-node` as a **library** in-process (`build_router(SharedEngine) -> Router`) | **NET-NEW** (the only new mode) | Personal / single-user desktop |
| **Supervised** | Daemon spawns & supervises external `valori-node` **processes** | **EXISTS** (`process-manager.ts`, to be ported to Rust) | Development, debugging, crash isolation |
| **Remote** | Daemon proxies to a **remote** node / cluster over HTTP | **EXISTS** (`ui/src/lib/server/connection.ts` resolves `VALORI_API_URL`) | Enterprise / cloud |

Two of the three modes already exist in some form. `build_router` returning a
mountable `axum::Router` is what makes **Embedded** a library binding rather
than a fork.

**v1 recommendation:** ship **Supervised** first (it's a direct port of proven
TS logic and gives free per-project crash isolation + independent restart),
with the interface designed so **Embedded** and **Remote** slot in behind it
without UI changes.

---

## Layered architecture

```
User
  │
Tauri Desktop            ← control-plane UI (built LAST)
  │  one API, project-scoped session
Valori Daemon            ← owns everything outside the engine
  ├── WorkspaceManager       (NET-NEW — the missing top layer)
  ├── ProjectManager         (port of process-manager.ts / projects.ts)
  ├── ModelManager           (NET-NEW — ONNX/Ollama/OpenAI/… abstraction)
  ├── PluginManager          (NET-NEW — later)
  ├── ProcessSupervisor      (port of process-manager.ts)
  ├── UpdateManager          (NET-NEW — Tauri updater backend)
  ├── LicenseManager         (NET-NEW — later)
  └── API Gateway            (NET-NEW — short→canonical path expansion, routing)
        │
        ▼
valori-node  (embedded | supervised | remote)   ← REUSED AS-IS
  └── Planner → ExecutionGraph → Effect Runner → Engine → Kernel → Storage
```

Everything below `valori-node` is reusable across desktop, servers, Kubernetes,
CI, and cloud. Everything above it is desktop/orchestration concern and lives in
the daemon — never in `Engine`.

---

## API contract: path is truth, token is sugar

Two request forms, one audit reality.

**Canonical (the wire truth):**
```
POST /v1/projects/{project}/collections/{collection}/search
```
Self-describing: the target is in the URL, so it lands in logs, receipts, and
the event chain verbatim. This is the form the **audit log always records**.

**Context form (desktop convenience):**
```
POST /v1/search      + project-scoped session/token
```
The desktop already knows the current Workspace → Project → Collection; there
is no reason to repeat `healthcare/chat` on every request. The **daemon expands
the short form into the fully-qualified canonical path** before it reaches the
engine, so:

- desktop APIs stay short,
- SDKs stay simple,
- **the audit trail still records the fully-qualified path.**

Scoping reuses the existing per-tenant key mechanism (`api_keys.rs`,
`ApiKeyRecord { scope, collection }`) extended with a **project** dimension —
this is an extension of a Phase-3.5 primitive, not net-new auth.

**Rationale:** for an audit-first system, concealing the request target in a
header that is gone by the time anyone reads the log is a downgrade. The path
is the truth; the token is convenience.

---

## Project / collection / scaling model

| Unit | Cost | Scale |
|---|---|---|
| **Collection** | A namespace inside a node (16-bit id, intrusive list) | Hundreds per project are fine — always were |
| **Project** | One `Engine` / `KernelState` / audit chain (one process in Supervised mode) | Dozens comfortably; the unit that costs a process/port |
| **Workspace** | A curated set of projects + models + settings | Cheap metadata |

The earlier "one port per collection can't scale to 500" concern conflated two
units: **500 collections live in one node as namespaces**; only 500 *isolated
projects* would be 500 processes. Process-per-project (Supervised) is fine for
realistic desktop density; **Embedded** (in-proc `Map<ProjectId, Engine>`) is
the answer only if extreme project density appears.

---

## Isolation model

- **Supervised (v1):** each project is a separate OS process → free crash
  domains, independent memory limits, independent restart (kill/respawn),
  separate BLAKE3 audit chains. A fault in "Finance" cannot touch "Healthcare."
- **Embedded (later):** one process hosts `Map<ProjectId, Engine>` → leaner
  memory, but a **shared crash domain** and per-project audit multiplexing
  become a design responsibility, and independent restart is harder. Choose it
  only when project density demands it.

The `SchedulerCapability` seam already exists (`valori-effect/src/capability.rs`,
currently always `None`) as the future home for per-project worker/scheduler
control.

---

## Observability contract: `_execution`

The daemon and UI consume ONE observability payload, opt-in via `?explain=true`
(EXPLAIN-style; default responses are byte-identical). Live today on
`POST /v1/memory/search_vector` (`crates/valori-node/src/routes/explain.rs`):

```json
{
  "_execution": {
    "operation": "MemorySearch",
    "model": "INLINE",                 // DIRECT | INLINE | (future) CACHED
    "graph_hash": "e841686b…",         // content-addressed ExecutionGraph
    "operation_hash": "2f244747…",     // BLAKE3(kind‖inputs‖policy)
    "state_hash": "83a14ff7…",         // post-execution BLAKE3 state
    "tasks": [{ "id": 0, "kind": "MemorySearch", "shard": 0 }],
    "edges": [],
    "duration_ms": 0.086,              // graph execution (not full request)
    "planner": { "model": "INLINE", "cache": false, "version": "A13" }
  }
}
```

One payload serves CLI, SDK, UI, MCP, and benchmarks. `duration_ms` is currently
graph-execution time; **per-task** Input/Output/Duration is deferred to runner
instrumentation (same work as the per-crate flamegraph). Fan-out to all
endpoints + cluster parity is tracked separately.

---

## What exists vs. net-new (grounded)

| Already exists (reuse) | Net-new (daemon layer) |
|---|---|
| `build_router(SharedEngine) -> Router` — server is library-shaped (`server.rs:208`) | WorkspaceManager (top layer) |
| `Engine` per store, memory-only (`valori-engine`) | ProjectManager + ProcessSupervisor (port ~780 LOC of TS) |
| Collections = namespaces; snapshots; receipts; audit chain | API Gateway (short→canonical expansion, project routing) |
| Per-tenant keys `ApiKeyRecord { scope, collection }` (`api_keys.rs`) | Project dimension on the key/session |
| Remote connection resolution (`connection.ts` / `VALORI_API_URL`) | Embedded mode (library host) |
| `_execution` block (`routes/explain.rs`) | ModelManager, PluginManager, UpdateManager, LicenseManager |
| `SchedulerCapability` seam (unwired) | Per-project scheduler/worker wiring |

---

## Sequencing

1. **This RFC** — freeze the contract (modes, routing, isolation, scaling).
2. **Daemon skeleton** — `ProcessSupervisor` + `ProjectManager` in Rust
   (Supervised mode), porting `process-manager.ts` / `projects.ts`. Single API
   gateway with short→canonical expansion. Standalone `valori-node` = "daemon
   with one project" as a special case.
3. **WorkspaceManager + ModelManager.**
4. **Embedded mode** — library host behind the same interface.
5. **Tauri desktop** — static Next export + native commands, consuming the
   daemon API. Built last.

---

## Open questions

- **Embedded audit multiplexing:** exact on-disk layout for N per-project
  event logs in one process (likely mirrors per-shard `events-shardN.log`).
- **Session model for the context API:** cookie/session vs. bearer token for
  binding "current project"; interaction with the existing key store.
- **ModelManager scope for v1:** provider abstraction (Ollama/ONNX/OpenAI/…)
  vs. also a Docker-Images-style local model registry/download manager.
- **Daemon supervision of clusters:** how the three-node cluster projects
  (existing `cluster-config.ts`) map onto ProjectManager in Supervised mode.
