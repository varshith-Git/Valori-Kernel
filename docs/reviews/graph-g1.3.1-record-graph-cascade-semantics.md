# G1.3.1 — Record → Graph Cascade Semantics

*Audit and design plan. **No source code was changed.** The audit found a critical, production-reachable data-loss bug; the remedy requires a semantic decision that the source code does not settle, so this document stops at a recommendation and implementation plan and waits for approval.*

---

## 1. Objective

Determine what actually happens to graph nodes when the record they reference is deleted — across standalone, cluster, replay, snapshot, and restart — and decide the correct lifecycle model.

---

## 2. Existing Implementation

### Record deletion — the two entry points

| Layer | Standalone | Cluster |
|---|---|---|
| HTTP | `POST /v1/delete`, `POST /v1/soft-delete` → `routes/records.rs:53` (shared) | same shared handler |
| `RecordOps::delete` | `server.rs:462-492` | `cluster_server.rs:1342-1381` |
| Mechanism | `Engine::delete_record` / `soft_delete_record` (`valori-engine/src/engine.rs:1240`, `:1217`) | `raft_write_data(KernelEvent::DeleteRecord \| SoftDeleteRecord)` — **nothing else** |
| Cascade to nodes? | **Attempts one node** via `record_to_node` | **None at all** |

```rust
// valori-engine/src/engine.rs:1240-1250 — standalone hard delete
pub fn delete_record(&mut self, id: u32) -> Result<(), EngineError> {
    if let Some(node_id) = self.record_to_node.get(&id).copied() {   // ← ONE node, from a cache
        self.delete_node(node_id)?;
    }
    let event = KernelEvent::DeleteRecord { id: RecordId(id) };
    self.commit_and_apply_ns(&event, DEFAULT_NS.0)?;                  // ← namespace hardcoded
    ...
}
```

```rust
// cluster_server.rs:1354-1368 — cluster: no cascade whatsoever
let event = if soft { KernelEvent::SoftDeleteRecord {..} } else { KernelEvent::DeleteRecord {..} };
raft_write_data(&shard.raft, ClientRequest { event, .., namespace_id: ns }).await?;
```

### The kernel does not cascade

`KernelState::apply_event_ns`'s `DeleteRecord` (`valori-kernel/src/state/kernel.rs:362-370`) and `SoftDeleteRecord` (`:372-380`) arms touch **only** `RecordPool` + the vector index. Neither reads or mutates `NodePool`. Cascade is therefore purely an Engine-layer convention that exists on one execution path.

**Verified empirically** (probe P1): after `DeleteRecord`, `node_count()==1`, the node survives with `record = Some(<deleted>)`, and `check_invariants()` returns `Err(NotFound)`.

### Two enforcers say that state is invalid

- `KernelState::check_invariants` (`kernel.rs:833-837`) — every `node.record` must resolve to a live record.
- `decode_state` (`valori-kernel/src/snapshot/decode.rs:341-346`) — **hard error** if a node references a missing record slot.

So the codebase *does* prove the invariant "a node's `record` must point at a live record." It does **not** prove which remedy is intended when a record is deleted (see §7).

---

## 3. Actual Record ↔ GraphNode Cardinality

**Optional, one-record-to-many-nodes, one-directional.**

- `GraphNode.record: Option<RecordId>` (`valori-kernel/src/graph/node.rs:11`); `Record` has no back-pointer.
- `CreateNode` (`kernel.rs:382-395`) validates only that the record exists and shares the node's namespace. **No uniqueness constraint on `record`.**

**This is not theoretical.** Shipped endpoints create a *new* node for an *existing* record on every call, with no reuse check:

| Endpoint | Site | Behavior |
|---|---|---|
| `/v1/memory/contradict` | `server.rs:976-981` | `create_node_for_record(Some(record_a))` + `(Some(record_b))` — unconditional |
| `/v1/memory/consolidate` | `server.rs:909-912` | same pattern |
| `/v1/memory/upsert` | `server.rs:753-754` | one `Chunk` node per record |

So `memory_upsert(R)` then `contradict(R, X)` then `contradict(R, Y)` leaves **record R referenced by three nodes** using only documented SDK methods.

---

## 4. Proven Bugs

All confirmed by executed probes (since removed — this phase changed no source).

### BUG-1 — CRITICAL: hard delete corrupts the snapshot; node cannot restart

`encode_state` writes a hard-deleted record slot as *absent*, while surviving nodes still carry `record = Some(rid)`. `decode_state` then rejects the file.

Reproduced with **only shipped endpoint semantics** (`memory_upsert` → `contradict` ×2 → `delete`):

```
record 0 referenced by nodes 1, 2, 4
after delete_record: orphan nodes still referencing deleted record = [1, 2]
check_invariants -> Err(NotFound)
SHUTDOWN SNAPSHOT: 8438 bytes written; RESTART decode -> Err("InvalidOperation")
```

The node writes a shutdown snapshot **it cannot read back**. Impact:
- Snapshot-only recovery: node fails to start.
- Event-log recovery (`try_recover` tries the log first): starts, but reproduces the same invalid state.
- Object-store DR (`restore_from_store`) is snapshot-based → same failure.

Soft delete is **safe**: the slot stays `Some`, so decode and `check_invariants` both pass (probe P3).

### BUG-2 — HIGH: standalone cascade is partial and order-dependent

3 nodes on one record; `delete_record` cascaded **only the last-written one**, orphaning the other two:

```
record=0 nodes=0,1,2 → after delete_record, surviving nodes = [0, 1]
check_invariants -> Err(NotFound)
```

Root cause: `record_to_node` is `HashMap<u32,u32>` — single-valued, last-write-wins (`post_apply_derived`, `engine.rs:1375`).

### BUG-3 — HIGH: standalone/cluster lifecycle divergence

Standalone attempts a (partial) cascade; cluster performs **none**. Identical API calls produce different canonical graph state on the two paths — a strictly larger divergence than the G1.3 seed-resolution one.

### BUG-4 — HIGH: cross-namespace record deletion is permitted

Probe P5: a record in namespace 1 was deleted through a call scoped to the default namespace, successfully.

- Standalone `RecordOps::delete` takes `_ns: u16` — **unused** (`server.rs:464`).
- `Engine::delete_record`/`soft_delete_record` hardcode `DEFAULT_NS.0`.
- The kernel arms read `namespace_id` from the *record itself*, so the passed value is never validated.
- Cluster passes `namespace_id: ns` into the event, but the kernel ignores it for these arms — so cluster does not validate either.

This is the same vulnerability class G1.1.1 fixed for graph *reads*, now on the record *mutation* path.

### BUG-5 — LOW: `reranker` not cleaned on hard delete

`soft_delete_record` calls `reranker.remove` (`engine.rs:1224`); `delete_record` does not (`:1240-1249`). Cosmetic/leak-only; noted for completeness.

---

## 5. `record_to_node` Audit

| Question | Finding |
|---|---|
| Declared | `engine.rs:134` — `pub record_to_node: HashMap<u32, u32>` |
| Populated | `post_apply_derived` on `CreateNode` (`:1375`); `rebuild_record_to_node` (`:381-388`) |
| Removed | `apply_committed_event{,_ns}` on `DeleteNode` (`:1330`, `:1348`) — **unconditional**, drops the entry even when sibling nodes survive |
| Multi-value? | **No** — single `u32` |
| Canonical? | **No.** Absent from snapshot encode/decode and from `hash_state_blake3`. Purely derived. |
| Rebuilt on recovery? | Yes — `try_recover` (`:1579`, `:1644`) and `restore_from_components` (`:1689`) |
| Can disagree with `NodePool`? | **Yes, proven** (BUG-2; and G1.3 proved the restart-repair divergence) |
| Remaining consumers | `soft_delete_record` (`:1218`), `delete_record` (`:1241`). G1.3 already removed the GraphRAG consumer. |
| Other caches, same class? | `created_at`, `batch_seen`, `reranker` are all keyed by `RecordId` only — no multi-value hazard. `record_to_node` is the only one modelling a 1:N relationship as 1:1. |

**Conclusion**: with only two consumers left, and both of them wrong, `record_to_node` has no correct remaining use. Deriving from `NodePool` (as G1.3 did for seeds) removes the whole class of bug rather than patching it.

---

## 6. Soft vs Hard Delete — actual semantics

| Property | Soft (`SoftDeleteRecord`) | Hard (`DeleteRecord`) |
|---|---|---|
| Slot in `RecordPool` | Retained, `FLAG_SOFT_DELETED` set (`pool.rs:91-103`) | Set to `None` (`pool.rs:49-61`) |
| `records.get()` | Returns `Some` | Returns `None` |
| `check_invariants` w/ referencing node | **Passes** | **Fails** |
| Snapshot decodable w/ referencing node | **Yes** | **No — BUG-1** |
| Vector search | Excluded (`is_searchable()`, `record.rs:71-73`) | Excluded (gone) |
| Namespace list | Unlinked from the intrusive list in both cases | same |
| Physically reclaimed later? | **No** — no compaction/GC path exists anywhere in the codebase |

**Key asymmetry**: soft delete is already safe for graph linkage. Only hard delete breaks the invariant.

---

## 7. Semantic Options

The invariant is proven; the **remedy is not**. Three models satisfy `node.record ⇒ live record`:

### Option A — cascade-delete every node referencing the record

- **Canonical state**: valid. **Event log**: explicit `DeleteNode` per node (if done at Engine/API layer) → replay exact, no kernel change. **Snapshot/BLAKE3**: unaffected, no format or hash-contract change. **GraphRAG**: seeds vanish with the record — consistent. **Determinism**: full, if cascade order is defined (ascending `NodeId`).
- **Cost**: deleting a record silently destroys graph structure — `_delete_node` also cascades all incident **edges** (`kernel.rs:770-774`). A `Contradicts` edge to an unrelated surviving record disappears as collateral.
- **Backward compat**: replaying an *existing* log that hard-deleted a multi-node record yields different state than before → different hash for that log. But such logs currently produce invalid, undecodable state, so there is no valid prior behavior to preserve.

### Option B — orphan the nodes (null out `node.record`)

- Preserves graph topology; the node survives as a structural node.
- **Requires a canonical change**: no event can mutate `node.record` today. Needs a new `KernelEvent` variant (or new semantics on `DeleteRecord`) → wire-format addition, and `hash_state_blake3` already commits `node.record`, so hashes change for affected states.
- Larger blast radius; arguably better *product* semantics for a relationship store.

### Option C — reject record deletion while nodes reference it

- No canonical change; smallest diff. But it is an **API breaking change** (`/v1/delete` starts returning an error for previously-accepted calls), and it makes deletion require a client-side cascade — poor ergonomics, and it strands existing already-corrupted states.

### Option D — restrict hard delete, keep soft delete as the safe default

Soft delete is already invariant-safe (§6). One could route API deletes to soft-delete-only. Rejected as a *primary* remedy: it does not fix already-corrupted states, does not fix the cluster/standalone divergence, and silently changes durability semantics.

---

## 8. Recommendation

**Option A — cascade-delete all referencing nodes — implemented at the Engine/API layer, emitting explicit `DeleteNode` events, on both execution paths.**

Why this fits Valori specifically:

1. **It is the intent already present in code.** Standalone already attempts exactly this (`engine.rs:1241`); it is simply single-valued and cache-backed. Option A fixes the existing intent rather than inventing new semantics.
2. **Zero canonical/format/hash change.** Cascade expressed as ordinary `DeleteNode` events means the kernel, the wire format, the snapshot format, and the BLAKE3 contract are all untouched — the strongest constraint this phase carries.
3. **Replay stays exact.** The log contains every `DeleteNode` explicitly, so replay reproduces state event-for-event with no implicit side effects — consistent with G0's "one authoritative apply path."
4. **Fixes both divergences at once.** Both paths emit the same event sequence, so standalone/cluster parity holds by construction (the G1.3 pattern).
5. **Option B's cost is not currently justified.** It requires a new canonical event and changes hashes. If preserving orphaned graph structure later proves valuable, it can be introduced deliberately — Option A does not foreclose it.

**Determinism rule**: cascade in **ascending `NodeId`** order, derived from `NodePool` at delete time — matching the `resolve_seed_nodes` convention G1.3 established.

### Honest caveat

Option A means **deleting a record deletes graph structure**, including edges to unrelated records. That is a genuine product consequence, not merely an implementation detail. If the desired product behavior is "graph survives record deletion," the correct answer is **Option B**, and this phase should not proceed as planned. **That specific choice is a product decision I am flagging rather than making** — everything else here is settled by code evidence.

---

## 9. Namespace Isolation (BUG-4 remedy, in scope)

Record deletion must validate that the target record's `namespace_id` matches the resolved namespace, collapsing mismatch into "not found" (never confirming cross-tenant existence) — exactly the pattern G1.1.1 established for graph reads. Required on **both** paths, and the cascade must only ever touch nodes in that same namespace (guaranteed structurally, since `CreateNode` enforces `node.namespace_id == record.namespace_id`).

---

## 10. Performance

Measured (release; "find all live nodes referencing record R", the scan a correct cascade needs):

| Live nodes | Scan cost | Matches |
|---|---|---|
| 1,000 | 791ns | 100 |
| 10,000 | 5.61µs | 1,000 |
| 100,000 | 82.2µs | 10,000 |
| 1,000,000 | 1.61ms | 100,000 |

Linear, as expected. At 1M nodes a record delete costs ~1.6ms of scan — acceptable for a delete (a write-path, non-hot operation), and consistent with G1.2/G1.3's conclusion that no additional index is justified. **No new index is proposed.** If record-delete throughput ever becomes a measured bottleneck, a correctly-maintained multi-valued `RecordId → Vec<NodeId>` index is the fallback — explicitly not built now.

---

## 11. Compatibility

| Surface | Impact |
|---|---|
| `KernelEvent` variants | **None** — reuses existing `DeleteNode` |
| WAL / event-log format | **None** |
| Snapshot format | **None** |
| BLAKE3 hash contract / domain version | **None** |
| Committed fixtures | **None** — no fixture exercises this path |
| Replay of *existing* logs | Unchanged for all valid logs. Logs that hard-deleted a multi-node record replay to a *different* (now valid) state — but their prior state was invalid and undecodable, so no valid behavior is lost |
| Python SDK | **None** — no signature change |
| Standalone/cluster parity | **Improved** (currently divergent) |
| API | `/v1/delete` gains namespace validation → a previously-succeeding cross-namespace delete now 404s. This is a **deliberate security fix**, matching G1.1.1's precedent |

---

## 12. Test Matrix (to be implemented on approval)

Multi-node matrix on `Record R ← {A, B, C}`: delete A; delete B; delete C; A→B; B→A; delete R; delete R after A gone; delete R after B,C gone — each followed by `check_invariants`, snapshot encode+decode, event-log replay, and restart, asserting equivalence and validity. Plus: soft vs hard parity; cross-namespace deletion rejected (both paths); ID-collision across namespaces; GraphRAG after each mutation; standalone/cluster equivalence; `route_parity`.

Every test must be verified to **fail without the fix** (the revert-check discipline used in G1.1.1/G1.3).

---

## 13. Implementation Plan (on approval)

1. **`Engine::delete_record` / `soft_delete_record`** — replace the `record_to_node` lookup with a canonical `NodePool` scan collecting *all* referencing live nodes; emit `DeleteNode` for each in ascending `NodeId`, then the record event. Add namespace validation; stop hardcoding `DEFAULT_NS`.
2. **Cluster `RecordOps::delete`** (`cluster_server.rs:1342`) — resolve the same node set from the shard's state machine, emit one `DeleteNode` Raft write per node (ascending), then the record event.
3. **Standalone `RecordOps::delete`** (`server.rs:462`) — thread the real `ns` through instead of `_ns`.
4. **Delete `record_to_node`** entirely (last two consumers removed) — or, if any consumer remains, convert to multi-valued. Prefer removal.
5. **Optional (BUG-5)**: `delete_record` should call `reranker.remove`, matching soft delete.
6. Tests per §12; full verification (`fmt`, `clippy -D warnings`, workspace tests, `route_parity`; wasm32 only if the kernel is touched — it should not be).

**Not in scope**: any hash/format change, graph index, hybrid ranking, GraphRAG ranking, or Option B's canonical `node.record` mutation.

---

## 14. Risks

- **Existing corrupted deployments.** Any node that already hard-deleted a multi-node record has an undecodable snapshot *today*. This fix prevents new corruption but does not repair existing files. A repair path (tolerant decode that nulls dangling `node.record`, or a rebuild-from-event-log procedure) may be warranted — **flagged as a separate decision**, since a tolerant decoder weakens a deliberate G0.1 integrity check.
- **Silent graph loss** is inherent to Option A (§8 caveat) — the product decision above.
- **Cluster cascade is multi-event**: N+1 Raft writes rather than one. Not atomic as a unit — a mid-sequence failure could leave partial cascade. Existing multi-event flows (`memory_upsert`, ingest) already have this property, so it is consistent with the codebase, but it should be stated rather than glossed.

---

## 15. Final Verdict

**Audit complete. Implementation NOT started — awaiting approval.**

- The invariant "a node's `record` must reference a live record" **is proven by code** (`check_invariants`, `decode_state`).
- A **critical, production-reachable data-loss bug (BUG-1)** was proven: shipped endpoints can put a node into a state where it cannot restart from its own snapshot.
- Four further bugs (partial cascade, standalone/cluster divergence, cross-namespace deletion, reranker leak) were proven.
- The **remedy is a product decision**: Option A (cascade, recommended — smallest blast radius, zero canonical change, matches existing intent) vs Option B (orphan nodes, preserves graph topology, requires canonical + hash change).

**Approval needed on one question**: *when a record is deleted, should the graph nodes referencing it be deleted (Option A) or preserved as orphans (Option B)?* Everything else in the plan follows from that answer.

Given BUG-1's severity, I'd also flag it as worth fixing on an expedited path regardless of which option is chosen.
