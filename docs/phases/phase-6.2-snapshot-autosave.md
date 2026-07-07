# Phase 6.2 — Snapshot autosave & verified project reopen (standalone + cluster)

## Goal

Guarantee that a UI project's data survives any close/reopen cycle — including
ungraceful kills — by adding periodic snapshot autosave, and verify end-to-end
that cluster projects can be opened entirely from the UI (no manual Launch page
steps).

## Delivered

| File | Change |
|---|---|
| `crates/valori-node/src/config.rs` | Removed the deprecation warning on `VALORI_SNAPSHOT_INTERVAL` — the "replacement" knobs (`VALORI_SNAPSHOT_EVERY_EVENTS/BYTES`) were parsed but never implemented anywhere, so the interval knob is the real cadence control |
| `crates/valori-node/src/main.rs` | Cluster mode now has a graceful-shutdown handler (`cluster_shutdown_signal`) — SIGTERM/Ctrl-C drains axum and lets redb close cleanly instead of dying mid-write |
| `crates/valori-node/src/cluster_server.rs` | **Bug fix:** cluster `/search` returned raw Q16.16 `i64` scores (e.g. `42954916`) while standalone returned floats (`0.0100…`). `SearchHit.score` is now `f32` divided by SCALE² across all three ranking paths (plain, reranked, decay) |
| `ui/src/lib/server/process-manager.ts` | Every spawn with a snapshot path now sets `VALORI_SNAPSHOT_INTERVAL=60` — periodic autosave even without a graceful close |
| `ui/src/app/api/projects/[name]/close/route.ts` | Captures the final record count from `/health` before stopping and persists it to the manifest, so at-rest project cards show accurate counts |
| `CLAUDE.md` | Added `VALORI_SNAPSHOT_INTERVAL` to the env var table |

## Findings

- `VALORI_SNAPSHOT_EVERY_EVENTS` / `VALORI_SNAPSHOT_EVERY_BYTES` are config-parsed
  but dead — nothing reads them. The deprecation warning pointed users at vaporware.
  Left the fields in place (removal is a separate cleanup); only the warning is gone.
- Cluster `/v1/snapshot/save` intentionally does **not** persist to the request path —
  cluster durability rides on the persisted Raft log (redb) + SM meta
  (`sm_last_applied`, persisted snapshot in `sm_meta` table). The UI's
  snapshot-then-stop POST is harmless there.
- Cluster `/health` has a different shape from standalone (no `records` field) —
  the close route's record-count capture is best-effort and skips cluster gracefully.
- The `pm` singleton survives Next.js hot reloads with stale method code — env-var
  changes to `startNode` only apply after a dev-server restart (known gotcha,
  re-confirmed).
- The user-reported "reopen shows no data" was **not** a persistence bug: the
  inspected project's `events.log` was 48 bytes (header only), meaning ingest had
  never committed anything — consistent with the earlier dimension-mismatch bug
  (server 1536 vs nomic 768) blocking all inserts before it was fixed.

## Validation

End-to-end via the UI API routes (dev server on :3001, release binary):

- **Standalone** (`e2e-standalone`, dim 4): create → open → create collection →
  insert 3 records → close (snapshot written, `uchg` applied, node stopped) →
  reopen → **3 records, collection list, and search results all restored** ✓
- **Cluster** (`e2e-cluster`, replication 3, ports 4010-4012): create → open from
  UI (3/3 nodes healthy, leader elected term 1) → 3 writes through Raft →
  follower search returns all hits → close (all 3 stopped, per-node WALs locked) →
  reopen → **data restored from redb Raft log on all nodes** ✓ (survived 2 cycles)
- **Autosave**: node with `VALORI_SNAPSHOT_INTERVAL=5` wrote 4 snapshots in ~9 s
  without any close ✓
- **Score fix**: after rebuild, cluster search returns `0.010001220…` — identical
  scale to standalone ✓
- `cargo test -p valori-kernel -p valori-node`: 289 passed, 0 failed (all suites, exit 0)

## Follow-ups

- Remove the dead `snapshot_every_events` / `snapshot_every_bytes` config fields, or
  implement them (event-count-based cadence is strictly better than wall-clock for
  bursty writes). Unowned.
- Cluster `/health` should expose a `records` field for shape parity with standalone
  (UI stat cards currently show nothing for cluster projects). Unowned.
- `/v1/proof/event-log` + `/v1/timeline` still read shard 0 only (pre-existing,
  tracked since S13).
