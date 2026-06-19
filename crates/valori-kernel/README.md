# valori-kernel

The deterministic, event-sourced core of Valori. A `no_std`-compatible Rust
library that owns vector storage, graph relationships, vector indexing, and
multi-tenant namespace isolation.

---

## Architecture

### Event sourcing (`event/`)

All state mutations flow through **`KernelEvent`**. Replaying the same event
sequence always produces the same BLAKE3 state hash — on every architecture.

| Event | Description |
|---|---|
| `InsertRecord` | Add a vector with an explicit ID (standalone mode). |
| `AutoInsertRecord` | Add a vector; ID assigned at apply time (cluster mode). |
| `DeleteRecord` | Soft-delete a record (tombstone). |
| `CreateNode` / `CreateEdge` | Explicit-ID graph mutations (standalone). |
| `AutoCreateNode` | Create a graph node; ID assigned at apply time (cluster mode). |
| `AutoCreateEdge` | Create a graph edge; ID assigned at apply time (cluster mode). |
| `CreateNamespace` | Register a new tenant namespace (up to 1 024 namespaces). |
| `DropNamespace` | Remove a namespace and all of its records. |

`AutoCreateNode` and `AutoCreateEdge` mirror the `AutoInsertRecord` pattern:
every Raft replica calls `next_node_id()` / `next_edge_id()` in the same
log-ordered sequence and converges to identical IDs without coordination.

```rust
// Cluster-mode: emit Auto events; the ID is determined at apply time.
let event = KernelEvent::AutoCreateNode {
    kind: NodeKind::Document,
    record: Some(record_id),
};
raft_committer.commit(event).await?;

// Standalone mode: caller picks the ID explicitly.
let event = KernelEvent::CreateNode {
    node_id: NodeId(42),
    kind: NodeKind::Document,
    record: None,
};
kernel.apply_event(event)?;
```

### Fixed-point math (`fxp/`)

All vector arithmetic uses **Q16.16 fixed-point** (`FxpScalar`) so results are
bit-identical across x86, ARM, and WASM.

### State machine (`state/`)

`KernelState` is the in-memory database.

| Component | Description |
|---|---|
| `RecordPool` | Dense vector storage with tombstone slots. |
| `NodePool` / `EdgePool` | Adjacency-list graph. |
| `BruteForceIndex` / HNSW | Pluggable approximate-nearest-neighbour index. |
| `namespace_record_heads` | Per-namespace intrusive linked-list heads (1 024 slots). |
| `namespace_node_heads` | Per-namespace intrusive node linked-list heads. |

---

## Multi-tenant namespaces

Records and nodes belong to a `NamespaceId(u16)`. `DEFAULT_NS = 0` is the
always-present default tenant.

Each namespace maintains an **intrusive doubly-linked list** threaded through
the record pool: `Record.next_in_ns` / `Record.prev_in_ns`. This makes
per-namespace iteration O(N\_tenant) without a separate index.

```rust
// Namespace-scoped search — touches only tenant-2 records.
let mut results = vec![SearchResult::default(); k];
let found = kernel_state.search_l2_ns(&query_vec, &mut results, namespace_id);

// Global search across the default namespace (backward-compatible).
let found = kernel_state.search_l2(&query_vec, &mut results, None);
```

Non-default namespace records are **never inserted** into the global
BruteForce/HNSW index. Isolation is enforced at three sites: the event-commit
path, the WAL replay path, and `build_index()` (post-snapshot restore).

---

## Key API

### `ValoriKernel`

```rust
// The only way to mutate state.
kernel.apply_event(event: KernelEvent) -> Result<(), KernelError>;

// Read-only index query (default namespace).
kernel.search(query: &FxpVector, k: usize) -> Vec<SearchResult>;

// Serialize entire state to a binary blob (VAL1 frame, V6 format).
kernel.snapshot() -> Result<Vec<u8>, SnapshotError>;

// Replace state from a binary blob.
kernel.restore(data: &[u8]) -> Result<(), SnapshotError>;
```

### Snapshot format (V6)

V6 snapshots include per-record namespace metadata and the 2 × 1 024 × 4 = 8 KB
namespace head arrays. The NSRG (namespace registry) section is appended after
the index payload and is backward-compatible: older readers that lack NSRG
support simply ignore the trailing bytes.

```
[VAL1 header][records+flags][nodes][edges][index][namespace heads: 8 KB][NSRG: 4-byte len + JSON]
```

---

## Invariants

1. **No floating point** in core logic. Use `FxpScalar`; never `f32`/`f64`.
2. **No system time**. Timestamps must arrive via events.
3. **No randomness**. Use a deterministic PRNG seeded from the event log.
4. **`DEFAULT_NS` (0) is undeletable**. `DropNamespace` on namespace 0 returns
   `KernelError::InvalidInput`.
5. **`MAX_NAMESPACES = 1 024`**. Attempting to create a 1 025th namespace
   returns an error.

---

## Testing

```bash
cargo test -p valori-kernel
```

Key test suites:

| Suite | What it covers |
|---|---|
| `tests/format.rs` | VAL1 frame encode/decode, foreign-format rejection, V6 buffer sizing. |
| `tests/snapshot_roundtrip.rs` | Full state round-trip through `snapshot()` → `restore()`. |
| `tests/namespace.rs` | Per-namespace insert, scoped search, drop, and linked-list integrity. |
| `tests/events.rs` | Every `KernelEvent` variant serializes and deserializes correctly. |
