# Phase D1.1 — Stabilize the Daemon API

## Goal

Make the daemon the single, versioned, discoverable API that every client
(desktop, CLI, SDK, future extensions) talks to — before building any client.
No client should ever address `valori-node` directly. Expand the Milestone 1
surface with system/discovery, workspaces, models (stubs), collections, and
logs, all under `/v1`.

## Delivered

| File | Change |
|---|---|
| `src/workspace.rs` | New `WorkspaceManager` — persisted `<home>/workspaces.json`, `default` always exists, create/rename/delete, name validation, 2 unit tests |
| `src/project.rs` | `ProjectConfig` gains `workspace` field (serde default `"default"` — old manifests still parse) |
| `src/supervisor.rs` | Node stdout/stderr captured to `<project>/node.log`; per-node `uptime_secs`; `binary()` / `port_range()` / `port_of()` / `running_count()` accessors |
| `src/daemon.rs` | Workspace CRUD, `system()`, `config()`, `project_logs()`, and collection proxying (`list/create/delete_collection` → running node's `/v1/namespaces`) |
| `src/http.rs` | Routes: `/version`, `/v1/system`, `/v1/config`, `/v1/workspaces[/:name]`, `/v1/projects/:name/logs`, `/v1/projects/:name/collections[/:collection]`, `/v1/models[...]` |
| `README.md` | Full API table |

## API surface (all under `/v1`)

- **System:** `GET /health`, `GET /version`, `GET /v1/system` (discovery), `GET /v1/config`
- **Workspaces:** `GET|POST /v1/workspaces`, `PATCH|DELETE /v1/workspaces/:name`
- **Projects:** unchanged from D1 + `GET /v1/projects/:name/logs?tail=N`
- **Collections:** `GET|POST /v1/projects/:name/collections`, `DELETE …/:collection` (proxied)
- **Models:** `GET /v1/models` (stub), `POST /v1/models/install` + `DELETE /v1/models/:id` → `501`

## Findings

- **Collections must proxy for now.** Collections are namespaces *inside* a
  node's `KernelState`, so the daemon proxies to the running node's
  `/v1/namespaces`. Requires the project to be started. Daemon-side metadata
  caching (so it "knows" collections without proxying) is a later optimization.
- **Workspace membership is by field, not directory.** Projects carry a
  `workspace` name; deleting a workspace with projects is refused (400). Moving
  projects between workspaces is a D3 concern.
- **Durability unchanged:** node logs are captured to a file for `GET …/logs`;
  the durable state remains the event log.

## Validation

```
cargo test -p valori-daemon
  → 3 project + 2 workspace unit + 2 e2e = 7 passed, 0 failed
```

Live HTTP smoke test (daemon binary + curl), all green:
- `GET /v1/system` → version, platform, pid, uptime, counts (projects/running/workspaces/models)
- workspace create → project created in it → `GET /v1/workspaces` rolls up `projects: 1`
- delete non-empty workspace → **400**; `POST /v1/models/install` → **501**
- start project → `POST …/collections {patient_notes}` → `GET …/collections` lists it **via the node** (real proxy)
- `GET …/logs?tail=N` returns captured node output; `/v1/system` shows `running: 1`

## Follow-ups

| Item | Milestone |
|---|---|
| Rich supervisor status (`starting`/`failed`/`restarting`, CPU/RAM/threads/restarts/crash reason) | D2 |
| Daemon-side collection metadata cache (drop the proxy round-trip) | D3 |
| Project ↔ workspace moves; per-workspace views | D3 |
| Real Model Manager (Ollama/ONNX/OpenAI/…) behind the stubs | D4 |
| Single-port API gateway: short `/v1/search` → canonical project/collection path | later |
