# Valori control plane — who owns what

RFC-0006 introduced `valori-daemon` as the sole owner of project/workspace
lifecycle, replacing a TypeScript process manager that used to live inside
`ui/`. This page is the ownership contract that migration produced — read it
before adding a route, a page, or a subprocess spawn anywhere in the stack.
The full narrative (why, and the exact routes migrated) lives in the phase
docs; this page is the rule, not the history.

---

## The layering

```
Desktop (Tauri)
    │  loads ui/ as its frontend — no separate React app
    ▼
ui/ (Next.js)
    │  React pages / hooks never talk to the daemon or a node directly —
    │  only to ui/'s own API routes (`/api/*`), which are the compatibility
    │  layer. That boundary is what lets the backend evolve without touching
    │  a single page.
    ▼
Next API routes (ui/src/app/api/*)
    │
    ├─▶ valori-daemon    — project/workspace/collection/model LIFECYCLE
    │       │               (create, list, delete, start, stop, restart)
    │       ▼
    │   Runtime (LocalRuntime today; Docker/SSH could implement the same
    │   `Runtime` trait later without daemon changes)
    │       │
    │       ▼
    │   valori-node       — spawned and supervised BY the daemon
    │
    └─▶ valori-node       — DATA operations, called directly once a project
                            is open (search, insert, GraphRAG, audit, tree,
                            community, timeline, snapshots, metrics, ...)
                                │
                                ▼
                            valori-engine → valori-planner → kernel/storage
```

## The one rule

| Concern | Owner | Never |
|---|---|---|
| **Lifecycle** — does a project exist, is it running, start/stop/restart, workspaces, models | `valori-daemon` **only** | Don't spawn or supervise a `valori-node` process from `ui/` or `desktop/` for anything that fits the daemon's project model (see exceptions below). Don't add a second on-disk project manifest anywhere. |
| **Data operations** — search, insert, delete-record, GraphRAG, community, tree, audit, timeline, snapshots, metrics | `valori-node` **only**, called directly once the daemon reports a project's URL | Don't route data operations through the daemon "for consistency" — that turns it into a proxy bottleneck for every request. The daemon tells you `project → running → localhost:PORT`; you talk to that URL for everything else, same as `kubectl` (API server) vs. a Pod IP (Service). |
| **Storage / execution** | `valori-engine` / `valori-planner` | Don't reach past `valori-node`'s HTTP surface into the kernel from anywhere outside the node process. |
| **Frontend** | `ui/` — the **one** React application | Don't build a second frontend (a prior pass briefly started one under `desktop/src`; reverted — see `desktop/README.md`'s decision record). `desktop/` is Tauri chrome around `ui/`, plus native-only concerns (window state, OS dialogs, eventually the bundled daemon binary). |

## Explicitly out of scope for the daemon (by design, not by omission)

Two things intentionally still spawn `valori-node` outside the daemon —
documented at their call sites (`ui/src/lib/server/process-manager.ts`), not
silently:

1. **3-node Raft cluster projects** (`replication === 3`). The daemon can
   *persist* cluster topology (`ProjectManifest.cluster`, RFC-0006 Phase B.0)
   but can't *launch* one yet — no Raft-join, no multi-node health
   coordination. Cluster projects still launch through the pre-migration
   `pm`-based path. Migrating this is future work, not a gap introduced by
   Phase B.1.
2. **The manual "advanced launch" playground** (`ui/src/app/launch/page.tsx`,
   `/api/launch*`). An ad-hoc, unnamed node/cluster sandbox with no project
   manifest at all — structurally outside anything a project-oriented daemon
   API could represent. Not a candidate for migration; it's a different tool
   for a different (debugging/exploration) job.

If you find a third place spawning `valori-node` outside the daemon that
*isn't* one of these two and doesn't have a comment explaining why, that's a
bug — file it or fix it.

## Status

| Piece | State |
|---|---|
| Single-node project lifecycle (create/list/delete/open/close) | **[wired]** — via `valori-daemon`, see `crates/valori-daemon`, `ui/src/lib/server/daemon.ts` |
| Cluster project lifecycle | **[legacy path]** — `process-manager.ts`, unchanged; daemon persists metadata only |
| Project registry migration (`ui-projects.json` → daemon) | **[wired]** — one-time, idempotent, see `crates/valori-daemon/src/migration/` |
| `ui/`'s old TS lifecycle code (`projects.ts` file-backed functions, superseded exports) | **[deprecated, not deleted]** — kept one release for rollback safety; delete only after that window, and only once `grep` confirms zero remaining imports |
| Daemon-launched cluster projects | **[not implemented]** — schema-ready (Phase B.0), no launch behavior |
| Desktop bundling its own daemon binary (vs. the developer running one manually) | **[not implemented]** — `desktop/` today just points at `ui/`'s dev server; see `desktop/README.md` |

## Where to look

| Question | File |
|---|---|
| What can the daemon's HTTP API do today? | `crates/valori-daemon/src/http.rs` (doc comment lists every route) |
| What does a project's full manifest contain? | `crates/valori-daemon/src/project.rs` — `ProjectManifest` |
| How does the daemon-backed `/api/projects/*` adapter work? | `ui/src/lib/server/daemon.ts` (1:1 wire client), `ui/src/lib/server/project-adapter.ts` (shape bridge) |
| Why does `process-manager.ts` still spawn nodes? | Top-of-file comment in `ui/src/lib/server/process-manager.ts` |
| Full RFC | `rfcs/0006-desktop-daemon-architecture.md` |
