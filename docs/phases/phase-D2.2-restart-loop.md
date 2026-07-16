# Phase D2.2 — Restart Loop & Health FSM

## Goal

Make the daemon self-healing: detect crashed nodes and restart them per an
operator-set `RestartPolicy`, with backoff and crash tracking. Enrich the
lifecycle state machine with recovery-aware states (review point 4). Keep the
policy in the **operational** layer (a `Supervisor`), not the runtime.

## Delivered

| File | Change |
|---|---|
| `src/supervisor.rs` | New — operational `Supervisor`: per-node policy, crash state, restart count, last crash reason, capped exponential backoff (2→60s). `on_started/on_stopped/on_crash/due_for_restart/on_restart_{success,failure}`, `SupervisionInfo` API overlay |
| `src/runtime/state.rs` | `RuntimeState` gains `Recovering` (auto-restart after crash ≠ fresh `Starting`, because Valori replays its event log); new legal transitions `Failed→Recovering→Running\|Failed` |
| `src/runtime/launcher.rs` | `RunningProcess::has_exited()` — non-blocking crash detection (`try_wait`) |
| `src/runtime/mod.rs`, `local.rs` | `Runtime::poll_exits()` — sweep dead processes, drop them, return `NodeExit`s |
| `src/project.rs` | `ProjectConfig.restart_policy` (serde default `never` — safe, no behaviour change) |
| `src/daemon.rs` | Owns the `Supervisor`; registers on start/stop; `supervise_tick()` = detect crashes → restart due nodes; merges supervisor state (Failed/Recovering) + restart count into `NodeInfo` |
| `src/main.rs` | Background monitor task: `supervise_tick()` every 2s |
| `src/http.rs` | `restart_policy` on create; `supervision` (restarts/last_crash/policy/state) in project responses |

## Design

Separation of concerns (review point 3): the **runtime** detects exits
(`poll_exits`) and executes `start`/`stop`; the **supervisor** decides *whether*
to restart (policy + backoff) and owns crash bookkeeping. The daemon's monitor
tick wires them: `runtime.poll_exits()` → `supervisor.on_crash()` →
`supervisor.due_for_restart()` → `runtime.start()`.

`RestartPolicy`: `never` (default) | `on_failure` | `always`. Backoff is capped
exponential per node; restart count and last crash reason are tracked and
surfaced in the API.

## Findings

- **Fine node-internal states deferred, honestly.** The daemon can observe
  `Stopped/Starting/Running/Stopping/Failed/Recovering` (process + `/health`),
  but NOT `ReplayingEvents`/`LoadingIndex` — those require the *node* to expose
  a startup-phase field in `/health`. Left as a node-side follow-up rather than
  reported as a state the daemon can't actually see.
- **Tick holds the daemon lock during a restart** (spawn + health wait). Fine
  for rare crashes; a future refinement can restart off the critical path.

## Validation

```
cargo test -p valori-daemon
  → 11 unit (+ supervisor policy/backoff) + 3 e2e = 14 passed, 0 failed
```

The e2e `supervisor_restarts_crashed_node` **kills a real node** and asserts the
supervisor restarts it (new PID, `restarts == 1`, back to `Running`).

Live smoke test with the **background monitor** (not a manual tick):
- start project (`restart_policy: always`) → `kill -9` the node →
- ~6s later: `status: running`, new PID, `supervision.restarts: 1`
- events: `project.created → started → crashed → recovering → restarted`
- no orphaned processes.

## Follow-ups

| Item | Milestone |
|---|---|
| Node-side startup phases (`ReplayingEvents`/`LoadingIndex`) via `/health` | node task |
| Health-failure (not just process-exit) as a crash trigger — periodic `/health` liveness | D2.3 |
| Restart off the critical path (don't hold the daemon lock across a restart) | D2.3 |
| `ResourceMonitor` → `MetricsProvider` rename as metrics expand (GPU/WAL/index size) | D4 |
