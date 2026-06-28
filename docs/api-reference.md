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
*   **Error responses**:
    | Status | Condition |
    |---|---|
    | `400 Bad Request` | Missing or malformed fields |
    | `401 Unauthorized` | Auth token required but not provided |
    | `507 Insufficient Storage` | `VALORI_MAX_RECORDS` (or `MAX_NODES` / `MAX_EDGES`) limit reached |

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
*   **Errors**: `507 Insufficient Storage` when `VALORI_MAX_RECORDS` is reached.

#### `POST /search`
Primitive search.
*   **Body**: `{"query": [...], "k": 5}`
*   **Response**: `{"results": [{"id": 123, "score": 4500}]}`

### 3. Tree-RAG (Hierarchical retrieval with receipts)

Navigate a document's table-of-contents to the *right section* instead of
returning vector-similar text. Deterministic (no embeddings, no LLM). All three
handlers are stateless and behave identically in standalone and cluster mode.
This is **separate from** `/search` — use whichever fits the query.

#### `POST /v1/tree/build`
Parse a structured/markdown document into a navigable tree.
*   **Body**: `{ "text": "<document>", "doc_name": "handbook" }` (`doc_name` optional)
*   **Response**: `{ "doc_name", "node_count", "structure_map": [...], "tree": {...} }`
    The `tree` object is passed back into `query`/`verify` (the caller holds it).

#### `POST /v1/tree/query`
Navigate the tree and answer with citations + a replayable receipt.
*   **Body**: `{ "tree": {...}, "query": "how many sick days?", "k": 2, "prev_hash": "<optional prior receipt_hash>" }`
*   **Response**: `{ "answer", "citations": [{ "node_id", "title", "breadcrumb", "lines" }], "visited_node_ids", "reasoning", "receipt": {...} }`
    Pass a receipt's `receipt_hash` as the next call's `prev_hash` to chain receipts.

#### `POST /v1/tree/verify`
Replay a receipt against the tree to prove the stored content was not altered.
*   **Body**: `{ "tree": {...}, "receipt": {...} }`
*   **Response**: `{ "valid": true|false }` — `false` means the cited section changed after retrieval.

### 4. State Management

#### `POST /snapshot`
Download the full state of the kernel.
*   **Response**: Binary body (application/octet-stream).

#### `POST /restore`
Restore state from a snapshot file.
*   **Body**: Binary content.
*   **Headers**: `Content-Type: application/octet-stream`
