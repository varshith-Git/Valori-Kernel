# Phase 1.8 — Storage Policy: Snapshot Cadence, zstd, Disk-Full Behavior

**Status:** planned  
**Roadmap:** [MULTINODE_ROADMAP.md](../MULTINODE_ROADMAP.md) § 1.8  
**Why now:** Three of the four storage decisions in this phase are
format-level (segment file naming convention, Checkpoint entry in the chain,
config knob names) or operational (disk-full contract). Locking them in before
production traffic means no migration cost and no operator surprise when Phase 2
adds Raft-coordinated snapshotting on top of the same storage layer.

---

## Goal

1. **Snapshot cadence knob** (`VALORI_SNAPSHOT_EVERY`) — recovery time bounded
   by cadence, not by log history. Recovery = latest snapshot + tail replay,
   not genesis-to-now replay.
2. **zstd compression of sealed segments** — sealed (rotated, no-longer-active)
   segment files are compressed; the active tail file is never compressed.
   Transparent to readers (Phase 1.7 covers the verifier side).
3. **Defined disk-full behavior** — degraded read-only mode with a clear error;
   heartbeats keep flowing; no silent data loss; tested.
4. **Fallback recovery chain** — latest snapshot → genesis replay (audit path).
   Documents the semantics precisely for Phase 2's Raft snapshot adapter.

---

## Current State vs. Target

| Property | Current | Phase 1.8 target |
|---|---|---|
| Snapshot trigger | `VALORI_SNAPSHOT_INTERVAL` (seconds only) | `VALORI_SNAPSHOT_EVERY` (bytes **and/or** events) |
| Recovery baseline | Always replay from genesis (or last snapshot if `VALORI_SNAPSHOT_PATH` set) | Snapshot + tail only; genesis replay is an explicit audit-only mode |
| Sealed segment files | Plain bincode, no compression | zstd-compressed (transparent to readers) |
| Disk-full behavior | Undefined — may panic or corrupt | Degraded read-only; HTTP 507 on writes; clear log message |
| Checkpoint entry | Exists in log (`LogEntry::Checkpoint`) | Written at every snapshot point; `snapshot_hash` field verified on restore |
| Segment naming | `events.log` (single file, rotates) | `events.log` (active) + `events.NNNN.log.zst` (sealed) |

---

## D1 — `VALORI_SNAPSHOT_EVERY` Knob

### Config additions (`NodeConfig`)

```rust
// crates/valori-node/src/config.rs — new fields (Phase 1.8)

/// Trigger a snapshot after this many events have been appended since the
/// last snapshot. Takes precedence over `snapshot_every_bytes` if both are
/// set (whichever threshold is crossed first wins).
///
/// Default: None (no event-count cadence).
/// Env:     VALORI_SNAPSHOT_EVERY_EVENTS=50000
pub snapshot_every_events: Option<u64>,

/// Trigger a snapshot after this many bytes have been appended to the active
/// segment since the last snapshot.
///
/// Default: 64 MiB (67_108_864 bytes).
/// Env:     VALORI_SNAPSHOT_EVERY_BYTES=67108864
///
/// Setting to 0 disables byte-triggered snapshots.
pub snapshot_every_bytes: Option<u64>,
```

`VALORI_SNAPSHOT_INTERVAL` (seconds-based) is **deprecated but still
accepted**; a startup warning is printed if it is set without the new knobs.
It will be removed in Phase 3. The new knobs are preferred because
seconds-based cadence gives no bound on replay length when write rate varies.

### Cadence enforcement in the write path

In `engine.rs` / `EventJournal`, after each successful event append:

```rust
self.events_since_snapshot += 1;
self.bytes_since_snapshot  += entry_bytes.len() as u64;

let should_snap =
    self.config.snapshot_every_events
        .map(|n| self.events_since_snapshot >= n)
        .unwrap_or(false)
    || self.config.snapshot_every_bytes
        .map(|b| self.bytes_since_snapshot >= b)
        .unwrap_or(false);

if should_snap {
    self.take_snapshot()?;  // triggers Checkpoint entry + rotate_if_needed
    self.events_since_snapshot = 0;
    self.bytes_since_snapshot  = 0;
}
```

The counters reset after every successful snapshot. If `take_snapshot()` fails
(e.g., disk full — see D4), the counters are NOT reset so the next write
attempt triggers another snapshot attempt. This ensures the cadence is
maintained under intermittent failures.

### Recovery time bound

With `VALORI_SNAPSHOT_EVERY_EVENTS=N`:

```
worst-case recovery time = snapshot_restore_time + N × per_event_replay_time
```

At 6 µs/event (per `wal-replay-guarantees.md`), `N = 50_000` gives a
worst-case tail-replay time of **300 ms** — a concrete, predictable number
operators can put in their SLA.

---

## D2 — Checkpoint Entry Written at Every Snapshot

The `LogEntry::Checkpoint` variant already exists in the wire format:

```rust
LogEntry::Checkpoint {
    event_count: u64,
    snapshot_hash: [u8; 32],
    timestamp: u64,
}
```

Currently checkpoints are written inconsistently. Phase 1.8 **mandates** that
every `take_snapshot()` call writes a `Checkpoint` entry to the log immediately
*after* the snapshot file is fsynced. This gives:

```
[... events ...]
[Checkpoint { event_count=N, snapshot_hash=H, timestamp=T }]
[... more events ...]
[Checkpoint { event_count=M, snapshot_hash=H2, timestamp=T2 }]
```

The snapshot file at path `snapshots/state-<event_count>.snap` MUST be
verifiable: `BLAKE3(snapshot_bytes) == snapshot_hash` in the Checkpoint entry.

### Restore path (Phase 1.8)

```
1. Scan snapshots/ for the newest .snap file (by event_count in filename).
2. Load it; verify BLAKE3(bytes) matches the corresponding Checkpoint entry
   in the log (if the log is present). On mismatch → fall through to genesis
   replay (log is the canonical truth).
3. Find the first log entry AFTER the Checkpoint at event_count N.
4. Replay only entries after offset N (the tail).
5. Done. Recovery time = snapshot_restore_time + (total_events - N) × per_event.
```

### Genesis replay mode (audit-only)

```bash
valori-node --replay-from-genesis   # new startup flag
```

Ignores all snapshots, replays the full event log from genesis. Slower but
produces the same final state hash — useful for audit and disaster recovery
when snapshots are unavailable. This flag is explicitly NOT the default; it is
an operator escape hatch, not a normal startup mode.

---

## D3 — Segment File Convention and zstd Sealing

### File naming convention

```
$DATA_DIR/audit/
  events.log              ← active tail segment (never compressed while active)
  events.0000.log.zst     ← sealed at rotation 0, zstd-compressed
  events.0001.log.zst     ← sealed at rotation 1, zstd-compressed
  ...

$DATA_DIR/snapshots/
  state-000050000.snap    ← snapshot after event 50,000
  state-000100000.snap    ← snapshot after event 100,000
  state-000100000.snap.tmp ← in-progress (never leaves .tmp if fsync fails)
```

The `events.NNNN` integer is the `segment_seq` from the v3 header, zero-padded
to 4 digits (supports 9999 archived segments = ~640 GiB at default cadence).

### Rotation + sealing sequence

```
1. Rename  events.log  →  events.NNNN.log   (atomic on POSIX)
2. Open    events.log  (new active file, write v3 header with segment_seq=NNNN+1
                        and prev_segment_chain_head = final head of events.NNNN.log)
3. fsync   new events.log
4. Compress+rename events.NNNN.log → events.NNNN.log.zst  (background task)
5. Delete  events.NNNN.log  (only after .zst fsync succeeds)
```

Step 4–5 run in a background thread so the write path is not blocked by
compression. The active tail is always the plain `events.log`; compressed
files are always sealed (read-only). If compression fails, the plain `.log`
file is retained — the verifier reads both formats (Phase 1.7).

### zstd compression parameters

```rust
// crates/valori-node/src/wal_writer.rs or new sealer module

const ZSTD_COMPRESSION_LEVEL: i32 = 3;
// Level 3: ~2–3× faster than level 6 with ≤5% larger output.
// At level 3, zstd typically achieves 3–5× compression on bincode event logs
// (highly repetitive schema bytes). Level chosen for seal throughput, not
// archive size (cold segments are rarely read).
```

Dependency:

```toml
# crates/valori-node/Cargo.toml
zstd = { version = "0.13", features = ["zstdmt"] }  # multi-threaded for sealing large segments
```

---

## D4 — Disk-Full Behavior

### Current problem

No defined behavior. If `write_all` returns `ENOSPC`, the error propagates up
through the event journal and may leave the log file in an inconsistent state.
There is no mechanism to switch the server to read-only mode or to alert
operators.

### Defined behavior (Phase 1.8)

```
disk-full event (ENOSPC on any write):
  1. Engine transitions to DegradedReadOnly state.
  2. All write endpoints return HTTP 507 Insufficient Storage immediately,
     without touching the log or state.
  3. Read endpoints (search, get, list) continue to work — reads are served
     from the in-memory KernelState.
  4. Heartbeat / health endpoint returns HTTP 200 with body:
       { "status": "degraded", "reason": "disk_full",
         "since": "<ISO timestamp>", "last_event": N }
  5. A single ERROR log line is emitted per new ENOSPC (rate-limited to 1/s
     to avoid log spam).
  6. Metrics: valori_disk_full_mode gauge set to 1.
  7. No panic, no process exit, no data loss for in-memory state.
```

### Recovery from disk-full

```bash
# Operator clears disk space, then:
curl -X POST http://localhost:3000/v1/admin/resume
# Transitions engine back to normal mode; retries the pending snapshot if one
# was in progress when ENOSPC occurred.
```

The `/v1/admin/resume` endpoint is the only way to exit degraded mode — there
is no automatic recovery, because automatic recovery without operator awareness
risks a rapidly-cycling ENOSPC/recover loop consuming CPU.

### Engine state machine

```rust
// crates/valori-node/src/engine.rs — new enum (Phase 1.8)

pub enum EngineMode {
    /// Normal operation: reads and writes both accepted.
    Normal,
    /// ENOSPC was encountered. Writes return HTTP 507 immediately.
    /// Reads continue to work from in-memory state.
    DegradedReadOnly {
        since: std::time::Instant,
        last_event_count: u64,
    },
}
```

All write handlers check `self.mode` before acquiring the engine lock. The
check is a cheap enum discriminant test with no allocation.

### Pre-write free-space check

As a proactive measure, `take_snapshot()` checks available disk space before
writing. If available bytes < `2 × current_state_size` (a conservative
estimate of the compressed snapshot size), it skips the snapshot and logs a
warning rather than triggering a guaranteed ENOSPC during the write. This
gives operators a warning window before the server goes into degraded mode.

```rust
const SNAPSHOT_FREE_SPACE_MULTIPLIER: u64 = 2;
// Heuristic: compressed snapshot ≈ 50% of raw state size; 2× gives a
// ~2× safety margin. Operators should target >20% free disk at all times.
```

---

## D5 — Snapshot Retention Policy

With a cadence-based snapshot, the `snapshots/` directory grows. Phase 1.8
defines a simple retention policy:

```
VALORI_SNAPSHOT_KEEP=3    # default: keep the 3 newest snapshots
```

On each new snapshot, after the new file is fsynced and the Checkpoint entry
is written:

1. List all `.snap` files in `snapshots/`, sorted by event_count descending.
2. Delete all but the newest `VALORI_SNAPSHOT_KEEP` files.

The corresponding sealed segment files before the oldest retained snapshot's
`event_count` are candidates for archival (S3 upload — Phase 3) or deletion.
Phase 1.8 does not delete segment files automatically; that is Phase 3 scope.

---

## Schema Reservations (Phase 1.8)

### `NodeConfig` additions (Phase 1.8)

```rust
// Reserved field names — implemented in Phase 1.8, not Phase 1.5/1.6:
//   snapshot_every_events: Option<u64>
//   snapshot_every_bytes:  Option<u64>
//   snapshot_keep:         Option<u32>     // VALORI_SNAPSHOT_KEEP
//   zstd_compression_level: Option<i32>   // VALORI_ZSTD_LEVEL
```

All new env-var names are reserved now to avoid conflicts with other knobs:

| Env var | Type | Default | Description |
|---|---|---|---|
| `VALORI_SNAPSHOT_EVERY_EVENTS` | `u64` | — | Snapshot every N events |
| `VALORI_SNAPSHOT_EVERY_BYTES` | `u64` | `67108864` | Snapshot every N bytes of log |
| `VALORI_SNAPSHOT_KEEP` | `u32` | `3` | Retain N most recent snapshots |
| `VALORI_ZSTD_LEVEL` | `i32` | `3` | zstd compression level for sealed segments |
| `VALORI_GENESIS_REPLAY` | `bool` | `false` | Skip snapshots, replay from genesis |

`VALORI_SNAPSHOT_INTERVAL` (existing, seconds-based) is deprecated with a
startup warning.

### Segment file naming is a format contract

The `events.NNNN.log.zst` naming is a contract between the node writer (Phase
1.8) and the verifier reader (Phase 1.7). Changing the naming requires a major
version bump. The convention is documented here and in `docs/SNAPSHOT_FORMAT.md`
(to be updated in Phase 1.8).

---

## Tested Scenarios

| Scenario | Test location | Method |
|---|---|---|
| Snapshot taken at `VALORI_SNAPSHOT_EVERY_EVENTS=100` | `tests/storage_policy.rs` | Insert 250 events, assert 2 snapshots taken |
| Recovery bounded by cadence (not full history) | `tests/storage_policy.rs` | 10K events, snapshot at 5K, kill+restart, assert replay of 5K (not 10K) |
| Checkpoint hash in log matches snapshot file | `tests/storage_policy.rs` | Read Checkpoint entry, BLAKE3 snapshot file, assert equal |
| zstd sealed segment round-trips | `tests/storage_policy.rs` | Rotate + compress, reload in new process, verify state hash |
| Disk-full → HTTP 507 on writes | `tests/storage_policy.rs` (mock FS) | Inject ENOSPC via test hook, assert 507 on insert, 200 on search |
| Disk-full → health endpoint shows `degraded` | `tests/storage_policy.rs` | Same mock FS setup |
| `--log-dir` multi-segment verify (verifier) | `crates/valori-verify/tests/hardening.rs` | 3 zstd segments, verify full chain |

---

## Acceptance Criteria

| Criterion | Evidence |
|---|---|
| Recovery time is bounded by cadence | Test: 10K events, snapshot at 5K, restart replays ≤ 5K events |
| Checkpoint entry hash matches snapshot file | Test: BLAKE3 comparison |
| Sealed segment is readable by verifier after zstd | Test: round-trip in hardening.rs |
| ENOSPC → HTTP 507 on writes, 200 on reads | Test: mock ENOSPC injection |
| `/v1/admin/resume` exits degraded mode | Test: mock + HTTP assertion |
| `VALORI_SNAPSHOT_KEEP=2` leaves 2 snapshots | Test: 5 snapshot cycles, assert 2 remain |
| `VALORI_SNAPSHOT_EVERY_EVENTS` and `_BYTES` accepted at startup | Config unit test |
| Full test suite: 0 regressions | `cargo test --workspace` |

---

## Findings

Design-only phase — no runtime findings. One forward concern noted:

**Compression during recovery:** If the server dies while sealing a segment
(between step 1 `rename` and step 5 `delete` in the rotation sequence), the
data directory may contain both `events.NNNN.log` (plain) and
`events.NNNN.log.zst` (partial). On restart, the recovery path must prefer
the plain `.log` if both exist (it is guaranteed complete; the `.zst` may be
a partial write). Document this in `recovery.rs` and test it explicitly.

**Snapshot file naming collision:** Two snapshot processes (possible in a
future multi-write scenario) could produce `state-000050000.snap` from
different histories. Phase 1.8 relies on the fact that only one process ever
writes to the data directory in standalone mode. Phase 2 (Raft) coordinates
snapshots through the state machine — this issue is resolved by the Raft log
compaction protocol.

**`/v1/admin/resume` authentication:** In Phase 1.8 this endpoint is
authenticated by `VALORI_AUTH_TOKEN` (same as all other admin endpoints). In
Phase 3 it must require the `ADMIN` role (1.6 RBAC). Add a note to the Phase 3
RBAC implementation task.

## Follow-ups

- Phase 1.7: verifier `--log-dir` consumes the `events.NNNN.log.zst` naming
  convention defined here (cross-phase dependency, ordered correctly in the
  roadmap).
- Phase 2: Raft snapshot adapter replaces `take_snapshot()` internals; the
  `Checkpoint` entry format and snapshot file format are unchanged.
- Phase 3: S3/Blob archival of sealed segments older than `VALORI_SEGMENT_ARCHIVE_AFTER`
  days; `valori-verify` against bucket contents.
- Phase 3: `VALORI_SNAPSHOT_KEEP` policy extended to consider segment archival
  status (don't delete segments that haven't been archived yet).
- Phase 3: `/v1/admin/resume` requires `ADMIN` role (RBAC from 1.6).
