# Phase: Studio S5 — Session Retention

## Goal

Fix the one P0 finding from `docs/reviews/studio-persistence-consolidation-audit.md`:
`studio.redb`'s `sessions` table grows by one row per app launch, forever,
with no pruning of any kind. Implement bounded, policy-driven retention
end-to-end — storage API, desktop integration, tests, and a real desktop
verification — without touching anything outside session retention.

## Delivered

### Storage API (`crates/valori-studio-storage`)

- **[`src/session.rs`](../../crates/valori-studio-storage/src/session.rs)**
  — `SessionRetentionPolicy` (typed struct: `max_completed_sessions: usize
  = 100`, `completed_retention_days: i64 = 90`, `crashed_retention_days:
  i64 = 180`, with a `Default` impl matching the task's specified policy)
  and `SessionStore::prune(current_session_id, &policy, now) ->
  StudioStorageResult<SessionPruneStats>`. `SessionPruneStats` reports
  `scanned`/`deleted`/`retained`/`protected_active`/`protected_current`/
  `protected_within_retention`.
- **The exact rule implemented** (see the type's own doc comment for the
  authoritative version): an open session or the current session
  (explicitly, by id, regardless of state) is never pruned. A completed
  session is deleted only when it is **both** outside the newest-N floor
  **and** older than the completed-retention window — a row-count
  overflow alone never deletes a still-recent session. A crashed session
  is deleted once older than the crashed-retention window; no count cap
  applies to crashed sessions.
- Deletion order among eligible rows is oldest-`started_at`-first,
  deterministic for a fixed `(policy, now)` pair — `now` is caller-
  supplied, never read from the wall clock inside the crate, matching
  every other store's existing convention in this codebase.
- Only the `sessions` table is read or written — `prune` never touches
  `meta`, `preferences`, `projects`, `project_cache`, `telemetry_queue`,
  `sync_state`, or `update_state`.
- **[`tests/sessions.rs`](../../crates/valori-studio-storage/tests/sessions.rs)**
  — 13 new tests (9 pre-existing session tests unchanged), covering every
  scenario the task listed: empty database, active session, recent/old
  completed, recent/old crashed, count below/above the cap, deterministic
  oldest-first deletion (including a two-fixture determinism check),
  reopen-after-prune, idempotent double-prune, and a realistic mixed
  fixture (current + other-active + recent-completed + old-completed-
  under-cap + recent-crashed + old-crashed) asserting the exact expected
  survivor set.

### Desktop integration (`desktop/src-tauri`)

- **[`src/session_service.rs`](../../desktop/src-tauri/src/session_service.rs)**
  — `SessionService::prune_sessions`, a thin delegate to
  `SessionStore::prune` (matches this file's existing pattern: every store
  method has a same-named service wrapper). One new test
  (`prune_sessions_wrapper_delegates_to_the_store`).
- **[`src/lib.rs`](../../desktop/src-tauri/src/lib.rs)**'s `setup()` —
  reordered to: DB open → installation identity → **crash reconciliation
  → prune (default policy) → start current session**, per the task's
  specified safe point. Reordering `reconcile_crashed_sessions` ahead of
  `start_session` is safe and semantics-preserving: reconciliation only
  needs the current session's *id* (to exclude it), never its row, and the
  row doesn't exist yet either way at that point in `setup()`.
- **Failure handling**: `prune_sessions`'s `Result` is `match`ed, never
  `.unwrap()`/`?`. On `Err`, `tracing::warn!` logs it and startup
  continues unconditionally — no call into `init_studio_storage`/recovery,
  no `panic!`, no `std::process::exit`. On `Ok` with `deleted > 0`,
  `tracing::info!` logs scanned/deleted/retained.
- **[`tests/session_retention_architecture.rs`](../../desktop/src-tauri/tests/session_retention_architecture.rs)**
  (new, 4 tests) — same source-scanning technique as
  `installation_id_architecture.rs`/`credential_security_architecture.rs`
  (this crate's `setup()` closure isn't unit-testable in isolation; it
  needs a running Tauri app). Mechanically verifies: `prune_sessions`'s
  result is never unwrapped/propagated, the failure arm only logs (never
  recovers or exits), `prune_sessions` runs before `start_session`, and
  `reconcile_crashed_sessions` runs before `prune_sessions`.

## Findings

- The existing crash-reconciliation ordering (`start_session` then
  `reconcile_crashed_sessions`) was **not** semantically required — it
  worked before purely because `reconcile_crashed_sessions` takes
  `current_session_id` as a parameter, not by reading the current row.
  This made the task's requested reordering (reconciliation, then prune,
  then start) a safe, compatible adjustment rather than a workaround —
  documented in `lib.rs`'s own comment at the call site rather than left
  implicit.
- No materially better policy than the one specified was found in the
  existing code — there was no prior retention logic of any kind to be
  compatible with (confirmed by the audit and reconfirmed here: `recent()`
  was the only bound-adjacent method, and it truncates a read, not the
  table).
- `chrono`'s `clock` feature isn't enabled for `valori-studio-storage`
  (only `std`) — `now` is caller-supplied throughout, by design (see
  `SessionStore::prune`'s doc comment), so this wasn't a blocker; it only
  affected how the real-desktop smoke test's scratch seeding script
  computed the current timestamp (`SystemTime::now()` instead of
  `chrono::Utc::now()`).

## Validation

```text
cargo test -p valori-studio-storage                                 all green — 9 pre-existing +
                                                                       13 new session-retention tests
                                                                       (full crate: 118 tests total,
                                                                       was 105 before S5)
cargo test -p valori-domain                                          all green, unchanged (not touched)
cargo test --workspace                                                all green, 0 failures
cargo check --workspace                                                clean
cargo clippy -p valori-studio-storage --all-targets -- -D warnings    clean
cargo test -p valori-node --test dependency_direction                 6 passed, 0 failed
cargo test -p valori-node --test architecture                         1 passed, 0 failed
cargo fmt --check                                                     clean (only the same pre-existing,
                                                                       unrelated dump_studio_db.rs issue
                                                                       from before S3/S5, not touched)
npx tsc --noEmit                                                      clean (no TS changed this phase)
npm run build                                                          succeeds

Desktop crate (separate build, outside the root workspace):
cargo build --lib                                                     clean
cargo test --lib                                                      65 passed, 0 failed (52 lib +
                                                                       5 credential + 4 installation-id +
                                                                       4 session-retention architecture)
```

### Real desktop smoke test

Against a disposable `$VALORI_HOME=/tmp/valori-s5-test` (deleted after),
running the actual compiled `desktop/src-tauri` binary, all 9 required
steps:

| # | Step | Result |
|---|---|---|
| 1 | Fresh launch creates a session | 1 session row, `ended_at: None` |
| 2 | Normal shutdown (SIGTERM → `shutdown_and_exit`) ends the session | `ended_at` set, `crashed: false` |
| 3 | Forced termination (SIGKILL) | process killed, row left open |
| 4 | Restart | app launches cleanly |
| 5 | Retention runs | real log line: `session retention: pruned old session history scanned=5 deleted=1 retained=4` |
| 6 | Protected sessions remain | seeded recent-completed (10d) and recent-crashed (20d) rows both survived |
| 7 | Old eligible sessions removed | seeded old-crashed row (200d, past the 180-day window) was gone after restart |
| 8 | Project files byte-for-byte unchanged | SHA-256 of `project.json`/`snapshot.val`/`events.log` identical before and after the entire sequence |
| 9 | `studio.redb` reopens successfully | reopened via a scratch inspector after the full sequence — 5 sessions listed correctly, including the just-reconciled force-killed one now showing `crashed: true` |

Session seeding and inspection used two scratch example files (public
`valori-studio-storage` API only — the desktop crate's modules are
private, unreachable from an external binary) — both deleted after the
smoke test, not part of the deliverable.

## Follow-ups

None required by this phase's scope. Explicitly not touched, per the
task's boundary: `metadata.redb`, credentials, sync, Cloud, marketplace,
`S6` reranker-config consolidation, or any other persistence surface named
in the S4 audit.
