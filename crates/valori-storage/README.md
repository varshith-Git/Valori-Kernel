# valori-storage

Durable storage layer for the Valori platform. Owns everything that touches
disk: WAL, append-only event log, crash recovery, and the object-store backend.

## Modules

| Module | Contents |
|---|---|
| `wal_writer` | `WalWriter` — append-only WAL with 16-byte header (version / dim / CRC) |
| `wal_reader` | `WalReader` — header-validated iterator over `Command`s; legacy recovery path |
| `events` | Event log (v2/v3 formats), journal, committer, replay, proof |
| `events::event_log` | `EventLogWriter` — BLAKE3-chained append-only log; rotation with splice |
| `events::event_journal` | `EventJournal` — committed/buffer distinction; tokio broadcast for live tailing |
| `events::event_commit` | `EventCommitter` — shadow-first commit barrier; batch; auto-rotation |
| `events::event_replay` | `recover_from_event_log`, `read_all_segments`, chain-splice verification |
| `events::event_proof` | `EventProof` — BLAKE3 log hash + canonical state proof |
| `object_store` | `ObjectStoreBackend` — S3/file upload/download/list/prune via opendal |
| `recovery` | `replay_wal`, `recover_from_events`, `validate_snapshot`; `StorageError` |

## Dependency graph position

```
valori-core
  └── valori-kernel
        └── valori-storage   ← this crate
              └── valori-node
```

## Key invariants

- **Shadow-first commit**: `EventCommitter` applies every event to a cloned
  shadow state before writing to the audit log. A rejected event never
  produces a phantom log entry.
- **Chain continuity across rotation**: Rotated segments record the closing
  chain head of the previous segment in their v3 header. Recovery verifies
  every splice point — a missing or tampered archive is detected, not silently
  skipped.
- **WAL is the legacy path**: new code uses `EventCommitter` + the event log.
  `WalWriter`/`WalReader` remain for crash recovery of pre-event-log data.
