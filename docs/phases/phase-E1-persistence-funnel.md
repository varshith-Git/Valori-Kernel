# Phase E1 — Single persistence funnel in Engine

## Goal

Finish what Phase 1.9 promised: collapse Engine's dual persistence branch
(`Option<EventCommitter>` + `Option<WalWriter>`, with
`if let Some(committer) … else …` duplicated across every write method)
into one write path.

## Delivered

| File | What |
|---|---|
| `crates/valori-node/src/commit/persistence.rs` (new) | `Persistence` enum — `EventLog(EventCommitter)` / `Wal(WalWriter)` / `Ephemeral`. `log_event_ns` / `log_batch_ns` are the only durable-logging entry points; `command_for()` translates `KernelEvent` → legacy WAL `Command` (returns `None` for `SetMeta`, which the WAL format cannot represent — matching pre-E1 behavior). |
| `crates/valori-node/src/engine.rs` | `Engine` now holds `persistence: Persistence` instead of the two `Option`s. New private funnel `commit_and_apply_ns(event, ns)` = durably log → apply exactly once (state + index + derived maps). 10 write methods rewritten to one-liners through the funnel: `set_meta_audited`, `insert_record_from_f32_ns`, `insert_encrypted_ns`, `shred_key`, `insert_batch_ns`, `soft_delete_record`, `delete_record`, `delete_node`, `delete_edge`, `create_node_for_record`, `create_edge`. Public accessors `event_committer()` / `event_committer_mut()` for observability call sites. |
| `crates/valori-node/src/replication.rs` | Field accesses → accessors; bootstrap swaps `engine.persistence` wholesale. |
| `crates/valori-node/src/server.rs` | 8 field accesses → accessors. |
| `crates/valori-ffi/src/lib.rs` | ~12 field accesses → accessors. |
| `crates/valori-node/tests/` (5 files) | `engine.event_committer = Some(x)` → `engine.persistence = Persistence::EventLog(x)`. |

## Design decision — enum, not `Box<dyn Committer>`

The original Phase 1.9 plan was `committer: Box<dyn Committer>`. Rejected:
~40 call sites (server.rs proof/timeline/receipts/rotation, replication.rs
streaming + wholesale replacement during bootstrap, valori-ffi, tests) need
the *concrete* `EventCommitter` (journal heights, `subscribe()`,
`rotate_log()`, `into_parts()`). A trait object would force a downcast at
every one. The enum keeps static dispatch and honest accessors. The
`Committer` trait remains the cluster seam (`RaftCommitter`,
`EventLogAuditSink` in cluster.rs); `StandaloneCommitter` stays as the
trait-shaped adapter for future symmetric work.

## Findings

1. **Old event-log batch path never auto-tiered**: `insert_batch_ns`'s
   committer branch skipped `auto_tier_check()` (the WAL branch checked per
   insert). The unified path checks once per batch — a small behavior fix.
2. `apply_event_ns` is a pure translation layer to `apply(&Command)`, so
   replacing the WAL branch's manual `state.apply(&cmd)` + index bookkeeping
   with `apply_committed_event_ns` is behavior-identical.
3. `create_edge`'s "use committer live_state for the edge ID" comment was
   stale — engine.state and live_state receive every event and are always
   count-identical; the unified path reads `self.state.edge_count()`.
4. Engine still maintains TWO KernelStates on the event-log path (its own +
   the committer's internal live_state, both applied per event). Pre-existing;
   not changed in E1.

## Validation

- `cargo test -p valori-node` — **228 passed, 0 failed**.
- `cargo test -p valori-kernel` — **66 passed, 0 failed**.
- `cargo check -p valoricore-ffi` — clean.

## Follow-ups

- **Phase E2**: registry reconciliation (see phase-E0 doc).
- Collapse the double-KernelState on the event-log path (Engine.state vs
  EventCommitter.live_state) — needs its own design pass.
- Map `CommitError::Apply(KernelError)` to `EngineError::Kernel` instead of
  `InvalidInput` so capacity errors keep their HTTP 507 semantics end-to-end
  (currently preserved by string mapping, same as pre-E1).
