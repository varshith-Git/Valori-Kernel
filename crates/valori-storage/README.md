# valori-storage

Durable storage layer for the Valori platform. Owns everything that touches
disk: WAL, append-only event log, crash recovery, and the object-store backend.

## Phase 4.1 additions

`CollectionManifest` gains three optional Phase-4.1 fields (all `#[serde(default)]` — no schema version bump):

| Field | Type | Purpose |
|---|---|---|
| `active_index_generation` | `Option<u32>` | Which artifact generation is currently ACTIVE |
| `active_index_type` | `Option<String>` | Algorithm name (`"hnsw"`, `"ivf"`, `"bq"`) |
| `active_index_base_lsn` | `Lsn` | WAL position the artifact was written at |

Recovery: if `active_index_base_lsn == recovered_lsn`, the artifact is loaded directly (fast path); otherwise the index rebuilds from `KernelState` records (always correct).

## Modules

| Module | Contents |
|---|---|
| `wal_writer` | `WalWriter` — append-only WAL with 16-byte header (version / dim / CRC) |
| `wal_reader` | `WalReader` — header-validated iterator over `Command`s; legacy recovery path |
| `collection_snapshot` | `CollectionSnapshot` (V3) — snapshot with records, graph nodes, and graph edges, with monotonic hole-filling across collections |
| `collection_manifest` | `CollectionManifest` — per-collection schema metadata (`dimension`, `metric`, `snapshot_base_lsn`) |
| `project_manifest` | `ProjectManifest` — project-level metadata and discovery root |
| `provider` | `StorageProvider`, `LocalStorageProvider`, `StorageKey`, `ListPrefix` — logical storage abstractions |
| `events` | Event log (v2/v3 formats), journal, committer, replay, proof |
| `events::event_log` | `EventLogWriter` — BLAKE3-chained append-only log; rotation with splice |
| `events::event_journal` | `EventJournal` — committed/buffer distinction; tokio broadcast for live tailing |
| `events::event_commit` | `EventCommitter` — shadow-first commit barrier; batch; auto-rotation and `StorageProvider` segment publishing |
| `events::event_replay` | `recover_from_event_log`, `read_all_segments`, `stream_events_from_provider`, chain-splice verification |
| `events::event_proof` | `EventProof` — BLAKE3 log hash + canonical state proof |
| `object_store` | `ObjectStoreBackend` — upload/download/list/prune via opendal against `s3://` (AWS, or MinIO/R2/Localstack via an endpoint), `b2://` (Backblaze B2 over its S3-compatible API, endpoint derived from the region), or `file://`; `check_connectivity()` write-then-reads a canary object, used by valori-node at startup to fail fast on a misconfigured store; `manifest.json` (`SnapshotManifest`) is the disaster-recovery entry point — `upload_snapshot_and_update_manifest()` names the current snapshot + archived WAL segments in one object instead of a caller listing/sorting `snapshots/`/`wal/` by hand; snapshots themselves are already versioned (`{epoch}_{hash8}.snap`, never overwritten) — the manifest is what picks the active one out of however many exist |
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

## The `utoipa` feature (Phase API-3.1)

Optional and **off by default** — nothing in the runtime path needs it, and
enabling it adds a dependency the shipped binary does not carry.

```toml
utoipa = ["dep:utoipa"]
```

`valori-node`'s own `utoipa` feature turns it on transitively. It adds
`#[derive(ToSchema)]` to `object_store::{SnapshotEntry, WalEntry, SnapshotManifest}`, which `valori-node` serialises verbatim from
`/v1/storage/*`.

The point is that there is **one** type. The public OpenAPI contract references
the same struct the handler returns, so a field added or renamed here shows up
in the contract automatically instead of drifting away from a hand-copied mirror
in `valori-node/src/api.rs`. `scripts/verify-api-route-contract.py` and the
byte-equality test in `crates/valori-node/tests/openapi_generated.rs` enforce it.

