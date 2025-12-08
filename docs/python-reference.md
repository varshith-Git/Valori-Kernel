# Valori Kernel Python Reference

The `valori_ffi` package provides direct bindings to the Rust kernel. It is designed for high-performance, single-process applications.

## Class: `PyKernel`

The main entry point for the kernel. Usage requires instantiating this class.

```python
from valori_ffi import PyKernel

kernel = PyKernel()
```

### Methods

#### `insert(vector: List[float]) -> int`
Inserts a new vector record into the kernel.

*   **Arguments**:
    *   `vector`: A list of floats. Must match the kernel's configured dimension (default: 16).
*   **Returns**: `int` (The newly assigned Record ID).
*   **Raises**: `ValueError` if dimension mismatch. `RuntimeError` if capacity exceeded.

#### `search(query: List[float], k: int) -> List[Tuple[int, int]]`
Performs a deterministic L2 optimized search.

*   **Arguments**:
    *   `query`: Search vector (list of floats).
    *   `k`: Number of nearest neighbors to return.
*   **Returns**: A list of tuples `(record_id, score)`.
    *   `score` is the raw fixed-point squared distance (lower is closer).

#### `create_node(kind: int, record_id: Optional[int] = None) -> int`
Creates a new node in the knowledge graph.

*   **Arguments**:
    *   `kind`: Integer representing the node type (User-defined semantic enum).
    *   `record_id`: (Optional) ID of a vector record to associate with this node.
*   **Returns**: `int` (The new Node ID).

#### `create_edge(from_id: int, to_id: int, kind: int) -> int`
Creates a directed edge between two nodes.

*   **Arguments**:
    *   `from_id`: Source Node ID.
    *   `to_id`: Target Node ID.
    *   `kind`: Integer representing the edge relationship type.
*   **Returns**: `int` (The new Edge ID).

#### `snapshot() -> bytes`
Serializes the entire kernel state (vectors + graph + index) into a byte array.
*   **Returns**: `bytes` object containing the deterministic state.

#### `restore(data: bytes) -> None`
Restores the kernel state from a snapshot. This completely overwrites the current state.
*   **Arguments**:
    *   `data`: Byte array from a previous snapshot.
