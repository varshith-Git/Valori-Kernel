# Phase E3 — Hide pub state

## Goal

Change `Engine.state: KernelState` from `pub` to `pub(crate)` and give external
crates (valori-ffi) controlled read access through explicit accessor methods.

## Delivered

| File | What |
|---|---|
| `crates/valori-node/src/engine.rs` | `pub state` → `pub(crate) state`. Added 10 public accessor methods: `record_count`, `node_count`, `edge_count`, `kernel_dim` (returns `Option<usize>`), `get_node`, `outgoing_edges`, `get_record`, `get_edge`, `clone_kernel_state`, `kernel_state` (read-only `&KernelState` for tag-filtered FFI search). |
| `crates/valori-node/src/server.rs` | 3 `engine.state.*` accesses → accessor methods. |
| `crates/valori-node/src/replication.rs` | 1 `engine.state.record_count()` → `engine.record_count()`. |
| `crates/valori-node/src/ingest.rs` | 2 `engine.state.*` accesses → accessor methods. |
| `crates/valori-ffi/src/lib.rs` | 9 remaining `engine.state.*` accesses eliminated. 7 stale `state_ref` dual-branches (checking `event_committer().live_state()` vs `engine.state`) replaced with `engine.kernel_state()` — these were pre-E1 patterns; after E1 engine.state is always current. FFI `create_node` pre-E1 dual-branch (committer path manually called `committer.commit_event` + read `live_state` for node ID) collapsed to `engine.create_node_for_record`. Stale "sync live_state → engine.state before snapshot" removed. `soft_delete` dual-branch collapsed to `engine.soft_delete_record`. |

## Findings

1. **7 stale dual-branch patterns in valori-ffi were pre-E1 artifacts.** They checked `event_committer().live_state()` for the authoritative state, because before E1 `engine.state` was never mutated in the event-log path. After E1's `commit_and_apply_ns`, both `engine.state` and `live_state` receive every event — the dual-branch was obsolete but invisible until this E3 forced the issue.
2. **FFI `create_node` bypassed Engine.** The committer branch called `committer.commit_event` + `committer.live_state().next_node_id()` directly, bypassing `commit_and_apply_ns`. After E1 this meant nodes went to `live_state` but not `engine.state`. Fixed by routing through `create_node_for_record`.
3. **`kernel_state(&self) -> &KernelState` is intentional controlled exposure.** Tag-filtered search (`search_l2` with `filter_tag`) has no Engine-level wrapper; adding one would be premature. The read-only reference is the pragmatic choice until a proper `search_filtered_ns` method exists.

## Validation

- `cargo check -p valori-node -p valoricore-ffi` — clean.
- Full workspace tests: all passed (see E4 validation).

## Follow-ups

- Tag-filtered search in FFI still goes through `kernel_state()`. A future phase can add `Engine::search_l2_filtered` to eliminate this.
- `engine.state.clone()` in `replication.rs:336` (used to seed EventCommitter at bootstrap) uses `pub(crate)` access — correct, same crate. No change needed.
