# Valori Kernel: Module Analysis - Knowledge Graph Core

The third layer of the `valori-kernel` architecture is the **Knowledge Graph**. While the `storage` module handles pure numerical vectors, the `graph` module wraps them in logical relationships. This allows semantic spaces to be deeply interconnected (e.g., tracking which Document generated which Embeddings, or which Agent authored a Document).

True to Valori's core constraints, the Graph cannot use standard Rust `Box` or `Rc` pointers, nor can it use `HashMap` for adjacency matrices, as these ruin determinism. Instead, it builds an adjacency list using strictly indexed static pools.

---

## 1. Graph Entities: Nodes & Edges

**Location**: `src/graph/node.rs` and `src/graph/edge.rs`

### `GraphNode`
```rust
pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub record: Option<RecordId>,
    pub first_out_edge: Option<EdgeId>,
}
```
- **`kind`**: An enum (like `Document`, `Chunk`, `Agent`) defining what the node represents.
- **`record`**: A node *optionally* wraps a vector by pointing to a `RecordId` in the `RecordPool`. Not all nodes have vectors (e.g., an abstract concept or user node might not have an embedding).
- **`first_out_edge`**: The critical field for deterministic graph traversal. This points to the physical array index (`EdgeId`) of the first outgoing edge from this node.

### `GraphEdge`
```rust
pub struct GraphEdge {
    pub id: EdgeId,
    pub kind: EdgeKind,
    pub from: NodeId,
    pub to: NodeId,
    pub next_out: Option<EdgeId>,
}
```
- **`kind`**: An enum defining the relationship (e.g., `ParentOf`, `AuthoredBy`).
- **`from` & `to`**: Directional pointers linking `NodeId` to `NodeId`.
- **`next_out`**: The key to the adjacency list. It forms a linked list pointing to the next edge originating from the *same* `from` node.

---

## 2. Graph Pools (Static Allocation)

**Location**: `src/graph/pool.rs`

Just like the `RecordPool`, the Graph uses flat `alloc::vec::Vec<Option<T>>` structures for physical storage.

### `NodePool` and `EdgePool`
```rust
pub struct NodePool {
    pub(crate) nodes: alloc::vec::Vec<Option<GraphNode>>,
}

pub struct EdgePool {
    pub(crate) edges: alloc::vec::Vec<Option<GraphEdge>>,
}
```
- **`insert`**: Appends the new node/edge to the end of the `Vec` and binds its internal ID to its array index.
- **`delete`**: Turns the slot into a `None` tombstone.
- **Why Tombstoning?**: If a node is deleted, its `NodeId` must never be given to another node in a way that shifts the array. If array shifting occurred, all existing `EdgeId` and `NodeId` references across the graph would silently point to the wrong memory slots.

---

## 3. Adjacency Logic & Traversal

**Location**: `src/graph/adjacency.rs`

Because the engine avoids complex graph databases, it manually manages a **Singly Linked Adjacency List** using array indices.

### `add_edge`
When a new edge is created linking `Node A` to `Node B`:
1. It verifies `Node A` and `Node B` actually exist in the `NodePool`.
2. It looks at `Node A`'s `first_out_edge` and temporarily holds it (`head`).
3. It sets the new Edge's `next_out` to that `head`.
4. It persists the new Edge into the `EdgePool`.
5. It mutates `Node A`, pointing its `first_out_edge` to the newly created Edge.
  
*This is standard linked-list insertion (insert at head) but executed entirely over `Vec` indices rather than memory addresses.*

### `OutEdgeIterator`
```rust
pub struct OutEdgeIterator<'a> {
    edges: &'a EdgePool,
    current: Option<EdgeId>,
}
```
To query the graph deterministically (e.g., "Find all chunks belonging to Document A"):
1. The iterator takes the `first_out_edge` of Document A.
2. `next()` yields the edge from the `EdgePool`.
3. It advances its state by reading the edge's `next_out` field.
4. **Determinism Guarantee**: Because edges are inserted linearly, iterating this linked list yields neighbors in the exact reverse order they were inserted. This iteration order is mathematically rigid and bit-exact across platforms.

---

### Summary of Module Edge Cases
1. **Dangling Edges**: Currently, `add_edge` safely errors out if either node doesn't exist (`Err(KernelError::NotFound)`). However, if a Node is deleted *after* edges are created, the edges technically still hold its `NodeId`. Handling cascading deletes requires the upper state machine (`KernelState`) to clean up adjacent edges when `DeleteNode` is executed.
2. **Borrow Checking Safeties**: The `add_edge` function carefully accesses `nodes` and `edges` separately to satisfy Rust's borrow checker without needing `RefCell` or runtime locks, keeping the kernel incredibly fast and simple.
