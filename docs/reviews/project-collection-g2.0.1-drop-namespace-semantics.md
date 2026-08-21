# G2.0.1 — Collection Drop/Delete Semantics Audit

**NO CODE CHANGES.** Focused trace of `DropNamespace` end-to-end, per the
open question flagged in
[project-collection-g2.0-domain-model.md](project-collection-g2.0-domain-model.md)
§20 item 1. Every claim below is code-verified with an exact citation.

---

## 1-3. API endpoint, standalone handler, cluster handler

`DELETE /v1/namespaces/:name` — identical route on both routers:
`crates/valori-node/src/server.rs:406`,
`crates/valori-node/src/cluster_server.rs:369`, both dispatching through
the shared `crate::routes::collections::drop_collection` body
(`routes/collections.rs:102`), which validates the name isn't `"default"`
then calls `ops.drop_collection(name)`.

- **Standalone** (`server.rs:2824-2829`): thin wrapper → `Engine::drop_collection`
  (`crates/valori-engine/src/engine.rs:988-1018`).
- **Cluster** (`cluster_server.rs:583-597`): fires exactly one Raft write,
  `KernelEvent::DropNamespace{name}`, and nothing else — no local
  cleanup at the HTTP-handler layer at all.

## 4-5. `KernelEvent::DropNamespace` + `KernelState` application

`crates/valori-kernel/src/state/kernel.rs:576-618`, the single
authoritative apply path (used identically by standalone commit, cluster
Raft apply, and event-log replay). Guards: rejects `ns == 0` (the default
namespace can never be dropped, enforced at the kernel level, not just
the API layer which also separately checks this) and `ns >=
MAX_NAMESPACES`. Then, in order, within one synchronous function call:

```rust
// 1. Walk the namespace's record linked list; hard-delete each record
let mut cursor = self.namespace_record_heads[ns];
while cursor != NS_LIST_NIL {
    let next = /* read next_in_ns before nulling */;
    self.records.records[cursor as usize] = None;   // hard delete
    self.index.on_delete(RecordId(cursor));           // kernel-native index only
    cursor = next;
}
self.namespace_record_heads[ns] = NS_LIST_NIL;

// 2. Walk the namespace's node linked list; delete each node via the
//    same cascading node-delete path G1.3.1 established (frees incident edges)
let mut node_ids = /* collect all node ids in this namespace */;
for nid in node_ids {
    if self.nodes.get(nid).is_some() {
        let _ = self._delete_node(nid);
    }
}
self.namespace_node_heads[ns] = NS_LIST_NIL;
```

## 6. RecordPool impact

**Hard delete, not soft delete.** `self.records.records[cursor as usize]
= None` is byte-identical to `RecordPool::delete()`'s own implementation
(`crates/valori-kernel/src/storage/pool.rs:49-61`, also just `self.records[idx]
= None`) — confirmed by direct comparison, not inferred. Every record in
the dropped namespace is freed, unconditionally, regardless of whether it
was previously soft-deleted or active. There is no tombstone, no
audit-preserving path — this is the same hard-delete class G1.3.1
analyzed for single-record deletion, applied in bulk.

## 7. NodePool impact

Every graph node in the namespace is deleted via `_delete_node` (the same
kernel primitive `KernelEvent::DeleteNode` uses), which cascades to that
node's own incident edges (both `first_out_edge`/`first_in_edge` chains)
— confirmed reused, not reimplemented, so it inherits whatever guarantees
`_delete_node` already provides elsewhere in the codebase.

## 8. EdgePool impact

Follows from #7 — every edge incident to a deleted node is freed as a
side effect of `_delete_node`'s own cascade. No edge in the namespace can
survive its namespace's node-deletion loop, since `CreateEdge` already
requires both endpoints to share one namespace (established invariant,
re-confirmed unaffected by this trace).

## 9. Vector index impact — the one real discrepancy this audit found

- **Kernel-native index** (`KernelState.index: ActiveIndex` —
  BruteForce/BinaryQuantization): cleaned up inline, `self.index.on_delete(...)`
  called per deleted record, inside the same kernel apply (line 595
  above). This is the **only** vector index cluster mode has (per the
  G1.4.3 audit), so cluster's vector index is fully, correctly cleaned up
  as a direct consequence of the canonical kernel apply — no extra step
  needed or taken.
- **Standalone's pluggable index** (`Engine.index: Box<dyn VectorIndex>`
  — the real HNSW/IVF/BQ/BruteForce implementation actually serving
  standalone search): **NOT touched by the kernel apply at all** — it's
  a separate structure the kernel has no reference to. `Engine::drop_collection`
  (`engine.rs:998-1002,1012-1014`) compensates for this explicitly and
  correctly:
  ```rust
  let ns_record_ids: Vec<u64> = self.state.iter_records_in_ns(id)
      .map(|r| r.id.0 as u64).collect();   // captured BEFORE the kernel apply
  self.commit_and_apply_ns(&KernelEvent::DropNamespace{...}, id)?;
  for rid in &ns_record_ids {
      self.index.delete(*rid as u32);       // cleans up the REAL standalone index
  }
  ```
  **Confirmed correct** — standalone's derived index is fully cleaned up,
  by a deliberate, explicit post-apply step, not by the kernel event
  itself (which cannot reach it).

## 10. Graph impact

Already covered by #7/#8 — full cascade, no separate graph-specific step
exists or is needed beyond the node/edge pool walk already described.

## 11. Namespace registry impact

- **Standalone**: `self.namespaces.drop(name)` (`engine.rs:994-997`,
  `valori-metadata/src/collection.rs:84`) removes the name→id mapping
  from `CollectionRegistry.map` — called **before** the kernel apply
  (the id is captured first, then used for the event), and
  `flush_namespaces()` (`engine.rs:1016`) persists the updated registry
  to the JSON sidecar afterward.
- **Cluster**: deliberately asymmetric versus `AutoCreateNamespace`, and
  the code comment explains why
  (`crates/valori-consensus/src/state_machine.rs:706-713`): *"AutoCreateNamespace
  speculatively inserts (idempotent) ... DropNamespace only resolves
  (read-only) here, removal happens after a confirmed successful kernel
  apply, below."* The name→id resolution happens read-only
  (`state_machine.rs:725-727`) before the kernel apply is attempted;
  `inner.namespace_registry.map.remove(name)` only executes **if the
  kernel apply succeeded** (`state_machine.rs:835-838`) — so a rejected
  drop (e.g., namespace not found, or `ns==0`) never mutates the
  registry. This is a real, deliberate, better-reasoned invariant than
  the standalone path's "drop the name first, then try the kernel apply"
  ordering — worth noting as an asymmetry between the two paths, not
  necessarily a bug in either (standalone's `namespaces.drop()` cannot
  itself fail in a way that would leave things inconsistent, since the
  only failure mode — name not found — is checked via the `Option` return
  before the kernel apply ever runs).

## 12. Snapshot impact

None beyond the ordinary consequence of the canonical state having
changed — `encode_state` serializes whatever `RecordPool`/`NodePool`/
`EdgePool`/namespace-heads state exists at snapshot time, which after a
completed `DropNamespace` simply has `None` slots and `NIL` heads for
that namespace, indistinguishable from a namespace that never had data.
No special-casing exists or is needed in `encode.rs`/`decode.rs` for this
event.

## 13. Event-log impact

The `DropNamespace` event itself is the durable log entry, committed via
the same DEDUP-CHECK → KERNEL-APPLY → AUDIT-WRITE invariant every other
mutation follows (re-confirmed, not a special case).

## 14. Restart/recovery behavior

Deterministic replay, same as every other event — re-applying the
recorded sequence of `InsertRecord`/`CreateNode`/`CreateEdge`/
`DropNamespace` events reconstructs identical canonical state, since the
kernel-level apply is a pure function of prior state + the event. No
special recovery behavior exists or is needed.

## 15. Raft behavior

Fully traced in #11 — the asymmetric speculative-insert-vs-read-only-resolve
handling is a deliberate design already in the code, not something this
audit is recommending.

## 16. Namespace ID reuse behavior

**Never reused.** `CollectionRegistry.next_id` (`valori-metadata/src/collection.rs:42,49,76-77`)
strictly monotonically increments on every successful `create()` call,
with no corresponding decrement anywhere in `drop()`
(`collection.rs:84`+, confirmed by reading the full function — it removes
the map entry only, never touches `next_id`). A dropped namespace's
integer id is permanently retired; the next collection created (with any
name, including the exact same name as the one just dropped) always gets
a strictly higher id.

## 17. What happens to records still inside the namespace

**Answered definitively in #6/#9**: every record is hard-deleted from the
canonical `RecordPool` (kernel-level, unconditional, atomic within the
event application) and removed from every index that's actually reachable
from the deletion path — the kernel-native index inline, the standalone
pluggable index via `Engine::drop_collection`'s explicit follow-up loop.

## 18. What happens to graph nodes/edges

**Answered definitively in #7/#8**: every node and its incident edges are
deleted via the same cascade `KernelEvent::DeleteNode` already uses
elsewhere, reused wholesale, not reimplemented for this path.

## 19. Is deletion atomic?

**At the canonical kernel-state level: yes, fully** — one synchronous
function call, no yield points, no partial-completion window observable
from outside (no `check_invariants()` or snapshot can be taken mid-call).
A crash before the event commits leaves state completely untouched
(standard DEDUP→APPLY→AUDIT ordering); a crash after commit leaves state
completely transitioned.

**At the standalone Engine level: no, not fully** — `Engine::drop_collection`'s
three-step sequence (kernel apply → `self.index.delete()` loop →
`self.reranker.remove_batch()` → `flush_namespaces()`) is **not**
transactional as a whole. If the process crashes between the kernel apply
succeeding and the `index.delete()` loop completing, the canonical state
is already correctly, durably empty (a restart will replay/restore
exactly that), but the standalone HNSW/IVF/BQ index retains stale entries
for those now-nonexistent records until either (a) a future
`rebuild_index()` call happens to run (every restart via the event-log
recovery path, per the recovery-breakdown audit's finding that this path
*always* unconditionally rebuilds — so in practice, any restart heals
this specific gap for free), or (b) the process never crashes and the
loop completes normally. **Net severity: low** — the only way this gap
outlives a single restart is a node configured for snapshot-only recovery
(no event log) that also never gets restarted after the crash, an
unusual combination. Confirmed via the same recovery-breakdown audit's
own established mechanics, not a fresh, independent finding.

**A related, confirmed, standing gap — not itself an atomicity failure,
but the same root cause (bookkeeping that lives outside the kernel event
and must be swept up separately)**: neither `Engine::drop_collection`
(standalone) nor cluster's `DropNamespace` handling in `state_machine.rs`
cleans up **`created_at`** (the decay-ranking timestamp map — checked
both `engine.rs:988-1018` and `state_machine.rs:706-842`, neither touches
it) or, on the **cluster** side specifically, **`text_corpus`** (the BM25
raw-text cache — standalone's `reranker.remove_batch()` is the equivalent
cleanup, and it exists; cluster has no analogous call anywhere in its
`DropNamespace` handling). Freed record ids are never reused (#16's
sibling guarantee for `RecordId`, established elsewhere in this
codebase), so these are **inert, unbounded-but-slow memory leaks
proportional to create/drop-namespace churn — not correctness bugs**:
a stale `created_at[old_id]`/`text_corpus[old_id]` entry can never be
misapplied to a different, later record, because that id will never be
issued again. Flagged here precisely because it's real and confirmed,
not escalated beyond what the evidence supports.

## 20. Can deletion leave dangling references?

**No, not observably, at the canonical level** — confirmed by #19's
atomicity analysis: by the time `apply_event_ns` returns (success or
error), there is no intermediate state visible to any other caller.
**Yes, transiently, at the derived-index level on standalone**, per the
narrow crash-window described in #19 — never a canonical-state hazard
(never risks the BUG-1-class undecodable-snapshot failure G1.3.1 found
for single-record deletion, because the *kernel* portion of this cascade
is what wrote the canonical state, and it already left zero dangling
`node.record` references by construction, per #7/#17/#18).

---

## State-transition diagram (what Valori does today — not a proposal)

```
POST /v1/namespaces {name}
        ↓
   ACTIVE  (id allocated, canonical; name in registry, non-canonical
            sidecar — per the G2.0 document's own flagged desync risk,
            unaffected by this audit)
        ↓
DELETE /v1/namespaces/:name
        ↓
   [kernel apply: hard-delete every record, cascade-delete every node
    and its edges, reset namespace heads to NIL — atomic, canonical]
        ↓
   [standalone only, non-atomic w.r.t. the step above: clean derived
    index + reranker corpus; cluster: nothing further needed, kernel-
    native index already cleaned inline]
        ↓
   [registry: name removed — standalone before the kernel apply even
    runs (harmless, since the only failure mode is "not found," checked
    first); cluster only after a CONFIRMED successful kernel apply]
        ↓
      GONE  — no tombstone, id permanently retired (#16), name freed for
              reuse by a future, differently-numbered collection
```

**This confirms the G2.0 document's own minimal recommended lifecycle
(§11 there) was correct, not merely a guess**: there is no meaningful
"still dropping" transient window to represent as a state — the entire
operation, at the canonical level, is a single synchronous, atomic
kernel-event application. The only non-atomicity found (§19) is narrow,
low-severity, self-healing on restart, and standalone-only — not the
kind of gap that would justify adding a `DELETING` lifecycle state to
the product-facing API, though it may be worth a small, targeted fix on
its own merits (making `Engine::drop_collection`'s cleanup loop
resumable, or simply accepting that restart already heals it) — **that
would be an implementation decision, not proposed here.**

---

## Answer to the G2.0 blocking question

**"What exactly happens when `DropNamespace` is executed?"** — fully
answered, with citations, above. **Every record, graph node, and graph
edge in the collection is unconditionally, atomically, hard-deleted from
canonical state.** There is no soft-delete option, no confirmation step,
no recovery path once committed, and no tombstone. This is a genuinely
destructive operation today, with the recommended minimal lifecycle
(§11 of the G2.0 document) now confirmed correct rather than merely
plausible.

**This unblocks G2.0's open question #1.** The two secondary,
lower-severity findings surfaced along the way (§19's non-atomic
standalone derived-index cleanup, and the `created_at`/`text_corpus`
leak) are new information not previously identified in G2.0 — flagged
here for awareness, not escalated into new blocking questions, since
neither affects canonical-state correctness or the collection-lifecycle
design decision this audit was scoped to resolve.
