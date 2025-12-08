# Valori Kernel Node.js API Reference

The `valori-node` server exposes the kernel over HTTP, allowing it to be used as a backend service.

**Base URL**: `http://localhost:3000` (Default)

## Endpoints

### 1. Vector Operations

#### `POST /records`
Insert a new vector record.

*   **Request Body**:
    ```json
    {
      "values": [0.1, 0.5, ... 16 floats ...]
    }
    ```
*   **Response**:
    ```json
    {
      "id": 123
    }
    ```

#### `POST /search`
Search for similar vectors.

*   **Request Body**:
    ```json
    {
      "query": [0.1, 0.5, ...],
      "k": 5
    }
    ```
*   **Response**:
    ```json
    {
      "results": [
        { "id": 123, "score": 4500 }
      ]
    }
    ```

### 2. Knowledge Graph Operations

#### `POST /graph/node`
Create a graph node.

*   **Request Body**:
    ```json
    {
      "kind": 1,
      "record_id": 123  // Optional
    }
    ```
*   **Response**:
    ```json
    {
      "node_id": 50
    }
    ```
    *   `kind` is a `u8` integer mapping to your domain entities (e.g., 1=User, 2=Doc).

#### `POST /graph/edge`
Create a relationship between nodes.

*   **Request Body**:
    ```json
    {
      "from": 50,
      "to": 51,
      "kind": 2
    }
    ```
*   **Response**:
    ```json
    {
      "edge_id": 10
    }
    ```

### 3. State Management

#### `POST /snapshot`
Download the full state of the kernel.

*   **Response**: Binary body containing the snapshot data.
*   **Content-Type**: `application/octet-stream`

#### `POST /restore`
Restore state from a snapshot file.

*   **Body**: Binary content of the snapshot.
*   **Result**: 200 OK on success.
