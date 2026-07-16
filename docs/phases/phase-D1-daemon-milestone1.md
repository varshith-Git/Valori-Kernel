# Phase D1 — Valori Daemon, Milestone 1

## Goal

Stand up the **Valori Daemon** (RFC-0006) — the control plane that owns project
lifecycle and supervises `valori-node` instances — as a new `valori-daemon`
crate. Milestone 1 scope: project lifecycle + process supervision over HTTP
(create / delete / start / stop / restart / list / health). No UI. This is the
Rust successor to the TypeScript process manager in `ui/src/lib/server/`.

## Delivered

| File | Contents |
|---|---|
| `crates/valori-daemon/Cargo.toml` | New crate + `valori-daemon` binary; workspace member + default-member |
| `src/project.rs` | `ProjectManager` — filesystem catalog (`<home>/projects/<name>/project.json`), create/get/list/delete, write-then-rename manifest, name validation |
| `src/supervisor.rs` | `Supervisor` — spawn/stop `valori-node`, internal port allocation (8100–8999), `/health` polling, best-effort graceful stop |
| `src/daemon.rs` | `Daemon` orchestrator — composes the two, enforces "no delete while running" |
| `src/error.rs` | `DaemonError` + HTTP status mapping (`IntoResponse`) |
| `src/http.rs` | axum router — the Milestone 1 API surface |
| `src/lib.rs`, `src/main.rs` | Library exports + binary (env-configured bind/home/node-bin) |
| `tests/lifecycle.rs` | 2 e2e tests incl. a **real** node spawn → health → stop → delete |
| `crates/valori-daemon/README.md` | Crate README |
| `Cargo.toml` (root) | Added `valori-daemon` to members + default-members |

## Findings

- `valori-node`'s server being library-shaped (`build_router(SharedEngine) ->
  Router`, RFC-0006) is not needed for Supervised mode — the daemon spawns the
  **binary** and supervises it as a child process, so Milestone 1 has zero
  coupling to `valori-node` as a library. Embedded mode (later) is where the
  library shape matters.
- Durability is free from the event log: even a hard `kill` is safe because each
  node writes a BLAKE3-chained `events.log` replayed on next start. Graceful
  snapshot-on-stop is a best-effort nicety, not a correctness requirement.
- Ports are a pure internal detail — the client only ever names projects. This
  is the concrete mechanism behind RFC-0006's "hide the ports."

## Validation

```
cargo test -p valori-daemon
  → 3 unit (project registry) + 2 e2e (real spawn) = 5 passed, 0 failed
```

Live HTTP smoke test (daemon binary + curl):
- `GET /health` → `{"status":"ok","service":"valori-daemon"}`
- `POST /v1/projects {name,dim}` → project created + persisted
- `POST /v1/projects/:name/start` → real node spawned (PID + internal port 8100), healthy
- `GET /v1/projects` → running, with pid + port
- `DELETE` while running → **409** (rule enforced)
- `stop` → stopped; `DELETE` → **200**
- Clean shutdown, no orphaned processes.

Root workspace unaffected: `valori-node` still builds; `valori-daemon` is a new
member.

## Follow-ups

| Item | Owner milestone |
|---|---|
| Live resource stats (RAM/CPU/uptime per node) | D2 (Process Supervisor) |
| Workspace layer above projects | D3 (Workspace) |
| Model manager (Ollama/ONNX/OpenAI/…) | D4 |
| Single-port API gateway (short→canonical path expansion) | later |
| Embedded + remote execution modes | later |
| Graceful SIGTERM (vs snapshot-then-kill) | D2 |
| Persist port assignments across daemon restarts / reconcile orphans | D2 |
