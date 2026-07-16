# Phase D3 — Desktop launches and manages the daemon; workspace becomes real `VALORI_HOME`

## Goal

Turn the desktop app from "a window pointed at a `valori-node` you started by hand
in a terminal" into a self-contained supervisor: it launches `valori-daemon`
itself, points it at the workspace folder the user picked in onboarding, and
asks it to shut down gracefully (snapshot every running project, then exit)
when the desktop window closes — closing the gap where the Welcome wizard's
folder picker was previously cosmetic.

## Delivered

- `crates/valori-daemon/src/http.rs` — new `POST /v1/shutdown` endpoint: the
  portable, cross-platform mechanism for a supervisor to ask the daemon to
  stop (OS signals aren't uniform across macOS/Linux/Windows for a
  process spawned by another process). Calls `Daemon::shutdown()` then exits
  the process after a short delay.
- `crates/valori-daemon/src/runtime/mod.rs`, `runtime/local.rs` — `Runtime::stop_all()`
  is now `async` and does the same snapshot-then-terminate as a single
  `stop()`, instead of hard-killing every node with no snapshot. This was a
  real, previously-undiscovered durability gap: a daemon shutdown (Ctrl-C, or
  the desktop app closing) silently skipped the "save before exit" guarantee
  that stopping a single project already had.
- `crates/valori-daemon/src/daemon.rs`, `src/main.rs` — `Daemon::shutdown()` is
  now `async`; the Ctrl-C handler and the new HTTP handler both await it.
- `desktop/src-tauri/src/daemon_manager.rs` (new) — owns the `valori-daemon`
  child process from the desktop side: `resolve_daemon_binary()` (env override
  or dev-mode search under the repo root's `target/{release,debug}`),
  `start_daemon` (spawns with `VALORI_HOME` from the chosen workspace, polls
  `/health` until ready, no-ops if already running), `stop_daemon` (calls
  `POST /v1/shutdown`, waits for the child to exit, falls back to a hard kill
  if it doesn't), `daemon_status`.
- `desktop/src-tauri/src/lib.rs` — registers `DaemonState`, the three new
  Tauri commands, and a `RunEvent::ExitRequested` hook that prevents exit,
  awaits graceful daemon shutdown, then lets the app quit — the "Close desktop
  → ask daemon to shutdown → snapshot → exit" sequence.
- `ui/src/lib/native.ts` — `startDaemon(home)`, `stopDaemon()`,
  `daemonStatus()` bridge functions (no-ops outside the desktop shell, same
  pattern as the rest of the file).
- `ui/src/components/onboarding/Welcome.tsx` — `finish()` now calls
  `startDaemon(workspaceDir)` after persisting the workspace choice, so the
  folder picker has a real effect for the first time.
- `ui/src/components/layout/AppShellGate.tsx` — on launch, a returning user
  (onboarding already complete) has the daemon started automatically against
  their previously-persisted `workspaceDir`.

## Findings

- `Runtime::stop_all()` hard-killing nodes with no snapshot was a genuine bug,
  found while building this feature, not reported by the user. Fixed as part
  of this phase since the desktop's graceful-shutdown flow is directly
  load-bearing on it.
- `crates/valori-daemon/tests/lifecycle.rs::supervisor_restarts_crashed_node`
  had a pre-existing timing race: it called `supervise_tick()` immediately
  after `kill -9`-ing the child and asserted the crash would be visible on
  that first tick, but `poll_exits()`/`try_wait()` isn't guaranteed to observe
  the exit that fast — it's a genuine OS-level reap-latency race, not
  something introduced by the `stop_all` change. Fixed by polling
  `supervise_tick()` (with short sleeps) until the restart lands, instead of
  asserting on a specific tick.

## Validation

- `cargo test -p valori-daemon` — 20 passed, 0 failed (17 unit + 3 integration
  in `lifecycle.rs`), including 3 repeated runs of
  `supervisor_restarts_crashed_node` to confirm the timing fix isn't flaky.
- `cargo build` / `cargo build --release` in `desktop/src-tauri` — both clean.
- `desktop/src-tauri/src/daemon_manager.rs` — new
  `start_stop_real_daemon_binary` test: spawns the **real** `valori-daemon`
  binary against a fresh temp `VALORI_HOME`, confirms `/health` responds,
  confirms a duplicate `start_daemon` call reports the same instance instead
  of spawning a second process, then verifies `stop_daemon` gracefully brings
  it down via `POST /v1/shutdown`. Passed.
- `npx tsc --noEmit` in `ui/` — no new errors introduced by
  `native.ts`/`Welcome.tsx`/`AppShellGate.tsx` (remaining errors are
  pre-existing Next.js route-param typing issues, unrelated).

## Follow-ups

- Phase 1 item 3 (bundle the daemon binary into the app via Tauri sidecar) —
  not started; `resolve_daemon_binary()` only does dev-mode discovery today.
- Phase 1 item 4 (produce real installers: `.dmg`/`.msi`/`.AppImage`) — not
  started, depends on sidecar bundling.
- Phase 2 (Tauri auto-updater via GitHub Releases) — deferred per explicit
  user sequencing.
- No UI surface yet shows daemon status (running/healthy) to the user —
  `daemonStatus()` exists on the bridge but nothing calls it from a settings
  or status page. Worth adding once there's a natural place for it.
- `lastWorkspace` app-memory field (from the earlier "Desktop should
  remember" request) still has no read path here — `AppShellGate` reads
  `workspaceDir` directly rather than a separate `lastWorkspace` key; these
  are currently the same value under two different historical names. Worth
  reconciling if a workspace-switcher UI is ever built.
