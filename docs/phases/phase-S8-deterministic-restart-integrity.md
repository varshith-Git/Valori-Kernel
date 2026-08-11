# Phase S8: Deterministic Restart Integrity

## Goal

Root-cause and fix the state-hash-across-restart divergence flagged by
the Local Cloud E2E phase: vector data and search results were
byte-identical before/after a real worker restart, but the BLAKE3 state
hash (`/v1/proof/state`) differed. Acceptance criterion: logical data
identical **and** state hash identical, or the invariant precisely
redefined — not a weakened test.

## Delivered

- Root cause isolated via a standalone Rust binary comparing
  live-apply vs event-log-replay hashes directly (`Engine::new` →
  `create_collection` → `insert_batch_ns` → hash, vs.
  `recover_from_event_log` → hash), bypassing Docker/HTTP entirely —
  reproduced the exact mismatch outside any container first, then
  confirmed the fix closes it both in isolation and against the real
  Docker container.
- `crates/valori-engine/src/engine.rs`: `create_collection()` and
  `drop_collection()` now route their `AutoCreateNamespace`/
  `DropNamespace` events through `commit_and_apply_ns()` instead of
  calling `state.apply_event_ns()` directly.
- `crates/valori-engine/src/engine.rs`: new `Engine::flush_pending_events()`.
- `crates/valori-node/src/main.rs`: `shutdown_signal()` now takes a
  write lock and calls `flush_pending_events()` before saving the final
  snapshot, instead of relying solely on `Engine::drop()` (not
  guaranteed to fire — see Findings).
- Regression test:
  `crates/valori-node/tests/persistence_tests.rs::test_state_hash_survives_restart_after_collection_create`.

## Findings

1. **Primary bug**: `create_collection()`/`drop_collection()` mutated
   `KernelState` — specifically bumping `state.version`, which IS part
   of the BLAKE3 hash — via a direct `state.apply_event_ns()` call that
   bypassed the durability layer entirely. `AutoCreateNamespace`/
   `DropNamespace` are otherwise no-ops on records/nodes/edges, so every
   *queryable* thing (search results, record IDs, vector content)
   stayed correct even while the hash silently diverged — exactly why
   this passed unnoticed until real E2E restart-tested it.
2. **Secondary bug, same symptom, different trigger**: even after
   routing through the correct "log then apply" helper, a *single*-event
   commit (`commit_event_ns`) only buffers in memory
   (`DEFAULT_WRITE_BUFFER_SIZE = 64`) rather than flushing immediately —
   unlike a batch commit (`commit_batch_ns`), which always flushes
   unconditionally. The only existing flush call site was
   `Engine::drop()`, which is not reliably invoked on graceful shutdown:
   `SharedEngine` is `Arc<RwLock<Engine>>`, and background tasks
   (auto-snapshot, process-metrics) can hold clones past the point
   `axum::serve(...).with_graceful_shutdown()` returns, so the Engine's
   strong count may never reach zero before the process exits.
3. Both bugs needed fixing together — fixing only #1 still lost the
   `AutoCreateNamespace` event on restart in testing, because it stayed
   buffered and unflushed.
4. This class of bug (a state mutation with no corresponding durable
   audit write) is exactly what CLAUDE.md's own invariant #1 ("DEDUP
   CHECK → KERNEL APPLY → AUDIT WRITE") exists to prevent — worth an
   audit of any other `state.apply_event_ns(...)`/`state.apply_event(...)`
   call site that doesn't go through `commit_and_apply_ns`/
   `commit_committed_event` first (see Follow-ups).

## Validation

- Standalone repro (bypassing Docker): live hash `c3a39f82...` vs.
  replay hash `7a15889f...` before the fix (mismatch, reproduced
  reliably); both `c3a39f82...` after the fix (match) — same exact
  bytes as the real container's own "before restart" value, i.e. the
  fix makes replay reproduce what live state *actually* was, not the
  other way around.
- `cargo test --workspace`: 1186 passed, 0 failed (including the new
  regression test).
- `cargo clippy --workspace`: 0 warnings, 0 errors.
- Real end-to-end: fresh isolated `valori-node` container, create
  namespace → insert 3 deterministic vectors → capture hash → real
  `docker stop`/`docker start` → capture hash again. Before the fix:
  `c3a39f82...` → `7a15889f...` (mismatch, matches the original E2E
  finding exactly). After the fix: `c3a39f82...` → `c3a39f82...`
  (match) — data and search results unchanged throughout, as before.
- Full Local Cloud E2E suite re-run from `docker compose down -v &&
  build --no-cache && up -d`: 46 passed, 2 skipped (unrelated — see
  `phase-local-cloud-e2e-verification.md`).

## Follow-ups

- Audit every other direct `state.apply_event_ns(...)`/
  `state.apply_event(...)` call site across the engine for the same
  bypass pattern — this phase fixed the two found via the restart test,
  but didn't do an exhaustive sweep of every call site in the codebase.
- Consider whether `commit_event_ns` should flush unconditionally too
  (matching `commit_batch_ns`), rather than relying on a shutdown-time
  flush — the current fix is correct for graceful shutdown, but an
  ungraceful crash (SIGKILL, OOM) still loses any buffered single-event
  commits, same as it always could for batches too small to trigger
  the write-buffer threshold in a crash-only scenario. This is a
  throughput/durability tradeoff, not a bug, but worth a deliberate
  decision rather than the implicit one it currently is.
- S9 — Resource Capacity Matrix (full dim/index/vector-count benchmark
  grid) is next, per the user's own sequencing.
