# Phase S15 — Namespace-scoped audit-log entries (standalone collection recovery)

Branch: `Node-scaleup` (follows S13/S14; commit `8630d8f` was the prior tip).

## Goal

Fix a real data-visibility bug the user hit: after uploading a document into a named collection, closing the project, and reopening it, the collection appeared empty ("no documents"). Root cause: the standalone engine's audit log recorded events as `LogEntry::Event(KernelEvent)`, and `KernelEvent` carries no namespace. At write time the engine applied each event to the correct collection *and* wrote it to the log — but on recovery, `replay_events()` re-applied every logged event via `apply_event()` (which hardcodes the default namespace). So all records/nodes replayed into collection 0, and the named collection came back empty. **No data was ever lost** — the events were all on disk — but they were re-shelved into the wrong collection on every restart. This is the standalone-mode twin of the cluster-mode bug fixed in S3a, and the exact scenario CLAUDE.md's invariant #2 ("namespace isolation at recovery") warns about.

## Delivered

- **`crates/valori-wire/src/lib.rs`** — new append-only `LogEntry::EventNs { namespace_id: u16, event: KernelEvent }` variant (variant #3, after `Event`/`Checkpoint`/`Admin`). The wire encode/decode/chain-advance layer is generic over `LogEntry`, so no serialization changes were needed. Pre-S15 logs (all `Event`) decode and replay exactly as before; default-namespace writes still emit the plain `Event` variant, so default-collection logs stay byte-identical.
- **`crates/valori-node/src/events/event_commit.rs`** — `commit_event`/`commit_batch` now delegate to new `commit_event_ns`/`commit_batch_ns`, which shadow-apply and live-apply via `apply_event_ns`, and write `EventNs` when `namespace_id != 0` (plain `Event` for the default namespace).
- **`crates/valori-node/src/events/event_replay.rs`** — `read_all_segments` now returns `Vec<(u16, KernelEvent)>` (each event paired with its recovered namespace; pre-S15 `Event` entries pair with namespace 0). `replay_events` applies each via `apply_event_ns`. `recover_from_event_log` threads this through; the journal (namespace-agnostic by design — height/dedup only) is fed the bare events.
- **`crates/valori-node/src/engine.rs`** — the namespaced write paths (`insert_record_from_f32_ns`, `insert_encrypted_ns`, `insert_batch_ns`, `create_node_for_record`) now call `commit_event_ns`/`commit_batch_ns` with their real `namespace_id` instead of dropping it.
- **Readers taught the new variant**: `valori-verify` (lib + `main.rs` — replays `EventNs` via `apply_event_ns` so the recomputed state hash matches the node's), cluster `/v1/timeline`, CLI `timeline`/`inspect`, CLI `engine.rs` point-in-time replay, `event_log.rs` event counter, the legacy `replication.rs` follower-sync stream (both the streaming gate and the follower apply, so a replicated collection write lands in the same collection), and the `valori-wire` evolution test.

## Findings

- **`search_as_of` (point-in-time historical search) in a non-default collection is a narrower, still-open gap.** It replays from `EventJournal`, which is namespace-agnostic by construction (it stores bare `KernelEvent`s for height/dedup). Reconstructing namespaced historical state would require threading namespaces through the journal too — a larger change than this bug warranted. Default-collection `as_of` search is unaffected; non-default `as_of` search returns empty. Documented here rather than scope-crept into the journal layer.
- **Existing pre-S15 project logs are not retroactively fixed.** `firstone`/`qwerty` (and any project created before this phase) have their collection writes recorded as plain `Event`, so those specific documents remain in the default collection on recovery. Only writes made *after* this fix land in `EventNs`. A one-time migration was considered out of scope — the events lack the namespace, so the only recoverable source is the `events.namespaces.json` name→id sidecar, which maps names but not which events belonged to which collection. Users wanting existing docs in a named collection should re-upload into a project created after this fix.

## Validation

- `cargo test` across `valori-kernel`, `valori-wire`, `valori-node`, `valori-consensus`, `valori-verify`, `valori-cli`: **369 passed, 0 failed.**
- New regression test `event_replay::tests::namespaced_events_recover_into_their_own_collection`: writes a record into namespace 1 via `commit_event_ns`, drops the committer (flush), recovers from scratch, asserts the record recovers into ns 1 (not ns 0) and the default-namespace record stays in ns 0.
- `cargo build -p valori-kernel --target wasm32-unknown-unknown`: clean (kernel untouched; invariant #7 holds).
- `cargo clippy` on the touched crates: clean.
- **Live end-to-end smoke test** against the running UI's spawned node: created project `s15test`, collection `docs-a`, inserted a record; searched `docs-a` → found; closed the project; reopened; searched `docs-a` again → **still found** (was empty pre-S15); confirmed the default collection did **not** absorb the record. Deleted the test project; pre-existing projects untouched.

## Follow-ups

- Namespace-aware `search_as_of` (point-in-time search in non-default collections) — requires the journal to carry namespaces; see Findings.
- Optional one-time repair tool for pre-S15 logs — low value given the namespace data isn't in those events; flagged, not planned.
