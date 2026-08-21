# Phase G1.3.1 (implementation) — Record → GraphNode Cascade Fix

Companion to the audit: [docs/reviews/graph-g1.3.1-record-graph-cascade-semantics.md](../reviews/graph-g1.3.1-record-graph-cascade-semantics.md).
That document diagnosed five bugs and posed one open product decision
(cascade-delete vs. orphan referencing nodes on record deletion). The user
approved **Option A (cascade-delete)**. This phase implements it.

## Goal

Fix BUG-1 (CRITICAL): hard-deleting a record that still has graph nodes
referencing it leaves those nodes dangling, and the resulting snapshot fails
to decode on restart — a real, shipped-endpoint-reachable data-loss path.
Fix the four related bugs found alongside it (partial cascade, standalone/
cluster divergence, missing namespace check, missing reranker cleanup) using
the approved Option A semantics, on both execution paths.

## Delivered

**`crates/valori-rag/src/graph.rs`** — new `nodes_referencing_record(state,
record_id) -> Vec<u32>`: enumerates every live node whose `record`
back-reference matches, in ascending `NodeId` order. This is the shared
primitive both paths use to cascade correctly to *every* referencing node,
not just one cached mapping.

**`crates/valori-engine/src/engine.rs`**:
- Removed the `record_to_node: HashMap<u32, u32>` field entirely (and
  `rebuild_record_to_node`, its population in `post_apply_derived`'s
  `CreateNode` arm, and its removal in `apply_committed_event`/
  `apply_committed_event_ns`'s `DeleteNode` handling). The audit confirmed
  its only two remaining consumers were the two buggy delete paths fixed
  here; nothing else in the codebase reads it.
- `delete_record` now calls `nodes_referencing_record` to enumerate every
  live referencing node, deletes each via the existing `delete_node` (which
  already cascades to that node's incident edges), in ascending order, then
  deletes the record. Also now calls `reranker.remove(id)` (BUG-5 — hard
  delete was the only delete path that skipped it; soft delete already did).
- `soft_delete_record` no longer touches the graph at all. The record row
  survives a soft delete (flagged, not freed), so `node.record ⇒ live
  record` already holds — the pre-fix code's `record_to_node`-driven partial
  node cascade there was itself a bug, now removed.

**`crates/valori-node/src/server.rs`** (`SharedEngine::delete`, standalone) —
BUG-4: validates `record.namespace_id == ns` before deleting (matching
`GraphOps::delete_node`'s existing convention); mismatch behaves exactly
like "not found" (404), never confirming cross-tenant existence.

**`crates/valori-node/src/cluster_server.rs`** (`DataPlaneState::delete`,
cluster) — BUG-2/BUG-3: same namespace check, plus the cascade the
standalone path already had and the cluster path previously had *none* of.
Enumerates referencing nodes via `shard.state_machine.with_state(...)`,
issues one `KernelEvent::DeleteNode` `raft_write_data` call per referencing
node (ascending order) before the `DeleteRecord`/`SoftDeleteRecord` write.

## Findings

All five bugs from the audit are now fixed:

| Bug | Fix |
|---|---|
| BUG-1 (CRITICAL) — hard delete corrupts the state's own snapshot | Cascade removes the dangling `node.record` reference before it can ever be encoded |
| BUG-2 (HIGH) — standalone cascade was partial (single cached node) | `nodes_referencing_record` enumerates all of them |
| BUG-3 (HIGH) — cluster did zero cascade | Cluster now cascades identically to standalone |
| BUG-4 (HIGH) — cross-namespace record deletion permitted | Both paths now validate namespace before deleting |
| BUG-5 (LOW) — `reranker.remove` skipped on hard delete | Added to `delete_record` |

One incidental design correction: soft delete's pre-fix code called
`delete_node` on the cached node (a partial, buggy cascade) even though the
record row survives a soft delete and the invariant never required it. That
cascade is now removed — soft delete is graph-neutral, per the approved
lifecycle.

## Validation

New tests (14 total, all revert-and-confirmed non-vacuous against the
pre-fix code — 4 of 8 in `graph_cascade_delete.rs` fail without the fix,
including the exact BUG-1 snapshot-corruption regression):

- `crates/valori-node/tests/graph_cascade_delete.rs` (8 tests) — zero/one/many
  referencing nodes, deterministic ascending-order cascade, incident-edge
  cleanup, soft-delete graph-neutrality, the BUG-1 encode→decode regression
  test, namespace scoping of the enumeration primitive.
- `crates/valori-node/tests/api_graph_cascade_delete.rs` (3 tests) — HTTP-level
  standalone: cascade on hard delete, no cascade on soft delete, cross-namespace
  404.
- `crates/valori-node/tests/cluster_graph_cascade_delete.rs` (3 tests) — same
  matrix over a real single-node Raft cluster, proving standalone/cluster
  parity.

Full verification: `cargo fmt --check` clean; `cargo clippy -p valori-engine
-p valori-rag -p valori-node --all-targets -- -D warnings` clean; `cargo test
-p valori-kernel` unaffected (kernel untouched this phase — its own
`no_std`/wasm32 invariant wasn't at risk, no rebuild needed); `cargo test -p
valori-node` — **338 passed, 0 failed** (up from 324 before this phase);
`route_parity` — 2/2 passed (no new routes; only existing handler bodies
changed, so parity was never at risk, but the mechanical check still ran
clean).

## Follow-ups

- **Not addressed** (explicitly out of scope, flagged in the audit as a
  separate decision): repairing already-corrupted (undecodable) snapshots
  that may exist in deployments today as a result of BUG-1 prior to this
  fix. No migration/repair tool exists; if a real corrupted snapshot is
  found in the wild, that is its own phase.
- Cluster cascade is multi-`raft_write_data`-call, non-atomic (matches the
  audit's risk note): if the leader crashes mid-cascade, some referencing
  nodes may be deleted and others not, before the record itself is deleted.
  This is a partial-application risk inherent to expressing a multi-node
  operation as several sequential Raft writes, not something this phase's
  scope covers fixing (would require a new compound `KernelEvent`, which is
  exactly the kind of canonical-event-format change Option A was chosen to
  avoid).
- `crates/valori-node/tests/vector_graph_retrieval.rs`'s
  `soft_deleted_record_drops_out_of_vector_results` test already covered
  soft-delete's search-visibility behavior (G1.3); this phase didn't touch
  that test, only confirmed it stays green.
