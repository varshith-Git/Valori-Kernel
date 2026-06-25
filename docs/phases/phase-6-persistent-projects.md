# Phase 6 — Persistent, isolated projects (UI workspace)

## Goal

Make each UI "project" a fully isolated, persistent session — its own data dir,
node process, and port — so reopening the app shows every existing project and
one click resumes it exactly where it left off. Data lives in a protected folder
deletable only through the UI, and every session close auto-snapshots so the next
open is instant.

## Delivered

**New — UI server layer**
- `ui/src/lib/server/projects.ts` — manifest CRUD at `~/.valori/ui-projects.json`
  (kept distinct from the CLI wizard's `projects.json`), free-port allocation
  (3010–3999), path derivation, and `protect()/unprotect()` via macOS
  `chflags uchg/nouchg` (Linux fallback: `chmod`). Also `importFromTmp()` for the
  legacy `/tmp/valori-n1.*` data.
- `ui/src/app/api/projects/route.ts` — `GET` (manifest + live status), `POST` create.
- `ui/src/app/api/projects/[name]/route.ts` — `DELETE` (stop → unprotect → rm → drop entry).
- `ui/src/app/api/projects/[name]/open/route.ts` — `POST`: unprotect, auto-start node,
  wait for `/health`, switch the proxy via `setApiUrl`, bump `lastOpenedAt`.
- `ui/src/app/api/projects/[name]/close/route.ts` — `POST`: snapshot-on-close, stop,
  wait for exit, re-`protect` at rest.
- `ui/src/lib/hooks/useProjectManifest.ts` — Home/sidebar hook (`create/open/close/remove`).
- `ui/src/app/projects/[name]/layout.tsx` — ensures the node is up + connected for the
  whole `/projects/<name>/*` subtree (covers deep links + refreshes).

**Modified — UI**
- `process-manager.ts` — `startProject()`, `snapshotThenStop()`, `isRunning()`, and
  leading-`~/` expansion + parent-dir `mkdir` for all node paths.
- `app/page.tsx` — Home is now the project picker (status pill, last-opened, Open/Close/Delete).
- `components/layout/Sidebar.tsx` — project list sourced from the manifest (all projects,
  running-dot), collections still from the live active node.
- `components/projects/CreateProjectDialog.tsx` — collects dim + index (per-project now).
- `app/projects/page.tsx` — redirects to Home (canonical picker).
- `app/launch/page.tsx` — `/tmp/valori-n*` defaults → `~/.valori/cluster/*`.

**Modified — node**
- `crates/valori-node/src/main.rs` — standalone `axum::serve` now uses
  `with_graceful_shutdown(shutdown_signal(...))`; on SIGTERM/Ctrl-C it writes a final
  snapshot if a snapshot path is configured (the durable backstop; the WAL already
  guarantees no data loss).

## Findings

- **`chflags uchg` blocks the owner too.** The immutable flag stops the node's own WAL
  appends and snapshot writes — so protection must be applied only **at rest** (node
  stopped) and cleared on open. The open/close routes own this lifecycle.
- **Manifest path collision.** The CLI `valori setup` wizard already writes
  `~/.valori/projects.json` in an incompatible `{projects:[...]}` cluster schema. Reusing
  that path crashed `listProjects().map`. Fixed by using `ui-projects.json` and making the
  reader return `[]` for any non-array shape.
- **Dev-HMR singleton staleness.** New `ProcessManager` methods aren't visible to the
  `global.__valori_pm__` instance until a full dev-server restart. Production-irrelevant.

## Validation

- `cargo test -p valori-kernel -p valori-node` → **243 passed, 0 failed**.
- `cargo build -p valori-kernel --target wasm32-unknown-unknown` → ok (no_std invariant held).
- `tsc --noEmit` on the UI → clean.
- End-to-end against a live dev server + debug node binary:
  1. create `varshith` → dir + port 3010 allocated;
  2. open → node boots, `/health` ok, `events.log` created;
  3. insert a 768-dim record → `event_log_height: 1`;
  4. close → `current.snap` written (11 385 B), both files gain `uchg`, node down;
  5. `rm current.snap` → **Operation not permitted** (file survives);
  6. reopen → `records: 1` restored;
  7. delete → flag cleared, dir + manifest entry gone, CLI `projects.json` intact;
  8. graceful-shutdown backstop: delete snap, `kill -TERM` the node directly → snapshot
     rewritten before exit.

## Follow-ups

- **Multi-project connection model.** Only one project is "active" in the proxy at a time;
  switching projects switches the global API URL. A future phase could namespace the proxy
  per-tab for true concurrent project views.
- **Auto-close on switch.** Opening many projects leaves their nodes running until closed
  from Home; consider an LRU idle-close. Deferred.
- **CLI ↔ UI project reconciliation.** The CLI wizard's cluster projects and the UI's
  per-project nodes remain separate notions. Unifying them is out of scope here.
- **Linux immutable flag.** `chflags` is macOS-only; Linux falls back to perms (`chattr +i`
  needs root). A privileged-helper option could harden Linux later.
