# Valori Kernel Functions

## src/fxp/ops.rs
- `fxp_add(a: FxpScalar, b: FxpScalar) -> FxpScalar`: Adds two fixed-point scalars with saturation.
- `fxp_sub(a: FxpScalar, b: FxpScalar) -> FxpScalar`: Subtracts two fixed-point scalars with saturation.
- `fxp_mul(a: FxpScalar, b: FxpScalar) -> FxpScalar`: Multiplies two fixed-point scalars with saturation.
- `from_f32(f: f32) -> FxpScalar`: Converts f32 to FxpScalar (Test/Std only).
- `to_f32(s: FxpScalar) -> f32`: Converts FxpScalar to f32 (Test/Std only).

## src/types/scalar.rs
- `FxpScalar::ZERO`: Constant for scalar value 0.
- `FxpScalar::ONE`: Constant for scalar value 1.0 (65536).

## src/types/vector.rs
- `FxpVector::new_zeros() -> Self`: Creates a new vector initialized to zeros.
- `FxpVector::as_slice(&self) -> &[FxpScalar]`: Returns the vector data as a slice.
- `FxpVector::as_mut_slice(&mut self) -> &mut [FxpScalar]`: Returns the vector data as a mutable slice.
- `Index<usize>` / `IndexMut<usize>` implementations: Allows `vec[i]` access.

## src/types/id.rs
- `Version::next(&self) -> Self`: Returns the next incremented version.

## src/math/dot.rs
- `fxp_dot(a: &FxpVector<D>, b: &FxpVector<D>) -> FxpScalar`: Computes the dot product of two vectors using i64 accumulation.

## src/math/l2.rs
- `fxp_l2_sq(a: &FxpVector<D>, b: &FxpVector<D>) -> FxpScalar`: Computes the squared L2 distance between two vectors.

## src/storage/record.rs
- `Record::new(id: RecordId, vector: FxpVector<D>) -> Self`: Creates a new Record instance.

## src/storage/pool.rs
- `RecordPool::new() -> Self`: Creates a new empty RecordPool.
- `RecordPool::insert(&mut self, vector: FxpVector<D>) -> Result<RecordId>`: Inserts a vector into the first available slot.
- `RecordPool::delete(&mut self, id: RecordId) -> Result<()>`: Deletes the record at the specified ID.
- `RecordPool::get(&self, id: RecordId) -> Option<&Record<D>>`: Gets a reference to the record if valid.
- `RecordPool::iter(&self) -> impl Iterator`: Iterates over all active records in deterministic order.
- `RecordPool::len(&self) -> usize`: Returns the count of active records.
- `RecordPool::is_full(&self) -> bool`: Returns true if the pool is full.

## src/index/brute_force.rs
- `SearchResult { score: FxpScalar, id: RecordId }`: Struct for search results (strict ordering: score asc, then ID asc).
- `BruteForceIndex::on_insert(&mut self, id, vec)`: Hook called after record insertion.
- `BruteForceIndex::on_delete(&mut self, id)`: Hook called after record deletion.
- `BruteForceIndex::rebuild(&mut self, pool)`: Rebuilds index (if stateful).
- `BruteForceIndex::search(&self, pool, query, results) -> usize`: Linear search filling the provided `SearchResult` slice.
- `BruteForceIndex::search_topk(&self, pool, query) -> [SearchResult; K]`: Helper returning a fixed-size array of top-K results.

## src/graph/node.rs
- `GraphNode::new(id, kind, record) -> Self`: Creates a new GraphNode.

## src/types/enums.rs
- `NodeKind::from_u8(v: u8) -> Option<Self>`: Safely converts u8 to NodeKind.
- `EdgeKind::from_u8(v: u8) -> Option<Self>`: Safely converts u8 to EdgeKind.

## src/graph/edge.rs
- `GraphEdge::new(id, kind, from, to) -> Self`: Creates a new GraphEdge.

## src/graph/pool.rs
- (`NodePool`) `new() -> Self`: Creates a new empty NodePool.
- (`NodePool`) `insert(&mut self, node) -> Result<NodeId>`: Inserts a node.
- (`NodePool`) `get(&self, id) -> Option<&GraphNode>`: Gets a node reference.
- (`NodePool`) `get_mut(&mut self, id) -> Option<&mut GraphNode>`: Gets a mutable node reference.
- (`NodePool`) `delete(&mut self, id) -> Result<()>`: Deletes a node.
- (`NodePool`) `is_allocated(&self, id) -> bool`: Checks if a node ID is currently valid.
- (`NodePool`) `len(&self) -> usize`: Returns count of active nodes.
- (`NodePool`) `is_full(&self) -> bool`: Returns true if node pool is full.
- (`EdgePool`) `new() -> Self`: Creates a new empty EdgePool.
- (`EdgePool`) `insert(&mut self, edge) -> Result<EdgeId>`: Inserts an edge.
- (`EdgePool`) `get(&self, id) -> Option<&GraphEdge>`: Gets an edge reference.
- (`EdgePool`) `get_mut(&mut self, id) -> Option<&mut GraphEdge>`: Gets a mutable edge reference.
- (`EdgePool`) `delete(&mut self, id) -> Result<()>`: Deletes an edge.
- (`EdgePool`) `is_allocated(&self, id) -> bool`: Checks if an edge ID is currently valid.
- (`EdgePool`) `len(&self) -> usize`: Returns count of active edges.
- (`EdgePool`) `is_full(&self) -> bool`: Returns true if edge pool is full.

## src/graph/adjacency.rs
- `add_edge(nodes, edges, kind, from, to) -> Result<EdgeId>`: Creates an edge and links it to the `from` node's adjacency list.
- `OutEdgeIterator::new(edges, start) -> Self`: Creates an iterator starting at a given edge ID.

## src/state/kernel.rs
- `KernelState::new() -> Self`: Creates a new initialized KernelState.
- `KernelState::apply(&mut self, cmd: &Command<D>) -> Result<()>`: Applies a state transition command (borrowed).
- `KernelState::get_record(&self, id) -> Option<&Record>`: Read API for records.
- `KernelState::get_node(&self, id) -> Option<&GraphNode>`: Read API for nodes.
- `KernelState::outgoing_edges(&self, node_id) -> Option<OutEdgeIterator>`: Returns iterator over outgoing edges (if node exists).
- `KernelState::search_l2(&self, query, results) -> usize`: Executes an L2 search against the index.
- `KernelState::check_invariants(&self) -> Result<()>`: Verifies internal consistency of the kernel.

## src/snapshot/encode.rs
- `encode_state(state, buf) -> Result<usize>`: Serializes the kernel state (verifies capacities).

## src/snapshot/decode.rs
- `decode_state(buf) -> Result<KernelState>`: Deserializes the kernel state (verifies capacities).

## src/snapshot/hash.rs
- `hash_state(state) -> u64`: Computes a deterministic hash of the kernel state for verification.

## src/error.rs
- `KernelResult<T>`: Alias for `Result<T, KernelError>`.

# Valori Node Functions (Host Extensions)

## node/src/structure/index.rs
- `VectorIndex`: Trait for pluggable indexing.
    - `snapshot() -> Result<Vec<u8>>`: Serializes index state.
    - `restore(&[u8]) -> Result<()>`: Restores index state.
- `BruteForceIndex`: Default exact-search implementation.

## node/src/structure/hnsw.rs (HNSW Index)
- `HnswIndex::new() -> Self`: Creates a deterministic HNSW index.
- `HnswIndex::insert(&mut self, id, vec)`: Inserts vector into graph.
- `HnswIndex::search(&self, query, k) -> Vec<(id, dist)>`: Approximate nearest neighbor search.
- `HnswIndex::deterministic_level(id) -> usize`: Computes layer level using FNV1a hash (no RNG).
- `HnswIndex::snapshot() -> Result<Vec<u8>>`: Deterministic serialization (sorted maps).

## node/src/persistence.rs
- `SnapshotManager::save(path, kernel, meta, index)`: Atomically saves snapshot (V2 format) with rotation.
- `SnapshotManager::parse(buffer) -> Result<(Meta, Kernel, MetaStore, Index)>`: Validates and parses snapshot blob.

## node/src/metadata.rs
- `MetadataStore::set(id, json)`: Stores arbitrary JSON metadata.
- `MetadataStore::get(id) -> Option<Value>`: Retrieves metadata.

## node/src/engine.rs
- `Engine::insert_record_from_f32(values: &[f32]) -> Result<u32>`: Inserts a record. **Validates** input values are within Q16.16 safe range `[-32768.0, 32767.0]`. Returns error on overflow.
- `Engine::search_l2(query: &[f32], k) -> Result<Vec<(u32, i64)>>`: Searches for k-nearest neighbors. **Validates** query vector range.
- `Engine::snapshot() -> Result<Vec<u8>>`: orchestrates full system snapshot.
- `Engine::restore(data)`: Restores Kernel, Metadata, and Index (rebuilding Index if needed).
