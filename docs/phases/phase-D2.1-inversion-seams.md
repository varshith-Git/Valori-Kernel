# Phase D2.1 — Dependency-Inversion Seams

## Goal

Introduce the foundational abstraction seams before D3/D4/desktop, so those
build on a stable core instead of reopening the daemon. Turn concrete
dependencies into injected trait objects (DIP), split the launcher out of the
runtime (SRP), move restart policy to the operational layer, and make node
lifecycle an explicit state machine.

## Delivered

| Seam | Trait | Impl today | Later (no daemon change) |
|---|---|---|---|
| Persistence — projects | `ProjectStore` | `JsonProjectStore` | Sqlite/Postgres/Cloud |
| Persistence — workspaces | `WorkspaceStore` | `JsonWorkspaceStore` | Sqlite/Cloud |
| Event stream | `EventStore` | `MemoryEventStore` | Sqlite/Redb/append-log |
| Process launch | `Launcher` + `RunningProcess` | `LocalLauncher` / `LocalProcess` | Docker(bollard)/SSH(openssh) |
| Node lifecycle | `RuntimeState` (state machine) | — | — |

| File | Change |
|---|---|
| `src/store.rs` | New — `ProjectStore`, `WorkspaceStore` traits |
| `src/project.rs` | `ProjectManager` → `JsonProjectStore` implementing `ProjectStore` |
| `src/workspace.rs` | `WorkspaceManager` → `JsonWorkspaceStore` implementing `WorkspaceStore` |
| `src/events.rs` | `EventLog` → `EventStore` trait + `MemoryEventStore` |
| `src/runtime/launcher.rs` | New — `Launcher` + `RunningProcess`; `LocalLauncher`/`LocalProcess` (std::process). Runtime orchestrates; launcher launches |
| `src/runtime/state.rs` | New — `RuntimeState` machine (Stopped→Starting→Running→Stopping→Stopped; illegal moves error) |
| `src/runtime/local.rs` | `LocalRuntime` now holds `Box<dyn Launcher>` + tracks `RuntimeState`; `with_launcher()` for injection |
| `src/policy.rs` | `RestartPolicy` moved OUT of `runtime/` — it is operational (whether a node should exist), not runtime (how to run it) |
| `src/daemon.rs` | `Daemon` holds `Box<dyn ProjectStore/WorkspaceStore/Runtime/EventStore>`; `DaemonDeps` + `with_deps()` inject everything; `new()` wires the defaults |
| `src/runtime/mod.rs` | `NodeInfo.status` is now `RuntimeState` (was a bespoke `NodeStatus`) |

## Findings

- **`&self` async + trait objects forces `Sync`.** The collection-proxy methods
  borrow `&Daemon` across an `await`, so `Daemon` must be `Sync`, so
  `RunningProcess` must be `Send + Sync`. `std::process::Child` is `Sync`, so
  `LocalProcess` qualifies — no `unsafe`, no wrapper needed.
- **Launcher returns `Box<dyn RunningProcess>`, not a `Child`.** That is what
  lets a `DockerLauncher` return a container handle instead of an OS process —
  the runtime never sees `std::process`.
- **RestartPolicy is defined, not yet enforced** (unchanged from D2) — but it
  now lives in the operational layer where the future restart loop belongs.

## Validation

```
cargo test -p valori-daemon
  → 9 unit (project, workspace, port, policy, events, state) + 2 e2e = 11 passed, 0 failed
```

Live HTTP smoke test, all green:
- project start → `status: "running"` (the `RuntimeState` machine, serialized)
- `GET /v1/config` → runtime `kind: "local"`
- `GET /v1/events` → `project.created`, `project.started`
- no orphaned processes; whole daemon runs on injected trait objects.

## Follow-ups

| Item | Milestone |
|---|---|
| Health-driven restart loop consuming `RestartPolicy` (backoff, crash reason) | D2.2 |
| `ConfigProvider` + `ModelProvider` seams (deferred from the review) | D4 |
| `DockerRuntime`/`DockerLauncher` (bollard) as the second `Launcher` | later |
| `SqliteEventStore` / `SqliteProjectStore` when durability/query needs grow | later |
| Full event bus (managers publish; runtime/logger/desktop subscribe) | later |
