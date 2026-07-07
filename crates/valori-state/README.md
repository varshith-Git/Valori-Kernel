# valori-state

State lifecycle orchestration for the Valori platform. Owns the transition of
`KernelState` between durable storage and in-memory operation.

**Divide:** `valori-storage` = raw bytes on disk; `valori-state` = state lifecycle.

## Modules

| Module | Contents |
|---|---|
| `bootstrap` | `recover_from_events`, `replay_wal`, `load_snapshot`, `validate_snapshot`, `has_event_log`, `has_wal`; `BootstrapMode` enum |
| `manifest` | `StateManifest` — snapshot path, event log segment list, last applied height, state hash |
| `lifecycle` | `StateLifecycle` enum — `Recovering`, `Ready`, `Snapshotting` |
| `shutdown` | `shutdown_snapshot(state, path)` — synchronous snapshot-on-close for graceful shutdown |
| `error` | `StateError` (Kernel / InvalidInput / Io); `StateResult<T>` |

## Dependency graph position

```
valori-core
  └── valori-kernel
        ├── valori-storage
        │     └── valori-state   ← this crate
        │           └── valori-node
        └── (directly) valori-node
```

## Recovery priority order

1. **Event log** — canonical truth. If `events.log` exists with committed events,
   replay from scratch via `recover_from_events()`.
2. **Snapshot** — fast-path cache. Loaded via `load_snapshot()` when no event log.
3. **WAL** — legacy fallback. Replayed via `replay_wal()` on top of loaded state.
4. **Fresh start** — no durable state found; start with empty `KernelState`.

## Key invariants

- `StateLifecycle::Recovering` → node does not accept HTTP writes.
- `shutdown_snapshot` must complete before the process exits to avoid a full
  WAL replay on the next startup.
- `StateManifest` is the on-disk record of which files make up current durable
  state. It is not required for correctness (the event log is canonical), but it
  enables faster bootstrap by skipping already-replayed segments.
