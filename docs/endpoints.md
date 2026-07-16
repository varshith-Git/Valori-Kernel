# Valori Kernel — Complete Server Endpoint Reference (`endpoints.md`)

This document is the definitive catalog of all **76 HTTP API endpoint definitions** across the Valori backend (`valori-node` and `cluster_server`). Below the summary matrix, you will find detailed usage guides, request payloads, and response schemas for **every single endpoint**.

---

## 📋 Comprehensive Endpoint Matrix

| Endpoint | Method | Used in UI? | Primary Audience / Purpose |
|---|:---:|:---:|---|
| **1. System & Cluster** | | | |
| `/health` | `GET` | ✅ **Yes** | Liveness probe and basic storage/memory health stats |
| `/metrics` | `GET` | ✅ **Yes** | Prometheus-compatible performance and latency metrics |
| `/v1/version` | `GET` | ❌ No | Server version, Git SHA, and build features |
| `/v1/cluster/status` | `GET` | ✅ **Yes** | Raft consensus status, term, leader ID, and node lag |
| `/v1/cluster/health` | `GET` | ❌ No | Lightweight cluster heartbeat check |
| `/v1/cluster/read-index` | `GET` | ❌ No | Linearizable read-index verification for strict Raft consistency |
| `/v1/cluster/role` | `GET` | ❌ No | Returns current node role (`leader`, `follower`, `candidate`) |
| `/v1/cluster/add-node` | `POST` | ❌ No | Dynamic Raft cluster reconfiguration (add peer) |
| `/v1/cluster/remove-node` | `POST` | ❌ No | Dynamic Raft cluster reconfiguration (remove peer) |
| `/v1/cluster/snapshot` | `POST` | ❌ No | Trigger immediate Raft log compaction and snapshotting |
| **2. Namespaces / Collections** | | | |
| `/v1/namespaces` | `GET` | ✅ **Yes** | List all active vector collections and their record counts |
| `/v1/namespaces` | `POST` | ✅ **Yes** | Create a new isolated namespace / collection |
| `/v1/namespaces/:name` | `DELETE` | ✅ **Yes** | Permanently drop a collection and its associated vectors/graph |
| **3. Vectors & Ingestion** | | | |
| `/v1/search` | `POST` | ✅ **Yes** | Vector similarity search (L2 / Cosine / Dot) with optional filtering |
| `/v1/vectors/batch-insert` | `POST` | ✅ **Yes** | High-throughput batch insertion of quantized Q16.16 vectors |
| `/v1/records` | `POST` | ❌ No | Single-record vector insert (SDK convenience) |
| `/v1/delete` | `POST` | ✅ **Yes** | Hard delete a record by ID |
| `/v1/soft-delete` | `POST` | ❌ No | Cluster tombstone soft deletion across Raft followers |
| `/v1/ingest` | `POST` | ✅ **Yes** | Ingest raw text chunks with auto-created graph document linking |
| `/v1/ingest/update` | `POST` | ✅ **Yes** | Update/replace an existing ingested document and its chunks |
| `/v1/ingest/document` | `POST` | ❌ No | Server-side file ingestion (for CLI/SDK when files reside on server) |
| `/v1/ingest/extract-entities` | `POST` | ✅ **Yes** | NLP / LLM extraction of graph entities and relationships from text |
| **4. Knowledge Graph** | | | |
| `/v1/graph/node` | `POST` | ✅ **Yes** | Create a new entity node (Document, Chunk, Concept, Agent, etc.) |
| `/v1/graph/node/:id` | `GET` | ✅ **Yes** | Get node details and associated metadata |
| `/v1/graph/node/:id` | `DELETE` | ✅ **Yes** | Delete a graph node and clean up dangling edges |
| `/v1/graph/nodes` | `GET` | ✅ **Yes** | List nodes in a collection with optional pagination |
| `/v1/graph/edge` | `POST` | ✅ **Yes** | Create a directed edge between two nodes (`HasChunk`, `Mentions`, etc.) |
| `/v1/graph/edges/:id` | `GET` | ✅ **Yes** | Retrieve all incoming and outgoing edges for a node |
| `/v1/graph/subgraph` | `GET` | ❌ No | Extract an N-hop neighborhood subgraph around a focal node |
| **5. Community Detection** | | | |
| `/v1/community/detect` | `POST` | ✅ **Yes** | Run Leiden/Louvain community detection over the knowledge graph |
| `/v1/community/overview` | `GET` | ✅ **Yes** | Retrieve hierarchical community summaries and cluster sizes |
| `/v1/community/search` | `POST` | ❌ No | Search community summaries using vector similarity |
| **6. Agentic Memory Protocol** | | | |
| `/v1/memory/meta/set` | `POST` | ✅ **Yes** | Attach arbitrary JSON metadata or LLM context sentences to a target ID |
| `/v1/memory/meta/get` | `GET` | ✅ **Yes** | Retrieve metadata for a target ID (`record:123`, `node:45`) |
| `/v1/memory/contradict` | `POST` | ✅ **Yes** | Scan and flag semantic contradictions between stored memory claims |
| `/v1/memory/upsert` | `POST` | ❌ No | High-level agent memory upsert (creates vector + chunk node + link) |
| `/v1/memory/upsert_vector` | `POST` | ❌ No | Alias for `/v1/memory/upsert` |
| `/v1/memory/search` | `POST` | ❌ No | High-level memory search returning graph context + vector scores |
| `/v1/memory/search_vector` | `POST` | ❌ No | Alias for `/v1/memory/search` |
| `/v1/memory/consolidate` | `POST` | ❌ No | Trigger background agent memory decay, deduplication, and consolidation |
| `/v1/graphrag` | `POST` | ❌ No | Execute GraphRAG traversal (vector search + N-hop graph expansion) |
| **7. Proof, Audit & Operations** | | | |
| `/v1/timeline` | `GET` | ✅ **Yes** | Chronological audit trail of all mutations in the namespace |
| `/v1/operations` | `GET` | ✅ **Yes** | List background operations (ingest jobs, rebuilds, community detections) |
| `/v1/operations/:id` | `GET` | ✅ **Yes** | Poll status and progress percentage of a specific operation |
| `/v1/operations/:id/execution` | `GET` | ✅ **Yes** | Retrieve detailed execution logs and timing breakdowns for an operation |
| `/v1/proof/state` | `GET` | ✅ **Yes** | Get global BLAKE3 state hash, record counts, and merkle roots |
| `/v1/cluster/proof` | `GET` | ❌ No | Verify that all distributed Raft shards have converged on the exact same BLAKE3 state hash |
| `/v1/proof/receipt` | `GET` | ✅ **Yes** | Get the latest cryptographic tamper-evident receipt |
| `/v1/proof/receipt/:id` | `GET` | ✅ **Yes** | Retrieve a specific historical receipt by transaction/event ID |
| `/v1/proof/event-log` | `GET` | ❌ No | Download the immutable binary event log stream for offline verification |
| **8. Tree-RAG (Hierarchical TOC Retrieval)** | | | |
| `/v1/tree/build` | `POST` | ❌ No | Parse document text into a deterministic Table-of-Contents tree index |
| `/v1/tree/query` | `POST` | ❌ No | Navigate the tree index to answer questions with exact line citations |
| `/v1/tree/hybrid` | `POST` | ❌ No | Single-call retrieval fusing tree navigation with vector similarity scores |
| `/v1/tree/verify` | `POST` | ❌ No | Replay a retrieval receipt against a tree to prove zero tampering |
| `/v1/tree/chain-verify` | `POST` | ❌ No | Verify an ordered sequence of receipts forms an unbroken audit chain |
| **9. Snapshots, Storage & WAL Replication** | | | |
| `/v1/storage/snapshots` | `GET` | ✅ **Yes** | List remote snapshots available in S3 / local disk backup storage |
| `/v1/storage/snapshots/upload`| `POST` | ✅ **Yes** | Upload/create a new kernel snapshot to storage |
| `/v1/storage/snapshots/restore`| `POST`| ✅ **Yes** | Restore kernel state from a specific storage snapshot |
| `/v1/snapshot/download` | `GET` | ❌ No | Direct binary download of current memory state |
| `/v1/snapshot/upload` | `POST` | ❌ No | Direct binary restore of memory state |
| `/v1/snapshot/save` | `POST` | ❌ No | Save snapshot to configured server disk path |
| `/v1/snapshot/restore` | `POST` | ❌ No | Restore snapshot from configured server disk path |
| `/v1/storage/wal` | `GET` | ❌ No | List archived Write-Ahead Log segments in storage |
| `/v1/storage/wal/archive` | `POST` | ❌ No | Archive current WAL segment to cold storage |
| `/v1/replication/wal` | `GET` | ❌ No | Stream live WAL bytes to follower nodes |
| `/v1/replication/events` | `GET` | ❌ No | Stream committed event records for cross-node replication |
| `/v1/replication/state` | `GET` | ❌ No | Get replication offset and synchronisation status |
| **10. Security, Crypto & Index Administration** | | | |
| `/v1/keys` | `POST` | ❌ No | Create a new API authentication token / key |
| `/v1/keys` | `GET` | ❌ No | List active API keys |
| `/v1/keys/:id` | `DELETE` | ❌ No | Revoke an API key |
| `/v1/records/encrypted` | `POST` | ❌ No | Insert payload encrypted with envelope encryption |
| `/v1/crypto/shred/:key_id` | `DELETE` | ❌ No | Crypto-shred an encryption key (instant GDPR right-to-erasure) |
| `/v1/crypto/status/:key_id`| `GET` | ❌ No | Check encryption key rotation and shred status |
| `/v1/index/config` | `GET` | ❌ No | Get current vector index configuration (HNSW / IVF / BQ / BruteForce) |
| `/v1/index/rebuild` | `POST` | ❌ No | Force an asynchronous background re-indexing and quantization job |
| `/v1/shard/routing` | `GET` | ❌ No | Get consistent-hashing shard routing table for multinode setups |

---

## 🛠 Complete Usage Guide for All 76 Endpoints

---

### 1. System & Cluster

#### `GET /health`
Liveness probe checking kernel storage pool allocation, memory utilization, and uptime.
```json
// Response
{
  "status": "ok",
  "uptime_seconds": 3600,
  "memory_used_bytes": 1048576,
  "records_count": 1500
}
```

#### `GET /metrics`
Returns Prometheus-formatted metrics for monitoring systems.
```text
# HELP valori_requests_total Total HTTP requests
# TYPE valori_requests_total counter
valori_requests_total{method="POST",endpoint="/v1/search"} 4210
valori_kernel_search_latency_ms{quantile="0.99"} 1.24
```

#### `GET /v1/version`
Returns server build metadata, Git commit SHA, and enabled hardware acceleration features.
```json
// Response
{
  "version": "1.4.0",
  "git_sha": "a8f92b1",
  "features": ["neon", "simd", "no_std_kernel"]
}
```

#### `GET /v1/cluster/status`
Returns Raft consensus group status, current leadership, and term number.
```json
// Response
{
  "node_id": 1,
  "role": "leader",
  "term": 42,
  "leader_id": 1,
  "commit_index": 889102
}
```

#### `GET /v1/cluster/health`
Lightweight cluster health check verifying network peer connectivity.
```json
// Response
{
  "cluster_healthy": true,
  "connected_peers": 2,
  "total_peers": 3
}
```

#### `GET /v1/cluster/read-index`
Verifies the linearizable Raft read-index to ensure stale reads do not occur from follower nodes.
```json
// Response
{
  "read_index": 889102,
  "safe_to_read": true
}
```

#### `GET /v1/cluster/role`
Returns only the string role of the current node.
```json
// Response
{
  "role": "leader"
}
```

#### `POST /v1/cluster/add-node`
Dynamically adds a new peer node to the Raft consensus group.
```json
// Request Payload
{
  "node_id": 4,
  "address": "10.0.0.4:3001"
}

// Response
{
  "success": true,
  "membership_version": 5
}
```

#### `POST /v1/cluster/remove-node`
Removes an existing peer node from the Raft consensus group.
```json
// Request Payload
{
  "node_id": 4
}

// Response
{
  "success": true,
  "membership_version": 6
}
```

#### `POST /v1/cluster/snapshot`
Forces an immediate Raft log compaction and snapshot across consensus peers.
```json
// Response
{
  "snapshot_index": 889100,
  "reclaimed_bytes": 52428800
}
```

---

### 2. Namespaces / Collections

#### `GET /v1/namespaces`
Lists all active vector collections, their dimensions, and record counts.
```json
// Response
{
  "namespaces": [
    { "name": "default", "dimension": 384, "record_count": 1200 },
    { "name": "finance_docs", "dimension": 1536, "record_count": 8500 }
  ]
}
```

#### `POST /v1/namespaces`
Creates a new isolated namespace / collection.
```json
// Request Payload
{
  "name": "legal_contracts",
  "dimension": 768,
  "metric": "l2"  // "l2", "cosine", or "dot"
}

// Response
{
  "created": true,
  "name": "legal_contracts"
}
```

#### `DELETE /v1/namespaces/:name`
Permanently drops a namespace and deletes all its vectors and graph nodes.
```json
// Response
{
  "deleted": true,
  "name": "legal_contracts",
  "records_removed": 8500
}
```

---

### 3. Vectors & Ingestion

#### `POST /v1/search`
Performs K-nearest neighbor vector similarity search.
```json
// Request Payload
{
  "query_vector": [0.012, -0.045, 0.112],
  "k": 5,
  "collection": "default",
  "filter_tag": 1024
}

// Response
{
  "results": [
    { "record_id": 42, "score": 8, "metadata": { "text": "Extracted text snippet..." } }
  ]
}
```

#### `POST /v1/vectors/batch-insert`
High-throughput batch insertion of quantized vectors.
```json
// Request Payload
{
  "collection": "default",
  "records": [
    { "id": 1, "vector": [0.1, 0.2, 0.3], "tag": 0 },
    { "id": 2, "vector": [-0.5, 0.8, 0.1], "tag": 0 }
  ]
}

// Response
{
  "inserted": 2,
  "collection": "default"
}
```

#### `POST /v1/records`
Single-record vector insert (SDK convenience endpoint).
```json
// Request Payload
{
  "collection": "default",
  "id": 100,
  "vector": [0.12, 0.44, -0.91],
  "tag": 0
}

// Response
{
  "id": 100,
  "status": "inserted"
}
```

#### `POST /v1/delete`
Hard deletes a record and invalidates its index location.
```json
// Request Payload
{
  "id": 100,
  "collection": "default"
}

// Response
{
  "deleted": true,
  "id": 100
}
```

#### `POST /v1/soft-delete` (Cluster Mode)
Tombstones a vector record without reclaiming physical storage until compaction.
```json
// Request Payload
{
  "id": 100,
  "collection": "default"
}

// Response
{
  "soft_deleted": true,
  "id": 100
}
```

#### `POST /v1/ingest`
Ingests text chunks, creates embeddings, inserts vector records, and links them to a graph Document node.
```json
// Request Payload
{
  "collection": "default",
  "document_title": "Q3 Financial Report",
  "chunks": [
    { "text": "Revenue increased by 14% year over year.", "chunk_index": 0 },
    { "text": "Operating expenses decreased by 3%.", "chunk_index": 1 }
  ]
}

// Response
{
  "document_node_id": 50,
  "chunks_inserted": 2,
  "record_ids": [101, 102]
}
```

#### `POST /v1/ingest/update`
Updates or replaces chunks for an existing document node.
```json
// Request Payload
{
  "collection": "default",
  "document_node_id": 50,
  "chunks": [
    { "text": "Updated revenue increased by 15% year over year.", "chunk_index": 0 }
  ]
}

// Response
{
  "updated": true,
  "document_node_id": 50,
  "new_record_ids": [103]
}
```

#### `POST /v1/ingest/document`
Server-side file ingestion from a local file path accessible by the server process.
```json
// Request Payload
{
  "file_path": "/var/data/ingest/manual.pdf",
  "collection": "default",
  "chunk_size": 1200
}

// Response
{
  "job_id": "op:ingest:882",
  "status": "QUEUED"
}
```

#### `POST /v1/ingest/extract-entities`
Extracts entities and relationships from raw text to populate the knowledge graph.
```json
// Request Payload
{
  "text": "Apple CEO Tim Cook announced the new Vision Pro in Cupertino.",
  "collection": "default"
}

// Response
{
  "entities": [
    { "name": "Apple", "kind": "Company" },
    { "name": "Tim Cook", "kind": "Person" },
    { "name": "Vision Pro", "kind": "Product" }
  ],
  "relationships": [
    { "from": "Tim Cook", "to": "Apple", "relation": "CEO_OF" }
  ]
}
```

---

### 4. Knowledge Graph

#### `POST /v1/graph/node`
Creates a semantic node in the knowledge graph.
```json
// Request Payload
{
  "kind": 0,       // 0: Document, 1: Chunk, 2: Concept, 3: Agent
  "record_id": 42, // Optional link to vector record
  "collection": "default"
}

// Response
{
  "node_id": 501,
  "kind": 0
}
```

#### `GET /v1/graph/node/:id?collection=default`
Retrieves a specific graph node and its properties.
```json
// Response
{
  "node_id": 501,
  "kind": 0,
  "record_id": 42,
  "collection": "default"
}
```

#### `DELETE /v1/graph/node/:id`
Deletes a graph node and removes all incoming/outgoing edges connected to it.
```json
// Request Payload
{
  "collection": "default"
}

// Response
{
  "deleted": true,
  "node_id": 501,
  "edges_removed": 3
}
```

#### `GET /v1/graph/nodes?collection=default&limit=100&offset=0`
Lists all graph nodes in a namespace with pagination.
```json
// Response
{
  "nodes": [
    { "node_id": 501, "kind": 0, "record_id": 42 },
    { "node_id": 502, "kind": 1, "record_id": 43 }
  ],
  "total": 2
}
```

#### `POST /v1/graph/edge`
Creates a directed edge between two graph nodes.
```json
// Request Payload
{
  "kind": 1,       // 0: HasChunk, 1: Mentions, 2: Follows, 3: Contradicts
  "from": 501,
  "to": 502,
  "collection": "default"
}

// Response
{
  "edge_id": 9001,
  "from": 501,
  "to": 502
}
```

#### `GET /v1/graph/edges/:id?collection=default`
Retrieves all incoming and outgoing edges for a node.
```json
// Response
{
  "outgoing": [
    { "edge_id": 9001, "kind": 1, "to": 502 }
  ],
  "incoming": []
}
```

#### `GET /v1/graph/subgraph?node_id=501&depth=2&collection=default`
Extracts an N-hop neighborhood subgraph centered around a specific node.
```json
// Response
{
  "nodes": [
    { "node_id": 501, "kind": 0 },
    { "node_id": 502, "kind": 1 }
  ],
  "edges": [
    { "edge_id": 9001, "from": 501, "to": 502, "kind": 1 }
  ]
}
```

---

### 5. Community Detection

#### `POST /v1/community/detect`
Triggers asynchronous Leiden/Louvain hierarchical clustering over the graph.
```json
// Request Payload
{
  "collection": "default",
  "resolution": 1.0
}

// Response
{
  "operation_id": "op:community:987"
}
```

#### `GET /v1/community/overview?collection=default`
Retrieves summaries of detected graph communities.
```json
// Response
{
  "communities": [
    { "id": 1, "level": 0, "node_count": 45, "summary": "Discussions regarding security protocols and firewall configuration." },
    { "id": 2, "level": 0, "node_count": 12, "summary": "Financial Q3 revenue reports and employee compensation." }
  ]
}
```

#### `POST /v1/community/search`
Searches community summary texts using vector similarity.
```json
// Request Payload
{
  "query": "firewall settings",
  "collection": "default",
  "k": 3
}

// Response
{
  "results": [
    { "community_id": 1, "score": 0.89, "summary": "Discussions regarding security protocols..." }
  ]
}
```

---

### 6. Agentic Memory Protocol

#### `POST /v1/memory/meta/set`
Attaches JSON metadata or LLM context sentences to a target identifier (`record:123` or `node:45`).
```json
// Request Payload
{
  "target_id": "record:101",
  "collection": "default",
  "metadata": {
    "context_sentence": "This chunk discusses Q3 revenue spikes.",
    "source_author": "Alice"
  }
}

// Response
{
  "success": true
}
```

#### `GET /v1/memory/meta/get?target_id=record:101&collection=default`
Retrieves attached metadata for a target identifier.
```json
// Response
{
  "target_id": "record:101",
  "metadata": {
    "context_sentence": "This chunk discusses Q3 revenue spikes.",
    "source_author": "Alice"
  }
}
```

#### `POST /v1/memory/contradict`
Scans stored memories to flag semantic contradictions.
```json
// Request Payload
{
  "collection": "default",
  "threshold": 0.85
}

// Response
{
  "contradictions": [
    {
      "node_a": 502,
      "node_b": 610,
      "confidence": 0.92,
      "reason": "Node 502 claims midnight restart; Node 610 claims continuous 24/7 uptime."
    }
  ]
}
```

#### `POST /v1/memory/upsert` (`_vector`)
High-level atomic memory creation: inserts vector, creates chunk node, and attaches to document.
```json
// Request Payload
{
  "collection": "default",
  "vector": [0.05, -0.12, 0.33],
  "attach_to_document_node": 10,
  "metadata": { "text": "Server must be restarted weekly." }
}

// Response
{
  "record_id": 105,
  "chunk_node_id": 502,
  "document_node_id": 10
}
```

#### `POST /v1/memory/search` (`_vector`)
High-level memory search returning both vector scores and graph context.
```json
// Request Payload
{
  "query_vector": [0.05, -0.12, 0.33],
  "k": 5,
  "collection": "default"
}

// Response
{
  "results": [
    { "record_id": 105, "chunk_node_id": 502, "score": 9, "text": "Server must be restarted weekly." }
  ]
}
```

#### `POST /v1/memory/consolidate`
Triggers background agent memory decay, deduplication, and consolidation.
```json
// Request Payload
{
  "collection": "default",
  "decay_factor": 0.95
}

// Response
{
  "operation_id": "op:consolidate:332"
}
```

#### `POST /v1/graphrag`
Executes GraphRAG retrieval: finds top-K vector hits, expands N-hop neighbors in the graph, and returns combined context.
```json
// Request Payload
{
  "query_vector": [0.1, 0.2, 0.3],
  "k": 3,
  "hop_depth": 2,
  "collection": "default"
}

// Response
{
  "vector_hits": [ { "record_id": 101, "score": 5 } ],
  "expanded_graph_nodes": [ { "node_id": 501, "kind": 0 }, { "node_id": 502, "kind": 1 } ],
  "context_summary": "Combined text from vector hits and graph neighbors..."
}
```

---

### 7. Proof, Audit & Operations

#### `GET /v1/timeline?collection=default&limit=50`
Retrieves chronological audit trail of all mutations in a namespace.
```json
// Response
{
  "events": [
    {
      "tx_id": "tx_889102",
      "timestamp_ms": 1751883200,
      "action": "INSERT_VECTOR",
      "target_id": "rec:105",
      "blake3_receipt": "7a8b...hash"
    }
  ]
}
```

#### `GET /v1/operations?collection=default`
Lists all active and completed background operations.
```json
// Response
{
  "operations": [
    { "id": "op:community:987", "type": "COMMUNITY_DETECT", "status": "IN_PROGRESS", "progress_pct": 65 }
  ]
}
```

#### `GET /v1/operations/:id`
Polls status and percentage completion of a specific job.
```json
// Response
{
  "id": "op:community:987",
  "status": "IN_PROGRESS",
  "progress_pct": 65,
  "started_at": "2026-07-07T10:00:00Z",
  "message": "Clustering level 2..."
}
```

#### `GET /v1/operations/:id/execution`
Retrieves detailed step breakdown and timing logs for a job.
```json
// Response
{
  "id": "op:community:987",
  "steps": [
    { "step": "Load Graph", "duration_ms": 120, "status": "DONE" },
    { "step": "Leiden Clustering", "duration_ms": 450, "status": "IN_PROGRESS" }
  ]
}
```

#### `GET /v1/proof/state`
Returns global BLAKE3 state hash and merkle roots for audit verification.
```json
// Response
{
  "version": 1420,
  "state_hash": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "record_count": 12500,
  "node_count": 340,
  "edge_count": 890
}
```

#### `GET /v1/cluster/proof` (Cluster Mode)
Returns the distributed BLAKE3 consensus root across all active Raft shards.
```json
// Response
{
  "cluster_state_hash": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  "shards_converged": true,
  "shard_count": 4
}
```

#### `GET /v1/proof/receipt?collection=default`
Retrieves the latest cryptographic tamper-evident transaction receipt.
```json
// Response
{
  "receipt_id": "rcpt_889102",
  "tx_id": "tx_889102",
  "timestamp_ms": 1751883200,
  "prev_receipt_hash": "6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b",
  "receipt_hash": "d4735e3a265e16eee03f59718b9b5d03019c07d8b6c51f90da3a666eec13ab35"
}
```

#### `GET /v1/proof/receipt/:id`
Retrieves a specific historical receipt by transaction or receipt ID.
```json
// Response
{
  "receipt_id": "rcpt_889100",
  "tx_id": "tx_889100",
  "receipt_hash": "6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b"
}
```

#### `GET /v1/proof/event-log?collection=default`
Downloads the immutable binary audit event log stream for offline verification.
```text
HTTP/1.1 200 OK
Content-Type: application/octet-stream

<binary event log byte stream>
```

---

### 8. Tree-RAG (Hierarchical TOC Retrieval)
*Note: These endpoints operate purely on document structure without requiring vector embeddings or floating-point math.*

#### `POST /v1/tree/build`
Builds a deterministic Table-of-Contents tree from markdown/text and stores it in the server cache.
```json
// Request Payload
{
  "text": "# 1.0 Introduction\nValori is deterministic...\n## 1.1 Architecture\nThe kernel uses fixed-point...",
  "doc_name": "valori_manual.md"
}

// Response
{
  "cache_key": "8f3a...blake3_hash",
  "doc_name": "valori_manual.md",
  "node_count": 3,
  "structure_map": [
    { "node_id": "0001", "title": "1.0 Introduction", "summary": "Valori is deterministic..." }
  ],
  "tree": { "doc_name": "valori_manual.md", "roots": ["0001"], "nodes": { ... } }
}
```

#### `POST /v1/tree/query`
Navigates the tree to answer a question, returning exact line citations and a tamper-evident receipt.
```json
// Request Payload
{
  "cache_key": "8f3a...blake3_hash",
  "query": "What math does the architecture use?",
  "k": 2
}

// Response
{
  "answer": "The kernel uses fixed-point...",
  "citations": [
    { "node_id": "0002", "title": "1.1 Architecture", "breadcrumb": "valori_manual.md > 1.0 Introduction > 1.1 Architecture", "lines": [3, 5] }
  ],
  "receipt": {
    "receipt_hash": "a1b2...",
    "cited_sections": ["0002"]
  }
}
```

#### `POST /v1/tree/hybrid`
Fuses structural Tree-RAG navigation scores with vector similarity search in a single call.
```json
// Request Payload
{
  "cache_key": "8f3a...blake3_hash",
  "query": "fixed-point math accuracy",
  "namespace": "default",
  "k": 3,
  "tree_weight": 0.6
}

// Response
{
  "hits": [
    { "source": "tree", "score": 0.95, "node_id": "0002", "title": "1.1 Architecture", "lines": [3, 5] },
    { "source": "vector", "score": 0.82, "record_id": 105, "distance": 12 }
  ]
}
```

#### `POST /v1/tree/verify`
Replays a receipt against a tree index to cryptographically prove cited lines were not altered.
```json
// Request Payload
{
  "tree": { ... },
  "receipt": { "receipt_hash": "a1b2...", "cited_sections": ["0002"] }
}

// Response
{
  "valid": true
}
```

#### `POST /v1/tree/chain-verify`
Verifies an ordered sequence of receipts forms an unbroken cryptographic chain.
```json
// Request Payload
{
  "receipts": [
    { "receipt_hash": "rcpt_1", "prev_hash": "GENESIS" },
    { "receipt_hash": "rcpt_2", "prev_hash": "rcpt_1" }
  ]
}

// Response
{
  "chain_valid": true,
  "verified_count": 2
}
```

---

### 9. Snapshots, Storage & WAL Replication

#### `GET /v1/storage/snapshots?collection=default`
Lists remote backups available in configured object storage (S3 / backup disk).
```json
// Response
{
  "snapshots": [
    { "snapshot_id": "snap-20260707-101500", "size_bytes": 10485760, "created_at": "2026-07-07T10:15:00Z" }
  ]
}
```

#### `POST /v1/storage/snapshots/upload`
Creates a snapshot of current state and uploads to remote storage.
```json
// Request Payload
{
  "collection": "default",
  "tag": "pre-upgrade-backup"
}

// Response
{
  "snapshot_id": "snap-20260707-101500",
  "size_bytes": 10485760,
  "state_hash": "e3b0c4..."
}
```

#### `POST /v1/storage/snapshots/restore`
Restores kernel state from a remote storage snapshot ID.
```json
// Request Payload
{
  "snapshot_id": "snap-20260707-101500",
  "collection": "default"
}

// Response
{
  "restored": true,
  "records_loaded": 12500
}
```

#### `GET /v1/snapshot/download?collection=default`
Direct binary download of current memory state.
```text
HTTP/1.1 200 OK
Content-Type: application/octet-stream

<binary snapshot bytes>
```

#### `POST /v1/snapshot/upload?collection=default`
Direct binary restore by uploading raw snapshot bytes in the HTTP request body.
```text
POST /v1/snapshot/upload?collection=default HTTP/1.1
Content-Type: application/octet-stream

<binary snapshot bytes>
```

#### `POST /v1/snapshot/save`
Saves kernel state to the server's local disk backup directory.
```json
// Request Payload
{
  "collection": "default",
  "filename": "local_backup.bin"
}

// Response
{
  "saved": true,
  "path": "/var/lib/valori/snapshots/local_backup.bin"
}
```

#### `POST /v1/snapshot/restore`
Restores kernel state from a file on the server's local disk.
```json
// Request Payload
{
  "collection": "default",
  "filename": "local_backup.bin"
}

// Response
{
  "restored": true,
  "records_loaded": 12500
}
```

#### `GET /v1/storage/wal?collection=default`
Lists archived Write-Ahead Log (WAL) segments in cold storage.
```json
// Response
{
  "segments": [
    { "segment_id": "wal_0001", "start_index": 1, "end_index": 10000, "size_bytes": 4194304 }
  ]
}
```

#### `POST /v1/storage/wal/archive`
Forces immediate archiving and rotation of the active WAL segment to cold storage.
```json
// Request Payload
{
  "collection": "default"
}

// Response
{
  "archived_segment_id": "wal_0002",
  "records_archived": 5000
}
```

#### `GET /v1/replication/wal?offset=889000`
Streams live WAL bytes to follower nodes starting from a specific Raft log offset.
```text
HTTP/1.1 200 OK
Content-Type: application/octet-stream

<continuous WAL byte stream>
```

#### `GET /v1/replication/events?after_tx=tx_889000`
Streams committed transaction event records for cross-node read replica synchronization.
```json
// Response
{
  "events": [
    { "tx_id": "tx_889001", "action": "INSERT_VECTOR", "payload": { ... } }
  ]
}
```

#### `GET /v1/replication/state`
Returns current replication log offset and follower lag statistics.
```json
// Response
{
  "leader_commit_index": 889102,
  "local_applied_index": 889102,
  "lag_records": 0,
  "in_sync": true
}
```

---

### 10. Security, Crypto & Index Administration

#### `POST /v1/keys`
Creates a new scoped API authentication key.
```json
// Request Payload
{
  "name": "production-agent-key",
  "role": "read_write",
  "collections": ["default", "finance"]
}

// Response
{
  "key_id": "key_9901",
  "secret": "vk_live_88329018490218490"
}
```

#### `GET /v1/keys`
Lists active API keys (secrets are redacted).
```json
// Response
{
  "keys": [
    { "key_id": "key_9901", "name": "production-agent-key", "role": "read_write", "created_at": "2026-07-07T00:00:00Z" }
  ]
}
```

#### `DELETE /v1/keys/:id`
Permanently revokes an API key.
```json
// Response
{
  "revoked": true,
  "key_id": "key_9901"
}
```

#### `POST /v1/records/encrypted`
Inserts a vector record payload encrypted via envelope encryption.
```json
// Request Payload
{
  "collection": "default",
  "id": 500,
  "encrypted_vector_payload": "base64_encoded_ciphertext...",
  "key_id": "key_tenant_finance"
}

// Response
{
  "inserted": true,
  "id": 500
}
```

#### `DELETE /v1/crypto/shred/:key_id`
Performs cryptographic shredding by destroying the envelope encryption key, instantly rendering all associated records permanently unreadable (GDPR right-to-erasure).
```json
// Response
{
  "shredded": true,
  "key_id": "key_tenant_finance",
  "records_invalidated": 4200
}
```

#### `GET /v1/crypto/status/:key_id`
Checks encryption key status and verification of crypto-shredding.
```json
// Response
{
  "key_id": "key_tenant_finance",
  "status": "SHREDDED",
  "shredded_at": "2026-07-07T10:30:00Z"
}
```

#### `GET /v1/index/config?collection=default`
Returns current vector indexing algorithm configuration and quantization settings.
```json
// Response
{
  "collection": "default",
  "index_type": "hnsw",
  "metric": "l2",
  "quantization": "q16.16",
  "hnsw_m": 16,
  "hnsw_ef_construction": 200
}
```

#### `POST /v1/index/rebuild`
Triggers an asynchronous background re-indexing and quantization job to optimize search latency.
```json
// Request Payload
{
  "collection": "default",
  "target_index_type": "hnsw",
  "m": 16,
  "ef_construction": 200
}

// Response
{
  "operation_id": "op:rebuild:441"
}
```

#### `GET /v1/shard/routing?collection=default`
Returns consistent-hashing shard routing table for multi-node deployments.
```json
// Response
{
  "collection": "default",
  "shards": [
    { "shard_id": 0, "leader_node": "10.0.0.1:3000", "hash_range": ["0000", "3fff"] },
    { "shard_id": 1, "leader_node": "10.0.0.2:3000", "hash_range": ["4000", "7fff"] }
  ]
}
```
