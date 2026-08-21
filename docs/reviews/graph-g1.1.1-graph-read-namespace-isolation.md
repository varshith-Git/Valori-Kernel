# G1.1.1 — Graph Read Namespace Isolation

*Follow-up to [`graph-g1.1-query-primitives.md`](graph-g1.1-query-primitives.md) §12. Closes the tenant-isolation gap G1.1 discovered but deliberately did not fix in that phase, per explicit direction: this is a security boundary problem, not a cleanup task, and it must land before any traversal-acceleration or hybrid-retrieval work is built on top of the existing graph read API.*

---

## 1. Objective

G1.1 found that `get_node`, `node_edges`, and `subgraph` — on both the standalone and cluster `GraphOps` implementations — accept a resolved namespace but never verify the node they return actually belongs to it. G1.1.1's sole objective: audit every graph read path for this class of bug, fix every real instance at the kernel/node layer itself (not in a Cloud authorization layer), and prove it with the exact namespace-isolation matrix specified.

---

## 2. Full Audit of Graph Read Paths

Every path that resolves a namespace and then looks up a node/edge by a caller-supplied numeric id, re-verified from source (not assumed from G1.1's notes):

| Path | Standalone | Cluster | Status before this phase | Fixed? |
|---|---|---|---|---|
| `get_node` | `server.rs:1710` (`impl GraphOps for SharedEngine`) | `cluster_server.rs:1860` (`impl GraphOps for DataPlaneState`) | **Broken** — `ns` ignored (standalone: literally `_ns`) / used only to pick a shard (cluster) | ✅ Yes |
| `node_edges` | `server.rs:1725` | `cluster_server.rs:1880` | **Broken**, same pattern | ✅ Yes |
| `subgraph` | `server.rs:1758` | `cluster_server.rs:1921` | **Broken**, same pattern | ✅ Yes |
| `delete_node` | `server.rs:1697` | `cluster_server.rs:1830` | **Exposed indirectly** — the shared handler (`routes/graph.rs::delete_node`) already gated deletion on `ops.get_node(ns, id).is_none()`, so fixing `get_node` closes this at the API layer; the `GraphOps::delete_node` implementations themselves had no independent check | ✅ Yes — fixed directly too, as defense-in-depth (§4), not left to depend solely on call ordering in the shared handler |
| `list_nodes` | `server.rs:1739` (via `engine.nodes_in_ns(ns)`) | `cluster_server.rs:1900` (`filter(\|n\| n.namespace_id == ns)`) | **Already correct** | No change needed |
| `query` (G1.1's new primitive) | `server.rs` | `cluster_server.rs` | **Already correct** — built with an explicit `namespace_id` check from the start (G1.1 §12) | No change needed |
| GraphRAG (`/v1/graphrag`, `/v1/graphrag` cluster) | `server.rs:1843` | `cluster_server.rs:2025` | **Safe by construction, audited not fixed** — see §3 | N/A |
| Entity extraction (`/v1/ingest/extract-entities`) | `server.rs:3909` | — (not a cluster-exposed capability; unaffected) | **Safe by construction, audited not fixed** — see §3 | N/A |
| Node-lookup helper functions elsewhere (`create_node_for_record`, `nodes_in_ns`, kernel-level `get_node`/`outgoing_edges`/`incoming_edges`) | — | — | Correctly namespace-blind **by design** — G0 established these are low-level primitives; namespace enforcement is the caller's responsibility, which is exactly the contract this phase closes at every caller that has one | N/A |

**No other graph read path with this vulnerability class was found.** The audit covered every `GraphOps` trait method, every handler in `routes/graph.rs`, both `/v1/graphrag` handlers, and `/v1/ingest/extract-entities`.

---

## 3. Why GraphRAG and Entity Extraction Were Audited, Not Fixed

Re-verified from source, not assumed from G1.1's notes:

- **GraphRAG's request shape** (`GraphRagRequest { query_vector, k, depth, collection }`, `server.rs:1831-1838`) has **no `root`/`node_id`/`start` field** — there is no way for a caller to point it at an arbitrary node id. Its seeds come entirely from `resolve_seed_nodes(state, record_ids)`, where `record_ids` are the output of a namespace-scoped vector KNN. `resolve_seed_nodes` (`crates/valori-rag/src/graph.rs`) matches nodes by their `record` field against that already-scoped set — and `CreateNode` (kernel layer, G0's invariant) already requires a node's `record` to share the node's own namespace. So a node `resolve_seed_nodes` returns cannot belong to a different namespace than the KNN search that produced its record id. This is a **structural** guarantee (enforced at the canonical mutation layer), not an incidental one.
- **`extract_entities` takes no node-id input at all** (`ExtractEntitiesRequest { text, entity_types, namespace, model }`, confirmed by re-reading the handler at `server.rs:3909-3990`) — it only ever *creates* new nodes/records in the resolved namespace; there is nothing to look up.

Both conclusions are proven by a passing test, not just argued: `graphrag_has_no_direct_node_id_parameter_to_exploit` (§6) confirms a GraphRAG call against a namespace with real cross-namespace graph data present returns no leaked hits.

---

## 4. The Fix

One pattern, applied at every broken call site, on both execution paths — validate the looked-up node's `namespace_id` against the resolved `ns` **before** returning/traversing/deleting, collapsing "wrong namespace" into the same outcome as "does not exist" (never confirm cross-tenant existence):

```rust
// get_node / node_edges (source-node check) / subgraph (root-node check)
engine.get_node(NodeId(id))
    .filter(|n| n.namespace_id == ns)   // or an equivalent match-guard
    .map(|n| ...)
```

- **`get_node`**: `.filter(|n| n.namespace_id == ns)` added directly to the existing `Option` chain — smallest possible diff.
- **`node_edges`**: validates the **source** node's namespace before listing its outgoing edges. Sufficient by construction — edges cannot cross namespaces (G0's invariant), so a correctly-scoped source node implies every one of its edges is also in-namespace; no per-edge check was needed or added.
- **`subgraph`**: validates the **root** node's namespace before calling `expand_subgraph`. A wrong-namespace root now behaves **exactly like a nonexistent root already did** — empty `{nodes: [], edges: []}`, `200 OK` — deliberately not a new `404` code path, to avoid an unrelated response-shape change for callers relying on the existing "nonexistent root → empty result" convention.
- **`delete_node`**: fixed independently at the `GraphOps` implementation level, **not left to depend on the shared handler's call ordering** (defense-in-depth, per the explicit instruction that the kernel/node layer itself must enforce the invariant):
  - Standalone: reads the node first, checks `namespace_id == ns`, only then calls the mutating `engine.delete_node(id)`.
  - Cluster: reads the node from the target shard's state machine first (`with_state`), checks namespace, and only submits the `KernelEvent::DeleteNode` Raft write if it matches — the write is never even proposed for a cross-namespace id.

**Where the fix lives**: entirely in `crates/valori-node/src/server.rs` and `crates/valori-node/src/cluster_server.rs` (the `GraphOps` implementations). Nothing was added to `valori-kernel`, no new `KernelEvent` variant, no Cloud/authorization-layer code anywhere — exactly as instructed: *"the kernel/node graph read operation itself should enforce its namespace invariant. Cloud authorization and graph namespace correctness are separate concerns."*

**One related, deeper fact this phase surfaced but did NOT change** (flagging per the standing instruction to report new architectural findings): the kernel's own `KernelEvent::DeleteNode`/`DeleteEdge` handling in `apply_event_ns` (`crates/valori-kernel/src/state/kernel.rs`) has no namespace parameter to check against at all — deletion by id is namespace-blind at the canonical layer itself; safety currently depends entirely on the caller (now correctly enforced at every `valori-node` call site by this phase) never submitting a mismatched id. This is architecturally consistent with how `DeleteNode`/`DeleteEdge` were designed (ids are already globally unique per `KernelState`, unlike e.g. `InsertRecord`, which needs a namespace to know *where* to place new data) — but it does mean the invariant has exactly one enforcement layer (the API), not two. Given every actual caller was just closed in this phase, this is **not** a live gap — it is noted here as context for anyone reasoning about defense-in-depth in a future phase, not as a new problem to fix now.

---

## 5. The Namespace Isolation Matrix — Proven, Not Assumed

For every fixed path, the exact matrix from the brief:

```
Namespace A → Node 1        Namespace B → Node 2

A + Node 1 → success            (200 OK)
A + Node 2 → NOT FOUND          (404)
B + Node 1 → NOT FOUND          (404)
B + Node 2 → success            (200 OK)
```

**Verified empirically, not just by code inspection.** For `get_node` specifically (both standalone and cluster), the fix was temporarily reverted, the test suite re-run to confirm it fails without the fix (`left: 200, right: 404`), then restored and re-confirmed green — proving these are real regression tests, not vacuously-passing ones.

---

## 6. Tests

**`crates/valori-node/tests/api_graph_namespace_isolation.rs`** (standalone, 5 tests): `get_node_matrix`, `node_edges_matrix`, `subgraph_matrix` (all three prove the exact 4-cell matrix above, plus confirm the *correct*-namespace calls return real, non-empty data — not just that the wrong-namespace calls fail), `delete_node_cannot_cross_namespaces` (proves the cross-tenant delete is rejected, the target survives, and the legitimate same-namespace delete still works), `graphrag_has_no_direct_node_id_parameter_to_exploit` (§3's structural-safety claim, made concrete).

**`crates/valori-node/tests/cluster_graph_namespace_isolation.rs`** (cluster, 3 tests): `cluster_get_node_matrix`, `cluster_node_edges_and_subgraph_do_not_leak`, `cluster_delete_node_cannot_cross_namespaces` — the same matrix against a **real, single-node, self-elected Raft cluster** (`shard_count: 1`, reusing the existing `boot_leader()`/`build_cluster_router` integration-test pattern already established in `cluster_namespaces.rs`/`cluster_data_plane.rs`), with `shard_count: 1` deliberately chosen so both test namespaces genuinely share one physical shard — the exact condition (`namespace_id % shard_count` collision) that made the pre-fix cluster bug exploitable in the first place, and the most common real deployment configuration (the default).

8 new tests total, all passing, both confirmed to actually fail without the fix (not just standalone — the cluster `get_node` check was independently reverted and re-confirmed to fail the same way).

---

## 7. Verification

| Check | Result |
|---|---|
| `cargo fmt --check` (workspace) | Clean |
| `cargo check --workspace` | Clean |
| `cargo clippy -p valori-node --all-targets -- -D warnings` | Clean |
| `cargo test -p valori-node` (full suite) | 308/308 passing (300 prior + 8 new) |
| `cargo test -p valori-node --test route_parity` | Passing — the fix touched only handler bodies, not route registration, so no parity drift |
| `cargo test -p valori-node --test api_graph_query --test graph_cascade --test api_graphrag` | All passing, unmodified — confirms the fix did not change behavior for any already-correct call pattern |
| Fix genuinely caught by tests (not vacuous) | Confirmed by temporary revert + re-run, both standalone and cluster (§5) |

No canonical state, snapshot format, event format, BLAKE3 contract, or vector-index (HNSW/IVF/BQ) code was touched — this phase, like G1.1, was scoped entirely to the `valori-node` HTTP/GraphOps layer.

---

## 8. Risks

- **None outstanding for the audited surface.** Every read/delete path that resolves a namespace and looks up a node by caller-supplied id now validates it.
- **The kernel-level `DeleteNode`/`DeleteEdge` namespace-blindness noted in §4** is context for future defense-in-depth work, not a live gap — every current caller is now correctly gated.
- **No new risk was introduced.** The fix is a pure narrowing of existing behavior (previously-succeeding cross-namespace calls now correctly fail); no previously-failing call now succeeds, and no wire/response format changed except for the specific cross-namespace cases that were the bug.

---

## Verdict

**G1.1.1 PASS.**

- Every existing graph read path was audited; the three broken ones (`get_node`, `node_edges`, `subgraph`) are fixed on both standalone and cluster.
- `delete_node` is fixed directly, not left dependent on the shared handler's call ordering.
- GraphRAG and entity extraction were audited and confirmed safe by construction — proven by a test, not just reasoned about.
- The exact A/B × Node1/Node2 matrix passes on both standalone and cluster, with cluster specifically exercising the `shard_count: 1` (multi-namespace-per-shard) condition that made the bug real.
- The fix lives entirely at the kernel/node layer (`valori-node`'s `GraphOps` implementations) — no Cloud/authorization-layer code was added or would have been appropriate, per the explicit instruction that these are separate concerns.
- Full regression suite green, zero unrelated changes.

The graph read API is now namespace-safe. **G1.2 may proceed** on a clean foundation.
