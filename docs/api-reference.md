# Valori Server API Reference

The `valori-node` server exposes the kernel over HTTP, allowing it to be used as a backend service. This server is written in **Rust** (not Node.js) but acts as a network "node" in a distributed system.

**Base URL**: `http://localhost:3000` (Default)

## Endpoints

### 1. Memory Protocol v1 (High Level)
Use these endpoints for full "Protocol" behavior (Atomic Insert + Graph Linking).

#### `POST /v1/memory/upsert_vector`
Inserts a vector, creates a chunk node, and optionally links it to a document.

*   **Request Body**:
    ```json
    {
      "vector": [0.1, ...],
      "attach_to_document_node": 123  // Optional
    }
    ```
*   **Response**:
    ```json
    {
      "memory_id": "rec:10",
      "record_id": 10,
      "document_node_id": 123,
      "chunk_node_id": 200
    }
    ```

#### `POST /v1/memory/search_vector`
Search for nearest neighbors.

*   **Request Body**:
    ```json
    {
      "query_vector": [0.1, ...],
      "k": 5
    }
    ```
*   **Response**:
    ```json
    {
      "results": [
        { "memory_id": "rec:5", "record_id": 5, "score": 1000 }
      ]
    }
    ```

### 2. Primitive Operations (Low Level)
Direct access to kernel primitives.

#### `POST /records`
Insert a new vector record.
*   **Body**: `{"values": [...]}`
*   **Response**: `{"id": 123}`

#### `POST /search`
Primitive search.
*   **Body**: `{"query": [...], "k": 5}`
*   **Response**: `{"results": [{"id": 123, "score": 4500}]}`

### 3. State Management

#### `POST /snapshot`
Download the full state of the kernel.
*   **Response**: Binary body (application/octet-stream).

#### `POST /restore`
Restore state from a snapshot file.
*   **Body**: Binary content.
*   **Headers**: `Content-Type: application/octet-stream`
