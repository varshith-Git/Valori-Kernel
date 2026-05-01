# Valori System Function Reference

This document provides a comprehensive folder-wise list of all functions (public and supporting/private) in the Valori codebase, along with file-level descriptions and one-line function explanations.

## 1. `src/` (Core Kernel - Rust)

### `src/adapters/ivecs.rs` - Loaders for ivecs data format.
- **Public**:
  - `IvecsLoader::new`: Initializes the loader for reading.
- **Supporting**:
  - `Iterator::next`: Iterator implementation for loading batches.

### `src/adapters/sift_batch.rs` - Zero-Copy, Batch-Optimized SIFT1M Loader.
- **Public**:
  - `SiftBatchLoader::new`: Initialize a loader starting from the beginning of the mmap.
  - `SiftBatchLoader::with_offset`: Initialize a loader starting from a specific byte offset.
  - `SiftBatchLoader::dim`: Returns the dimension of vectors in this file.
  - `SiftBatchLoader::len`: Returns the number of vectors available.
  - `SiftBatchLoader::next_batch`: Returns the next batch of raw bytes containing vectors.
  - `SiftBatchLoader::parse_vector`: Helper to parse a raw vector from a slice.

### `src/dist.rs` - Distance calculations.
- **Public**:
  - `euclidean_distance_squared`: Calculates L2^2 distance between FixedPoint vectors.
  - `euclidean_distance_fxp`: Calculates Dot Product.
  - `dot_product`: Computes dot product.

### `src/event.rs` - Event log definitions for state mutation.
- **Public**:
  - `KernelEvent::event_type`: Returns a human-readable description of the event type.
  - `Deserialize::serialize`: Serde serialization.
  - `Deserialize::deserialize`: Serde deserialization.
- **Supporting**:
  - `Serialize/Deserialize/Visitor`: Internal serialization helpers.

### `src/fxp/ops.rs` - Core fixed-point arithmetic and FFI f32 conversions.
- **Public**:
  - `fxp_add`: Basic fixed-point addition with saturation.
  - `fxp_sub`: Basic fixed-point subtraction with saturation.
  - `fxp_mul`: Fixed-point multiplication with scaling and saturation.
  - `from_f32`: Canonical f32 → Q16.16 conversion (single source of truth).
  - `to_f32`: Helper to convert FxpScalar to f32.

### `src/graph/adjacency.rs` - Graph node connection mapping.
- **Public**:
  - `add_edge`: Adds an edge to the graph, updating the adjacency list.
  - `OutEdgeIterator::new`: Iterator over outgoing edges.
- **Supporting**:
  - `Iterator::next`: Edge traversal logic.

### `src/graph/edge.rs` - Graph edge definitions.
- **Public**:
  - `GraphEdge::new`: Creates a new GraphEdge.

### `src/graph/node.rs` - Graph node definitions.
- **Public**:
  - `GraphNode::new`: Creates a new GraphNode.

### `src/graph/pool.rs` - Memory pools for graph entities.
- **Public**:
  - `NodePool::raw_nodes`: Exposes internal node array.
  - `NodePool::new`: Initializes NodePool.
  - `NodePool::insert/get/get_mut/delete/is_allocated/len/is_full`: Node pool operations.
  - `EdgePool::raw_edges`: Exposes internal edge array.
  - `EdgePool::new`: Initializes EdgePool.
  - `EdgePool::insert/get/get_mut/delete/is_allocated/len/is_full`: Edge pool operations.

### `src/hnsw.rs` - In-memory Hierarchical Navigable Small World index.
- **Public**:
  - `ValoriHNSW::new`: Initializes a new HNSW index.
  - `ValoriHNSW::insert`: Inserts a vector.
  - `ValoriHNSW::search`: Searches the index.
  - `ValoriHNSW::save`: Saves the index to disk.
  - `ValoriHNSW::load`: Loads the index from disk.
- **Supporting**:
  - `ValoriHNSW::get_vec/determine_level/insert_into_graph/search_layer/select_neighbors/add_connection`: Core HNSW internals.

### `src/index/brute_force.rs` - Exact nearest-neighbor search.
- **Public**:
  - `BruteForceIndex::search_topk`: Exhaustive linear search.
- **Supporting**:
  - `VectorIndex::on_insert/on_delete/rebuild/search`: Trait implementations.

### `src/kernel.rs` - Valori API boundary and high-level orchestrator.
- **Public**:
  - `ValoriKernel::new`: Initializes kernel.
  - `ValoriKernel::record_count`: Returns total active records.
  - `ValoriKernel::state_hash`: Computes current global state hash.
  - `ValoriKernel::apply_event`: Applies an event deterministically.
  - `ValoriKernel::search`: Performs a top-K search.
  - `ValoriKernel::insert`: Inserts a record.
  - `ValoriKernel::save_snapshot`: Serializes the state.
  - `ValoriKernel::load_snapshot`: Restores the state.

### `src/math/dot.rs` - Vector dot product.
- **Public**:
  - `fxp_dot`: Computes dot product.

### `src/math/l2.rs` - Vector L2 distance.
- **Public**:
  - `fxp_l2_sq`: Computes squared L2 distance.

### `src/replay.rs` - Recovery and playback logging.
- **Public**:
  - `WalHeader::read`: Reads WAL header bytes.

### `src/proof.rs` - Cryptographic proof utilities.
- **Public**:
  - `generate_proof_bytes`: Generates a raw 32-byte BLAKE3 Merkle root from Q16.16 integers. Single source of truth for Merkle logic.
  - `merkle_root`: Computes the recursive Merkle tree root from leaf hashes.

### `src/snapshot/blake3.rs` - Modern cryptographic state hashes.
- **Public**:
  - `hash_state_blake3`: API method for BLAKE3 hash generation.
- **Supporting**:
  - `hash_bytes`: BLAKE3 wrapper.

### `src/snapshot/decode.rs` - Custom snapshot deserialization.
- **Public**:
  - `decode_state`: Deserializes snapshot binary data back to memory.
- **Supporting**:
  - `read_u32/read_u64/read_u8/read_i32`: Binary read utilities.

### `src/snapshot/encode.rs` - Custom snapshot serialization.
- **Public**:
  - `encode_state`: Serializes memory space to binary snapshot data.
- **Supporting**:
  - `write_u32/write_i32/write_u8/write_u64/write_bytes`: Binary write utilities.

### `src/snapshot/hash.rs` - Fast state hashing (FNV-1a).
- **Public**:
  - `FnvHasher::new/write/write_u32/write_i32/finish`: Hashing primitives.
  - `FnvHasher::hash_state`: Generates an FNV-1a checksum of the kernel state.

### `src/state/kernel.rs` - Deterministic kernel state machine.
- **Public**:
  - `KernelState::new`: Initializes state.
  - `KernelState::version/record_count/node_count/edge_count`: Metadata queries.
  - `KernelState::get_record/get_node/outgoing_edges/is_edge_active`: Entity accessors.
  - `KernelState::search_l2`: Vector search implementation.
  - `KernelState::create_node/create_edge`: Graph state modifiers.
  - `KernelState::apply_event`: The ONLY valid mutation entrypoint for event-sourced operations.
  - `KernelState::apply`: Legacy command applicator.
  - `KernelState::check_invariants`: Internal safety checks.
- **Supporting**:
  - `KernelState::_delete_node/_delete_edge`: Internal deletion handlers.

### `src/storage/pool.rs` - Memory pool for fixed-size records.
- **Public**:
  - `RecordPool::raw_records`: Exposes the internal record array.
  - `RecordPool::new`: Initializes pool.
  - `RecordPool::insert`: Assigns a vector to an ID.
  - `RecordPool::delete`: Frees a slot.
  - `RecordPool::get`: Reads a slot.
  - `RecordPool::iter/len/is_full`: Pool metadata.

### `src/storage/record.rs` - Core record container.
- **Public**:
  - `Record::new`: Instantiates a record.

### `src/types/enums.rs` - Common enumeration types.
- **Public**:
  - `NodeKind::from_u8`: Casts byte to NodeKind.
  - `EdgeKind::from_u8`: Casts byte to EdgeKind.

### `src/types/id.rs` - Strongly-typed identifiers.
- **Public**:
  - `Version::next`: Iterates version safely.

### `src/types/vector.rs` - Core fixed-point arrays.
- **Public**:
  - `FxpVector::new_zeros`: Allocates an empty array.
  - `FxpVector::as_slice`: Read accessor.
  - `FxpVector::as_mut_slice`: Write accessor.
- **Supporting**:
  - `Serialize/Deserialize/Index/IndexMut/IntoIterator/Default`: Trait implementations.

---

## 2. `ffi/` (Python Bindings - Rust)

### `ffi/src/lib.rs` - PyO3 bridge exposing kernel functionality to Python.
- **Public**:
  - `ValoriEngine::new`: Initializes the Rust engine for Python usage.
  - `ValoriEngine::insert`: Insert a record. Returns the assigned ID. Valori Kernel enforces dense ID packing (first free slot).
  - `ValoriEngine::insert_batch`: Batch insert multiple vectors atomically. Returns list of assigned IDs.
  - `ValoriEngine::insert_with_proof`: Atomic insert with proof. Validates and converts f32 to Q16.16, generates BLAKE3 Merkle proof over integers, and inserts record with proof hash as `Record.metadata`. Returns `(record_id, proof_hash_hex)`.
  - `ValoriEngine::insert_batch_with_proof`: Batch atomic insert generating individual BLAKE3 proofs.
  - `ValoriEngine::search`: Performs an L2 search returning record IDs and scores.
  - `ValoriEngine::create_node`: Exposes graph node creation to Python.
  - `ValoriEngine::create_edge`: Exposes graph edge creation to Python.
  - `ValoriEngine::get_metadata`: Get metadata for a record. Returns bytes or None if no metadata.
  - `ValoriEngine::set_metadata`: Set metadata for a record. Metadata is arbitrary bytes (up to 64KB).
  - `ValoriEngine::save`: Triggers a full state snapshot save to disk.
  - `ValoriEngine::restore`: Restore from snapshot data. Loads kernel state from bytes.
  - `ValoriEngine::get_state_hash`: Get cryptographic hash of current state. Returns 32-byte BLAKE3 hash as hex string.
  - `ValoriEngine::record_count`: Get number of records in the database.
  - `ValoriEngine::soft_delete`: Soft delete a record (marks as deleted but doesn't remove).
- **Supporting**:
  - `ingest_embedding`: Convert float embeddings to Q16.16 fixed-point integers. Single source of truth.
  - `generate_proof`: Build a position-aware Merkle tree over Q16.16 integers. Returns the root hash as a hex string. Same BLAKE3 crate the kernel uses.
  - `verify_embedding`: Cryptographically verifies an embedding against a claimed proof hash.
  - `valori_ffi`: PyO3 module initialization hook exposing the classes and functions to Python.

---

## 3. `node/` (HTTP Engine - Rust)

### `node/src/api.rs` - API State and Metrics
- **Public**:
  - `ApiState::new`: Initializes API state wrapping the Engine.
  - `ApiState::metrics`: Retrieves node metrics.

### `node/src/config.rs` - Node Configuration
- **Public**:
  - `NodeConfig::default`: Provides default configuration suitable for production.
  - `NodeConfig::with_brute_force`: Configures node to use brute force indexing.
  - `NodeConfig::with_wal`: Enables Write-Ahead Log for durability.
  - `NodeConfig::with_event_log`: Enables Event Log for deterministic replays (Phase 23).

### `node/src/engine.rs` - Core HTTP Node Engine wrapping the Kernel
- **Public**:
  - `Engine::new`: Initializes the networked node engine with specific configuration.
  - `Engine::insert_record_from_f32`: Ingests standard float records and converts to fixed-point.
  - `Engine::insert_batch`: Insert a batch of records in a single atomic transaction.
  - `Engine::apply_committed_event`: Apply an event that has already been committed. Updates BOTH kernel state AND auxiliary structures.
  - `Engine::create_node_for_record`: Creates a graph node for a record.
  - `Engine::create_edge`: Creates a graph edge.
  - `Engine::search_l2`: Performs vector search utilizing the configured advanced index.
  - `Engine::save_snapshot`: Checkpoints state, metadata, and indices to disk.
  - `Engine::snapshot`: Serializes the entire engine state in memory.
  - `Engine::restore`: Restores engine state completely from a multipart binary blob.
  - `Engine::restore_with_wal_replay`: Restore from snapshot then replay WAL for crash recovery (primary recovery method).
  - `Engine::get_proof`: Retrieves cryptographic proofs for compliance.
  - `Engine::root_hash`: Calculates the overall root hash of the engine.
- **Supporting**:
  - `Engine::rebuild_index`: Rebuilds index from kernel state.
  - `Engine::restore_from_components`: Restores node from separate memory components.

### `node/src/errors.rs` - Engine Errors
- **Public**:
  - `EngineError::into_response`: Converts engine errors to HTTP Axum responses.
- **Supporting**:
  - `From::from`: Type conversion from KernelError.

### `node/src/metadata.rs` - JSON Metadata Store
- **Public**:
  - `MetadataStore::new`: Initializes the metadata store.
  - `MetadataStore::set`: Sets metadata for a specific key.
  - `MetadataStore::get`: Retrieves metadata.
  - `MetadataStore::snapshot`: Serializes the metadata to binary.
  - `MetadataStore::restore`: Deserializes the metadata from binary.

### `node/src/network/client.rs` - Leader Node Client
- **Public**:
  - `NodeClient::new`: Initializes the client.
  - `NodeClient::base_url`: Returns the base URL of the remote node.
  - `NodeClient::get_proof`: Fetches the proof from the remote node.
  - `NodeClient::stream_events`: Stream events starting from a specific offset.
  - `NodeClient::download_snapshot`: Download the latest full snapshot.
- **Supporting**:
  - `Clone::clone`: Client cloning.

### `node/src/persistence.rs` - Snapshot Persistence
- **Public**:
  - `SnapshotManager::save`: Save kernel state and metadata to a combined V2 snapshot file.
  - `SnapshotManager::parse`: Parse a combined snapshot file into its components.

### `node/src/recovery.rs` - WAL Recovery logic
- **Public**:
  - `has_wal`: Check if WAL file exists.
  - `replay_wal`: Replay WAL file into kernel state.

### `node/src/replication.rs` - Cluster Replication
- **Public**:
  - `spawn_replication_stream`: Spawns a background task that listens for incoming events from the leader.
  - `run_follower_loop`: The main entry point for a follower node (bootstraps and streams).
- **Supporting**:
  - `bootstrap_from_leader`: Downloads snapshot from leader and initializes state.

### `node/src/server.rs` - HTTP server routing and handlers.
- **Public**:
  - `build_router`: Constructs the Axum HTTP routing layer for the node.
- **Supporting**:
  - `handle_insert_batch / handle_insert / handle_search`: Vector HTTP handlers.
  - `handle_create_node / handle_create_edge`: Graph HTTP handlers.
  - `handle_snapshot / handle_restore / handle_get_proof`: System HTTP handlers.
  - `get_wal_stream`: Endpoint for followers to stream the WAL in real-time.
  - `get_replication_events`: Endpoint to fetch historical events for catch-up.
  - `get_replication_state`: Endpoint to check current replication state.
  - `metrics_handler`: Endpoint to serve Prometheus observability metrics.

### `node/src/wal_reader.rs` - Write-ahead Log Reading
- **Public**:
  - `WalReader::open`: Opens WAL for reading.
  - `WalReader::from_file`: Initializes reader from a file handle.
- **Supporting**:
  - `WalReader::read_header`: Validates WAL magic bytes and version.
  - `WalReader::ensure_not_eof`: Checks if the reader has reached the end of the log.
  - `Iterator::next / IntoIterator::into_iter`: Trait implementations.

### `node/src/wal_writer.rs` - Write-ahead Log Writing
- **Public**:
  - `WalWriter::open`: Opens or creates a WAL file safely.
  - `WalWriter::append_command`: Append a single command safely to WAL.
  - `WalWriter::sync`: Forces `fsync` on the WAL file.
- **Supporting**:
  - `WalWriter::write_header`: Write V1 WAL Header.

### `node/src/events/event_commit.rs` - Event log committing
- **Public**:
  - `EventCommitter::new`: Creates a new EventCommitter.
  - `EventCommitter::from_state`: Creates an EventCommitter from existing live state.
  - `EventCommitter::shadow_apply`: Executes event on shadow state for validation.
  - `EventCommitter::shadow_state/live_state/live_state_mut/journal/event_log`: State/component accessors.
  - `EventCommitter::into_state`: Consumes the committer and extracts live state.
  - `EventCommitter::commit_event`: Commits a single event using the canonical 4-step pipeline.
  - `EventCommitter::commit_batch`: Commits a batch of events atomically.
  - `EventCommitter::into_parts`: Decomposes into components for reconstruction.
  - `EventCommitter::rotate_log`: Rotates the event log (Compaction/Checkpointing).
  - `EventCommitter::subscribe`: Subscribes to live event stream.
  - `EventCommitter::write_checkpoint`: Writes a checkpoint entry and aligns journal height.

### `node/src/events/event_journal.rs` - Event buffering and state rollback
- **Public**:
  - `EventJournal::new`: Creates a new empty journal.
  - `EventJournal::new_at_height`: Creates a new empty journal starting at a specific height.
  - `EventJournal::from_committed`: Creates a journal from committed events.
  - `EventJournal::set_height`: Sets committed height.
  - `EventJournal::append_buffered`: Appends an event to the buffer (shadow execution).
  - `EventJournal::commit_buffer`: Promotes buffered events to canonical truth.
  - `EventJournal::rollback_buffer`: Discards shadow execution state.
  - `EventJournal::committed/buffered/committed_height/buffer_size`: Accessors.
  - `EventJournal::has_pending_buffer`: Checks if buffer is empty.
  - `EventJournal::subscribe`: Subscribes to live event stream.
- **Supporting**:
  - `Default::default`: Internal initialization.

### `node/src/events/event_log.rs` - Event disk logging
- **Public**:
  - `EventLogWriter::path`: Returns the log file path.
  - `EventLogWriter::open`: Opens or creates an event log file.
  - `EventLogWriter::append`: Appends a single entry to the log.
  - `EventLogWriter::append_batch`: Appends multiple entries to the log with a SINGLE fsync.
  - `EventLogWriter::event_count`: Gets the number of events written.
  - `EventLogWriter::rotate`: Rotates the event log.
- **Supporting**:
  - `EventLogHeader::new/to_bytes/from_bytes/validate`: EventLog header serialization.

### `node/src/events/event_proof.rs` - Event Merkle proofs
- **Public**:
  - `EventProof::new`: Creates a new event proof.
  - `EventProof::matches`: Verifies two proofs match (for cross-system validation).
  - `EventProof::verify`: Verifies this proof matches expected values.
  - `EventProof::compute_event_log_hash`: Computes hash of event log file using BLAKE3.
  - `EventProof::generate_proof`: Generates a complete event proof from current system state.

### `node/src/events/event_replay.rs` - Event log replay engine
- **Public**:
  - `read_event_log`: Replays events from log file.
  - `replay_events`: Replays events into a fresh kernel state.
  - `recover_from_event_log`: Performs full recovery from event log.
  - `verify_snapshot_consistency`: Verifies that snapshot is consistent with event log.
- **Supporting**:
  - `read_header`: Read and validate event log header.

### `node/src/structure/hnsw.rs` - Deterministic HNSW Index
- **Public**:
  - `HnswIndex::new`: Initializes index.
- **Supporting**:
  - `HnswIndex::dist`: Internal distance calculator.
  - `HnswIndex::deterministic_level`: Deterministic Level Generation using FNV1a.
  - `HnswIndex::safe_dist`: Safe distance calculation logic.
  - `HnswIndex::search_layer/select_neighbors`: Internal search traversal and heuristic.
  - `VectorIndex::build/insert/search/snapshot/restore`: Trait methods for unified indexing.
  - `Default/PartialEq/PartialOrd/Ord`: Core traits implementations.

### `node/src/structure/index.rs` - Index abstractions and Brute Force wrapper
- **Public**:
  - `BruteForceIndex::new`: API method.
- **Supporting**:
  - `VectorIndex::build/insert/search/snapshot/restore`: Trait methods.
  - `VectorIndex::l2_distance_sq`: Base squared L2 logic.

### `node/src/structure/ivf.rs` - Deterministic Inverted File Index
- **Public**:
  - `IvfIndex::new`: Initializes index.
- **Supporting**:
  - `IvfIndex::find_nearest_centroid`: Internal cluster location logic.
  - `VectorIndex::build/insert/search/snapshot/restore`: Trait methods.
  - `VectorIndex::l2_sq`: Internal L2 squared logic.
  - `Default::default`: Internal logic.

### `node/src/structure/quant/pq.rs` - Product Quantizer
- **Public**:
  - `ProductQuantizer::new`: Initializes PQ.
  - `ProductQuantizer::build`: Trains sub-quantizers.
  - `ProductQuantizer::snapshot`: Serializes PQ model.
  - `ProductQuantizer::restore`: Deserializes PQ model.
- **Supporting**:
  - `Quantizer::quantize/reconstruct/l2_sq`: Trait logic.
  - `Default::default`: Internal logic.

### `node/src/structure/deterministic/kmeans.rs` - Deterministic KMeans
- **Public**:
  - `deterministic_kmeans`: Bit-identical K-Means clustering algorithm. Guarantees bit-identical centroids given the same inputs.
- **Supporting**:
  - `hash_vec_id`: Hash for reproducible pseudo-random initial centroid sampling.
  - `l2_sq`: Base vector logic.

---

## 4. `verify/` (Standalone Verification Tool)

### `verify/src/main.rs` - CLI for offline state verification.
- **Public**:
  - `main`: Entry point for the CLI tool to verify snapshots against WALs offline.
- **Supporting**:
  - `parse_snapshot`: Helper to extract the kernel blob from a multipart snapshot.

---

## 5. `python/` (Python SDK)

### `python/valori/__init__.py` - SDK entry point and client factory.
- **Supporting**:
  - `Valori::__new__`: Factory yielding either a LocalClient (FFI) or RemoteClient (HTTP).

### `python/valori/adapter.py` - Drop-in adapter for external vector DBs with cryptographic proofs.
- **Public**:
  - `ValoriAdapter::__init__`: API method.
  - `ValoriAdapter::insert`: Insert into existing DB and generate a kernel-backed proof.
  - `ValoriAdapter::insert_batch`: Insert a batch into existing DB and generate kernel-backed proofs.
  - `ValoriAdapter::search`: Search existing DB and attach verification status to results.
  - `ValoriAdapter::get_proof`: Get the stored proof hash for a given external ID.
  - `ValoriAdapter::verify`: Verify an embedding against its kernel-stored proof.

### `python/valori/chunking.py` - Text chunking strategies.
- **Public**:
  - `split_by_sentences`: Deterministic, simple chunker preserving sentence boundaries.
  - `naive_paragraph_chunker`: Split on double newlines, then further break long paragraphs into max_chars chunks.

### `python/valori/ingest.py` - File loading and ingestion.
- **Public**:
  - `load_text_from_file`: If extension is .txt: read as UTF-8 safely.
  - `chunk_text`: Uses the paragraph chunker from the chunking module.

### `python/valori/local.py` - FFI local SDK client.
- **Public**:
  - `LocalClient::__init__`: Initializes the local FFI client.
  - `LocalClient::insert`: Inserts a record.
  - `LocalClient::insert_with_proof`: Insert a vector and return its ID and Merkle proof hash.
  - `LocalClient::search`: Performs a vector search.
  - `LocalClient::create_node`: Creates a knowledge graph node.
  - `LocalClient::create_edge`: Creates a knowledge graph edge.
  - `LocalClient::snapshot`: Dumps the local database to binary.
  - `LocalClient::restore`: Restores the database from binary.
  - `LocalClient::insert_batch`: Insert multiple vectors atomically.
  - `LocalClient::insert_batch_with_proof`: Insert multiple vectors atomically and generate a proof for each.
  - `LocalClient::get_metadata`: Get metadata for a record.
  - `LocalClient::set_metadata`: Set metadata for a record.
  - `LocalClient::get_state_hash`: Get cryptographic hash of current kernel state.
  - `LocalClient::record_count`: Get number of records in database.
  - `LocalClient::soft_delete`: Mark a record as deleted without removing it.

### `python/valori/memory.py` - Graph memory layer.
- **Public**:
  - `MemoryClient::__init__`: Wraps a Valori instance (local or remote).
  - `MemoryClient::add_document`: Split text into chunks using `chunk_text` and map them into the graph.
  - `MemoryClient::add_chunks`: Lower-level API to register pre-chunked text.
  - `MemoryClient::upsert_vector`: Directly upsert a vector, optionally attaching to a doc node.
  - `MemoryClient::semantic_search`: Compute embedding and semantically search across chunks.

### `python/valori/protocol.py` - High-level protocol handling.
- **Public**:
  - `ProtocolRemoteClient::__init__`: API method.
  - `ProtocolRemoteClient::snapshot`: API method.
  - `ProtocolRemoteClient::restore`: API method.
  - `ProtocolRemoteClient::upsert_vector`: API method.
  - `ProtocolRemoteClient::search_vector`: API method.
  - `ProtocolRemoteClient::set_metadata`: Set metadata for a memory_id, record_id, or node_id.
  - `ProtocolRemoteClient::get_metadata`: Get metadata for a target_id.
  - `ProtocolRemoteClient::upsert_text`: API method.
  - `ProtocolClient::__init__`: API method.
  - `ProtocolClient::snapshot`: API method.
  - `ProtocolClient::restore`: API method.
  - `ProtocolClient::set_metadata`: API method.
  - `ProtocolClient::get_metadata`: API method.
  - `ProtocolClient::upsert_text`: API method.
  - `ProtocolClient::upsert_vector`: Vector-first API.
  - `ProtocolClient::search_text`: API method.
  - `ProtocolClient::search_vector`: API method.
- **Supporting**:
  - `_validate_vector`: Validates vector type.
  - `_ensure_keys`: Validates dictionary keys.
  - `ProtocolRemoteClient::_post`: POST request helper.
  - `ProtocolClient::_memory_id_from_record_id`: Formats ID internally.

### `python/valori/remote.py` - HTTP remote SDK client.
- **Public**:
  - `RemoteClient::__init__`: Initializes HTTP client.
  - `RemoteClient::insert`: Insert a vector record. Returns the new Record ID.
  - `RemoteClient::insert_with_proof`: Insert a vector and return (id, proof_hash) with local proof calculation.
  - `RemoteClient::insert_batch`: Insert a batch of vectors. Returns list of new Record IDs.
  - `RemoteClient::search`: Search for nearest vectors. Returns list of hits.
  - `RemoteClient::create_node`: Create a graph node. Returns Node ID.
  - `RemoteClient::create_edge`: Create a graph edge. Returns Edge ID.
  - `RemoteClient::snapshot`: Download snapshot from remote.
  - `RemoteClient::restore`: Upload snapshot to remote.
- **Supporting**:
  - `RemoteClient::_post`: Internal HTTP poster.

### `python/valori/adapters/base.py` - Base adapter interface.
- **Public**:
  - `ValoriAdapter::__init__`: API method.
  - `ValoriAdapter::search_vector`: API method.
  - `ValoriAdapter::upsert_vector`: Upsert a vector with metadata.
  - `ValoriAdapter::upsert_document`: Upsert a text document with automatic embedding.
- **Supporting**:
  - `ValoriAdapter::_retry`: Retry wrapper for transient API issues.

### `python/valori/adapters/langchain.py` - LangChain integrations.
- **Public**:
  - `ValoriRetriever::__init__`: API method.
  - `ValoriRetriever::get_relevant_documents`: Retrieve documents relevant to a query for Langchain pipelines.

### `python/valori/adapters/langchain_vectorstore.py` - LangChain VectorStore interface.
- **Public**:
  - `ValoriVectorStore::__init__`: Initialize Valori vector store.
  - `ValoriVectorStore::add_texts`: Add texts to the vector store.
  - `ValoriVectorStore::add_documents`: Add LangChain Documents (text + metadata).
  - `ValoriVectorStore::similarity_search`: Search for similar documents.
  - `ValoriVectorStore::similarity_search_with_score`: Search with distance scores.
  - `ValoriVectorStore::from_texts`: Create a Valori vector store from texts.
  - `ValoriVectorStore::from_documents`: Create from LangChain Documents.

### `python/valori/adapters/llamaindex.py` - LlamaIndex integrations.
- **Public**:
  - `ValoriVectorStore::__init__`: API method.
  - `ValoriVectorStore::client`: API method.
  - `ValoriVectorStore::add`: Add nodes to index.
  - `ValoriVectorStore::delete`: Delete nodes.
  - `ValoriVectorStore::query`: Query index for top k most similar nodes.

### `python/valori/adapters/sentence_transformers_adapter.py` - Local huggingface embedder.
- **Public**:
  - `SentenceTransformerAdapter::__init__`: API method.
  - `SentenceTransformerAdapter::embed`: Embeds a single string into a list of floats.
  - `SentenceTransformerAdapter::embed_batch`: Efficient batch embedding.

### `python/valori/adapters/utils.py` - Conversion tools.
- **Public**:
  - `validate_float_range`: Validates and converts a float vector to Q16.16 compatible floats.

### `python/examples/` - Demonstration scripts.
- **Public**:
  - `demo_embeddings.py::main`: Local embedding test.
  - `demo_remote.py::main`: Remote cluster embedding test.
  - `demo_sentence_transformers.py::main`: Huggingface embedding test.
