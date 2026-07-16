# valori-daemon

The **Valori Daemon** — a long-running control plane that owns project and
workspace lifecycle and supervises `valori-node` instances. The Rust successor
to the TypeScript process manager in `ui/src/lib/server/`.

See [`rfcs/0006-desktop-daemon-architecture.md`](../../rfcs/0006-desktop-daemon-architecture.md).

> **Model:** Docker Desktop for AI Memory. `dockerd` supervises containers behind
> one API; the daemon supervises `valori-node` instances behind one API. The
> desktop addresses **projects by name** — node ports are an internal detail.

## Status — Milestone 1 + D1.1 (stable API surface)

The daemon is the single API every client (desktop, CLI, SDK) talks to — no
client addresses `valori-node` directly. Everything is versioned under `/v1`.

**System / discovery**

| Capability | Endpoint |
|---|---|
| Daemon health | `GET /health` |
| Version + API level | `GET /version` |
| Discovery (call this first) | `GET /v1/system` |
| Effective config | `GET /v1/config` |
| Graceful shutdown (D3) | `POST /v1/shutdown` |

**Workspaces** (grouping layer above projects; `default` always exists)

| Capability | Endpoint |
|---|---|
| List (+ project counts) | `GET /v1/workspaces` |
| Create | `POST /v1/workspaces` `{name}` |
| Rename | `PATCH /v1/workspaces/:name` `{name}` |
| Delete (must be empty) | `DELETE /v1/workspaces/:name` |

**Projects**

| Capability | Endpoint |
|---|---|
| List (+ node status) | `GET /v1/projects` |
| Create | `POST /v1/projects` `{name, dim, index?, workspace?, restart_policy?}` |
| Detail (+ `supervision`: restarts/last_crash/policy) | `GET /v1/projects/:name` |
| Delete (must be stopped) | `DELETE /v1/projects/:name` |
| Start / Stop / Restart | `POST /v1/projects/:name/{start,stop,restart}` |
| Node logs (tail) | `GET /v1/projects/:name/logs?tail=N` |

**Self-healing (D2.2):** a background monitor detects crashed nodes and restarts
them per `restart_policy` (`never` default / `on_failure` / `always`) with capped
exponential backoff. Crash count + reason surface under `supervision`; lifecycle
emits `project.crashed` / `project.recovering` / `project.restarted`.

**Collections** (proxied to the running node's namespaces)

| Capability | Endpoint |
|---|---|
| List / Create | `GET\|POST /v1/projects/:name/collections` |
| Delete | `DELETE /v1/projects/:name/collections/:collection` |

**Models (E1-lite)** — daemon model catalog (management only; no inference yet):

| Capability | Endpoint |
|---|---|
| List (installed + available + disk) | `GET /v1/models` |
| Install (remote=register, local=download+SHA-256) | `POST /v1/models/install` `{id}` |
| Detail / Remove | `GET\|DELETE /v1/models/*id` |

Registry ships OpenAI / Ollama / BGE-ONNX. Local models stream-download to
`<home>/models/<id>/` with SHA-256 verification. Each model's `provider` is the
seam the document pipeline's embedder (E2) and a future local-inference
`ModelProvider` (E1-full) dispatch on.

Also live (D2): `GET /v1/events` (lifecycle stream), `GET /v1/projects/:name/runtime`
(CPU/RAM/threads/uptime). Every resource carries a stable UUID `id`.

## Architecture

The daemon depends on **traits**, not concretes — every durable dependency is
injected (`DaemonDeps` + `Daemon::with_deps`); nothing is built internally.

```
HTTP  (http.rs)
  │
Daemon  (daemon.rs)          ← orchestrates injected trait objects, records events
  ├── Box<dyn ProjectStore>    → JsonProjectStore   (project.rs)   | Sqlite/Cloud later
  ├── Box<dyn WorkspaceStore>  → JsonWorkspaceStore (workspace.rs) | Sqlite/Cloud later
  ├── Box<dyn EventStore>      → MemoryEventStore   (events.rs)    | Sqlite/Redb later
  └── Box<dyn Runtime>         → LocalRuntime       (runtime/)     | Docker/SSH/remote later
        ├── Box<dyn Launcher>  → LocalLauncher (runtime/launcher.rs)  ← launches; Docker=bollard later
        ├── PortAllocator      (runtime/port.rs)
        ├── ResourceMonitor    (runtime/resource.rs)   ← CPU/RAM via `ps`
        └── RuntimeState       (runtime/state.rs)      ← Stopped→Starting→Running→Stopping (illegal moves error)

RestartPolicy (policy.rs)    ← operational, above the runtime (not inside it)
```

- **Every layer is replaceable (Open/Closed).** Swap `JsonProjectStore` for
  `SqliteProjectStore`, or `LocalLauncher` for `DockerLauncher`, with no change
  to the daemon, API, or UI.
- **Runtime orchestrates; Launcher launches.** The runtime owns health, state,
  and resources; the launcher owns "turn a spec into a running process" — so a
  `DockerLauncher` returns a container handle the runtime never inspects.
- **Project** = a directory `<home>/projects/<name>/` with `project.json` +
  per-project data (`events.log`, `snapshot.val`). One project → one node.
- **Ports** are allocated internally (8100–8999) and hidden from the client.
- **Stable ids, mutable names.** Every resource has a UUID `id`; routes use
  `name` for usability, but `id` survives renames.
- **Durability:** stop is best-effort graceful (asks the node to snapshot, then
  terminates) — this applies to both stopping one project (`stop()`) and
  shutting the whole daemon down (`stop_all()`, D3): every running node gets
  the same snapshot-then-terminate treatment, not a hard kill. Even a hard
  kill is safe — each node writes a BLAKE3-chained `events.log` that is
  replayed on the next start.
- **Graceful daemon shutdown over HTTP (D3):** `POST /v1/shutdown` — the
  portable way for a supervisor (the desktop app) to stop the daemon. OS
  signals aren't uniform across macOS/Linux/Windows for a process spawned and
  supervised by another process, so this is what `desktop/src-tauri`'s
  `daemon_manager.rs` calls instead of killing the process directly.

## Run

```bash
cargo run -p valori-daemon          # listens on 127.0.0.1:8080
```

Environment:

| Var | Default | Purpose |
|---|---|---|
| `VALORI_HOME` | `~/.valori` | Data root (`<home>/projects/…`) |
| `VALORI_DAEMON_BIND` | `127.0.0.1:8080` | Daemon listen address |
| `VALORI_NODE_BIN` | — | Path to the `valori-node` binary to supervise |
| `VALORI_REPO_ROOT` | cwd | Where to find `target/{release,debug}/valori-node` |

Example:

```bash
curl -X POST localhost:8080/v1/projects -d '{"name":"healthcare","dim":128}'
curl -X POST localhost:8080/v1/projects/healthcare/start
curl localhost:8080/v1/projects        # → running, with pid + internal port
```

## Tests

```bash
cargo test -p valori-daemon            # 3 unit + 2 e2e (real node spawn)
```

The e2e test (`tests/lifecycle.rs`) actually spawns a `valori-node`, confirms
health, then stops it — it skips gracefully if the debug binary isn't built.

## Not yet (next milestones, per RFC-0006)

Workspace layer · Model manager · single-port API gateway (short→canonical path
expansion) · embedded/remote execution modes · live resource stats (RAM/CPU).
