# Phase D2 — Node Runtime

## Goal

Decompose the monolithic `Supervisor` before it becomes a God object, and
introduce a `Runtime` abstraction so future backends (Docker, SSH, remote
cluster) slot in with **no change** to the daemon, the API, or the desktop.
Add the observability the desktop will need: per-node resource stats, a
Docker-style event stream, and stable resource IDs.

## Delivered

| File | Contents (one reason to change each — SRP) |
|---|---|
| `src/runtime/mod.rs` | `trait Runtime` (start/stop/restart/status/resources/describe/…), `NodeInfo`, `NodeStatus` |
| `src/runtime/local.rs` | `LocalRuntime` — supervised local `valori-node` process (the refactored Supervisor); the first `Runtime` impl |
| `src/runtime/port.rs` | `PortAllocator` — pick a free port |
| `src/runtime/resource.rs` | `ResourceMonitor` + `ResourceStats` — CPU/RAM/threads via `ps` (no platform crate) |
| `src/runtime/policy.rs` | `RestartPolicy` — Always / OnFailure / Never |
| `src/events.rs` | `EventLog` — bounded in-memory ring buffer of lifecycle events |
| `src/daemon.rs` | Holds `Box<dyn Runtime>` + `EventLog`; records events on lifecycle ops; `with_runtime()` constructor for future backends |
| `src/project.rs`, `src/workspace.rs` | Stable `id` (UUID v4) field — names become mutable labels |
| `src/http.rs` | New routes: `GET /v1/events`, `GET /v1/projects/:name/runtime`; `id` in every resource response |
| `src/supervisor.rs` | **Deleted** — logic moved into the decomposed runtime |

## Architecture

```
Daemon
  ├── WorkspaceManager
  ├── ProjectManager
  ├── Box<dyn Runtime>   ← LocalRuntime today; Docker/SSH/Remote later
  │     ├── PortAllocator
  │     ├── ResourceMonitor
  │     └── RestartPolicy
  └── EventLog           ← everything publishes here (seed of the event bus)
```

No component knows another; the `Daemon` composes them. `Runtime` knows
lifecycle + status only — nothing about workspaces, HTTP, or events.

## Findings

- **`Runtime` needs `async_trait`** — start/stop are async (process spawn +
  health poll); `Box<dyn Runtime>` with `#[async_trait]` keeps the daemon
  backend-agnostic.
- **Resource stats via `ps`** avoid a platform crate and work on macOS + Linux
  (`ps -o %cpu=,rss=`). Threads come from `/proc` on Linux only.
- **Stable IDs, mutable names.** Every resource gets a UUID `id`; routes stay
  by `name` for usability, but `id` is the identity that survives renames.
  Manifests without `id` (none exist yet) serde-default to a fresh one.
- **`RestartPolicy` is defined, not yet enforced** — the health-driven restart
  loop (backoff, crash reason) is a later milestone; the vocabulary is in place
  so that logic never leaks into the launcher.

## Validation

```
cargo test -p valori-daemon
  → 8 unit (project, workspace, port, policy, events) + 2 e2e = 10 passed, 0 failed
```

Live HTTP smoke test, all green:
- project create → response carries a UUID `id`
- `GET /v1/config` → `runtime: { kind: "local", node_binary, node_port_range }`
- `GET /v1/projects/healthcare/runtime` → `cpu_percent: 1.7, memory_mb: 20.9, uptime_secs` (real, via `ps`)
- `GET /v1/events` → `project.created → project.started → project.stopped`
- no orphaned processes.

## Follow-ups

| Item | Milestone |
|---|---|
| Health-driven restart loop (enforce `RestartPolicy`, backoff, crash reason, restart count) | D2.1 |
| Richer status states (`starting`/`stopping`/`failed`/`restarting`) | D2.1 |
| SSE / WebSocket push for `/v1/events` (desktop subscribes, no polling) | later |
| `DockerRuntime` / `SshRuntime` / `RemoteRuntime` behind the same trait | later |
| Promote IDs to the primary key in routes (optional) | D3 |
| Full event bus (managers publish; runtime/logger/desktop subscribe) | later |
