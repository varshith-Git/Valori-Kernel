# Valori Kernel: Module Analysis - Storage & Indexing Core

This is the second module-by-module breakdown. Having established the Fixed-Point (FXP) arithmetic layer, we now look at how the kernel actually stores these vectors and how it retrieves them. 

Since `valori-kernel` is a strictly `no_std`, deterministic environment, it relies on static flat buffers and tightly packed arrays rather than dynamic, non-deterministic B-Trees or SQL databases.

---

## 1. Storage: The Record Structure

**Location**: `src/storage/record.rs`

The fundamental data unit in Valori is the `Record`. 

### `Record`
```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub id: RecordId,
    pub vector: FxpVector,
    pub metadata: Option<alloc::vec::Vec<u8>>,
    pub tag: u64,
    pub flags: u8,
}
```
- **`id`**: A monotonic, zero-indexed `RecordId`.
- **`vector`**: The `FxpVector` containing the quantized embeddings.
- **`metadata`**: An optional binary blob. Because this is binary, the kernel itself doesn't care about JSON parsing. That is strictly the responsibility of the Python client / HTTP layer.
- **`tag`**: An integer mapping for collections. Instead of creating physical tables, records are logically grouped by tags.
- **`flags`**: Reserved byte for tombstoning or state signaling (e.g., deleted, hidden).

---

## 2. Storage: The Record Pool

**Location**: `src/storage/pool.rs`

### `RecordPool`
```rust
pub struct RecordPool {
    pub(crate) records: alloc::vec::Vec<Option<Record>>,
}
```
The pool handles the physical memory layout of records.
- **Why `Option<Record>`?** This guarantees that a record's physical address (its index in the `Vec`) *never changes*. This is a crucial requirement for the cryptographic hashing phase. If we used `Vec::remove`, the elements would shift, and the global state hash would become unpredictable.

### Core Functions:
- **`insert(&mut self, ...)`**: Appends a new `Some(Record)` to the back of the vector and uses its array index as the `RecordId`.
- **`delete(&mut self, id: RecordId)`**: Takes the `RecordId`, casts it to `usize`, and sets `self.records[idx] = None`. This creates a tombstone.
- **`iter(&self)`**: Uses `filter_map(|opt| opt.as_ref())` to return an iterator over only the active records.
- **Determinism Guarantee**: Iterating through a `Vec` is 100% deterministic, unlike a `HashMap` where key iteration order is randomized by default in Rust.

---

## 3. Indexing: The Interfaces

**Location**: `src/index/mod.rs`

To search the `RecordPool`, the kernel abstracts indexing behind a unified trait.

### `SearchResult`
```rust
pub struct SearchResult {
    pub score: FxpScalar,
    pub id: RecordId,
}
```
- **Sorting Logic**: Implements custom `Ord` and `PartialOrd`. It sorts by `score` first, and if scores are exactly identical, it tie-breaks using `id`. Tie-breaking by `id` is a classic mechanism to ensure that searches are perfectly reproducible across instances.

### `VectorIndex` Trait
```rust
pub trait VectorIndex {
    fn search(&self, pool: &RecordPool, query: &FxpVector, results: &mut [SearchResult], filter: Option<u64>) -> usize;
}
```
Any indexing strategy (like IVF or HNSW at the Node layer) must conform to this signature, receiving a target buffer `results` to populate.

---

## 4. Indexing: The Brute-Force Engine

**Location**: `src/index/brute_force.rs`

While the Node layer (`node/src/engine.rs`) provides advanced HNSW and IVF indices, the core Kernel implements a strictly deterministic, stateless flat search for perfect recall.

### `BruteForceIndex`
- **Stateless Design**: It contains no internal data. When a search is executed, it borrows the entire `RecordPool`.
- **Working Mechanism (`search`)**:
  1. **Initialization**: Populates the target `results` slice with worst-case defaults (`score = i32::MAX`).
  2. **Scan Phase**: Iterates through the pool `for record in pool.iter()`.
  3. **Tag Filtering**: If a `filter` is requested, it ignores records where `record.tag != req_tag`.
  4. **Distance Calculation**: Computes `dist_sq = fxp_l2_sq(&record.vector, query)`.
  5. **Insertion Sort**: Since $K$ (the number of requested results) is usually small (e.g., 5 or 10), it uses a simple insertion sort logic to slide the new candidate into the `results` array, maintaining a sorted list of the closest matches.

### Summary of Module Edge Cases
1. **Physical Stability**: Because records are never shifted upon deletion, pointer references (like `Node -> Record` in the Graph module) will never become misaligned. 
2. **Stable Tie-Breaking**: When two vectors are exactly equidistant from a query, the tie is broken deterministically by their insertion index (`RecordId`).
3. **Array Bounds**: Insertion sort array boundaries `k` are strictly guarded against out-of-bounds panics during search candidate shuffling.
