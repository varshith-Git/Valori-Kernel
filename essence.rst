Valori Kernel - Codebase Essence
================================

This document provides a comprehensive, deep-dive analysis of the entire Valori codebase structure. 
It maps every file and folder (excluding build artifacts and hidden files) to its specific purpose in the system.

.. note:: 
    **Legend**:
    - **[DIR]**: Directory
    - **[FILE]**: Source file or Configuration
    - **[TEST]**: Test suite or verification script

Root Directory
--------------
High-level project configuration and documentation.

- ``.`` (**Root**)
    - ``Cargo.toml``: **[FILE]** Workspace definition. Manages shared dependencies and members (kernel, node, ffi, etc.).
    - ``README.md``: **[FILE]** Primary entry point. Contains architecture overview, quickstart guides, and project status.
    - ``architecture.md``: **[FILE]** Detailed architectural blueprint (Layered design, Determinism specs).
    - ``all-functions.md``: **[FILE]** Auto-generated reference of all kernel functions.
    - ``build.log``: **[FILE]** Log file capturing build outputs (useful for debugging CI/Build failures).
    - ``Dockerfile``: **[FILE]** Docker container definition for packaging the Valori Node server.
    - ``LICENSE``: **[FILE]** AGPLv3 License text.
    - ``COMMERCIAL_LICENSE.md``: **[FILE]** Commercial licensing terms.
    - ``test_replication_e2e.py``: **[TEST]** Top-level end-to-end Python script verifying Leader-Follower replication.
    - ``valori_ffi.pyd``: **[FILE]** Compiled Python extension module (shared object) for direct FFI usage.

crates/ (Core Libraries)
------------------------
The workspace holding the fundamental Rust libraries.

- ``crates/`` (**Workspace**)
    - ``kernel/``: **[DIR]** **THE HEART**. The pure, `no_std`, deterministic core.
        - ``Cargo.toml``: Dependencies for the kernel (minimal).
        - ``src/``:
            - ``lib.rs``: Kernel library entry point. Re-exports core modules.
            - ``error.rs``: Defines ``KernelError`` types (CapacityExceeded, etc.).
            - ``event.rs``: Defines ``KernelEvent`` enum for Event Sourcing (InsertRecord, CreateNode, etc.).
            - ``proof.rs``: Cryptographic proof generation logic (State Hashing).
            - ``verify.rs``: logic to verify proof signatures/hashes.
            - ``replay.rs``: Traits/Logic for replaying commands from logs.
            - ``replay_events.rs``: Logic for replaying *Events* (Phase 23+).
            - ``config.rs``: Kernel-level configuration constants/structs.
            - ``fxp/``: **[DIR]** Fixed-Point Arithmetic (Q16.16)
                - ``mod.rs``: Module definition.
                - ``qformat.rs``: The ``FxpScalar`` type definition (i32 wrapper).
                - ``ops.rs``: Deterministic implementation of Add, Sub, Mul, Div, Sqrt.
            - ``graph/``: **[DIR]** Knowledge Graph Components
                - ``mod.rs``:  Module exposure.
                - ``node.rs``: ``Node`` struct definition (ID, Kind, pointers).
                - ``edge.rs``: ``Edge`` struct definition (From, To, Kind).
                - ``pool.rs``: Static memory pool implementation for Nodes/Edges.
                - ``adjacency.rs``: Adjacency list logic for graph traversal.
            - ``index/``: **[DIR]** Vector Indexing
                - ``mod.rs``: Indexing traits.
                - ``brute_force.rs``: Baseline sequential scan search (Deterministic reference).
            - ``math/``: **[DIR]** Vector Math
                - ``mod.rs``: Math traits.
                - ``l2.rs``: Euclidean distance calculation (using FXP).
                - ``dot.rs``: Dot product calculation.
            - ``quant/``: **[DIR]** Quantization (Future)
                - ``mod.rs``: Traits for vector quantization.
            - ``snapshot/``: **[DIR]** State Serialization
                - ``mod.rs``: Snapshot traits.
                - ``encode.rs``: Deterministic serialization of KernelState.
                - ``decode.rs``: Deserialization logic.
                - ``hash.rs``: Computing the cryptographic hash of the state.
                - ``blake3.rs``: Wrapper around BLAKE3 hasher.
            - ``state/``: **[DIR]** State Machine
                - ``mod.rs``: State module.
                - ``kernel.rs``: **MAIN STRUCT** ``KernelState``. Holds storage, graph, and index.
                - ``command.rs``: ``Command`` enum for Legacy WAL operations.
            - ``storage/``: **[DIR]** Vector Storage
                - ``mod.rs``: Storage traits.
                - ``record.rs``: ``Record`` struct (ID + Vector data).
                - ``pool.rs``: Static memory pool for Records.
            - ``types/``: **[DIR]** Core Types
                - ``mod.rs``: Type exports.
                - ``vector.rs``: Generic ``FxpVector`` struct.
                - ``scalar.rs``: Scalar type aliases.
                - ``id.rs``: Strongly typed IDs (RecordId, NodeId, EdgeId).
                - ``enums.rs``: Enumerations (NodeKind, EdgeKind).
            - ``tests/``: **[DIR]** **[TEST]** Kernel Unit Tests
                - ``mod.rs``: Test module setup.
                - ``determinism_tests.rs``: Tests verifying cross-arch output stability.
                - ``fxp_tests.rs``: Tests for fixed-point math precision/overflow.
                - ``graph_tests.rs``: Tests for graph operations (add node/edge).
                - ``index_tests.rs``: Tests for search correctness.
                - ``math_tests.rs``: Tests for distance functions.
                - ``proof_tests.rs``: Tests for state hashing.
                - ``quant_tests.rs``: Tests for quantization.
                - ``snapshot_tests.rs``: Tests for save/restore roundtrips.
                - ``state_tests.rs``: Tests for basic kernel state operations.
                - ``storage_tests.rs``: Tests for record pool management.
                - ``e2e_tests.rs``: Internal end-to-end flows.

    - ``persistence/``: **[DIR]** Persistence Abstractions
        - ``Cargo.toml``: Dependencies.
        - ``src/``:
            - ``lib.rs``: Library entry.
            - ``error.rs``: Persistence-specific errors.
            - ``fixtures.rs``: Helpers for creating test data (WALs, Snapshots).
            - ``idx.rs``: Logic for handling index files.
            - ``snapshot.rs``: Traits for reading/writing snapshots.
            - ``wal.rs``: Traits for Write-Ahead Log interaction.

    - ``cli/``: **[DIR]** Command Line Interface
        - ``Cargo.toml``: Dependencies (Clap, etc.).
        - ``tests/``: Integration tests for CLI arguments.
        - ``src/``:
            - ``main.rs``: CLI entry point.
            - ``commands/``: (If present) Subcommand implementations.

    - ``demo-generator/``: **[DIR]** Data Generator
        - ``Cargo.toml``: Dependencies.
        - ``src/``:
            - ``main.rs``: Executable to generate synthetic vector/graph data for demos.

node/ (Server Implementation)
-----------------------------
The production-grade HTTP server and application logic wrapping the kernel.

- ``node/`` (**Server**)
    - ``Cargo.toml``: Server dependencies (Axum, Tokio, Serde, Tracing).
    - ``build_err.txt``: Capture of build errors (ephemeral).
    - ``src/``:
        - ``main.rs``: **ENTRY POINT**. Initializes logging, config, engine, and HTTP server.
        - ``lib.rs``: Exposes modules for integration testing.
        - ``server.rs``: **API LAYER**. Defines routes (POST /records, etc.) and handlers.
        - ``api.rs``: **DTOs**. Request/Response structs (Serde derived) including Batch types.
        - ``engine.rs``: **COORDINATOR**. Wraps ``KernelState``, manages WAL/EventLog I/O, locks, and high-level logic.
        - ``config.rs``: ``NodeConfig`` struct parsing (env vars / CLI args).
        - ``errors.rs``: ``EngineError`` enum and Actix/Axum error mapping.
        - ``telemetry.rs``: Prometheus metrics setup and Tracing subscriber init.
        - ``metadata.rs``: Key-Value store for auxiliary metadata (not in core kernel).
        - ``persistence.rs``: Integration with ``crates/persistence``.
        - ``recovery.rs``: Logic to replay WAL/Snapshots on startup.
        - ``replication.rs``: **REPLICATION**. Logic for Leader/Follower states and stream consumption.
        - ``wal_writer.rs``: Appends commands to disk with ``fsync``.
        - ``wal_reader.rs``: Iterates over WAL files.
        - ``network/``: **[DIR]** Internal Cluster Networking
            - ``mod.rs``: Module definitions.
            - ``client.rs``: (Likely) Internal client for node-to-node communication.
        - ``events/``: **[DIR]** Event Sourcing Implementation (Phase 23+)
            - ``mod.rs``: Module exports.
            - ``event_log.rs``: Handles I/O for the append-only Event Log. Includes ``append_batch``.
            - ``event_commit.rs``: **CRITICAL**. Implements the Atomic Commit Pipeline (Shadow -> Persist -> Apply).
            - ``event_replay.rs``: Logic to rebuild state from Event Log.
        - ``structure/``: **[DIR]** HNSW Indexing (Host-side)
            - ``mod.rs``: Module exports.
            - ``hnsw.rs``: HNSW Implementation (approximate search index managed by Node, not Kernel).
    - ``tests/``: **[DIR]** **[TEST]** Integration Test Suite
        - ``integration_tests.rs``: Basic HTTP API tests.
        - ``api_batch_ingest.rs``: **[TEST]** Verifies atomic batch ingestion and rollback.
        - ``api_replication.rs``: **[TEST]** Verifies Leader-Follower replication sync.
        - ``replication_bootstrap.rs``: Tests initial snapshot bootstrap for followers.
        - ``replication_cluster.rs``: Tests multi-node cluster scenarios.
        - ``replication_divergence.rs``: Tests detection of state divergence.
        - ``fuzz_crash_recovery.rs``: **[TEST]** Fuzz testing for crash consistency.
        - ``persistence_tests.rs``: Verifies WAL writing/reading.
        - ``persistence_index_tests.rs``: Verifies persistence of auxiliary indices.
        - ``deterministic_ivf_tests.rs``: Tests for IVF index determinism.
        - ``deterministic_pq_tests.rs``: Tests for PQ index determinism.
        - ``deterministic_kmeans_tests.rs``: Tests for K-Means clustering.
        - ``deterministic_edge_tests.rs``: Tests for graph edge cases.
        - ``hnsw_tests.rs``: Tests for HNSW index recall/precision.
        - ``multi_arch_determinism.rs``: **[TEST]** Critical test for cross-arch output stability.
        - ``fuzz/``: Directory for fuzz targets.

python/ (Client SDK)
--------------------
The usage-facing Python library.

- ``python/`` (**SDK**)
    - ``setup.py``: Build script using ``setuptools-rust`` to compile the FFI extension.
    - ``test_valori_integrated.py``: **[TEST]** Integration test validating the installed package works.
    - ``test_memory.py``: Tests for memory-related features.
    - ``test_protocol.py``: Tests for the wire protocol.
    - ``test_protocol_remote.py``: Tests specifically for remote protocol.
    - ``test_unified.py``: Unified test suite.
    - ``valori/``: **[DIR]** The Python Package
        - ``__init__.py``: Package root. Exports ``Valori`` factory.
        - ``local.py``: ``LocalClient`` implementation wrapping the FFI.
        - ``remote.py``: ``RemoteClient`` implementation using HTTP (requests).
        - ``protocol.py``: Definitions of API protocol schemas.
        - ``memory.py``: High-level Memory API abstraction.
        - ``chunking.py``: Text chunking utilities.
        - ``ingest.py``: data ingestion helpers.
        - ``kinds.py``: Enum definitions for Python.
        - ``adapters/``: **[DIR]** Framework Integrations
            - ``__init__.py``: Exports.
            - ``base.py``: Base adapter classes.
            - ``langchain.py``: LangChain integration.
            - ``langchain_vectorstore.py``: VectorStore implementation for LangChain.
            - ``llamaindex.py``: LlamaIndex integration.
            - ``sentence_transformers_adapter.py``: Helper for using SentenceTransformers.
            - ``utils.py``: Utility functions.
    - ``tests/``: **[DIR]** **[TEST]** Unit Tests
        - ``test_adapters.py``: Verifies adapters work.
        - ``test_protocol_errors.py``: Verifies error handling.
    - ``examples/``: **[DIR]** Usage Examples
        - ``demo_embeddings.py``: Script showing embedding generation.
        - ``demo_remote.py``: demo of connecting to a remote node.
        - ``demo_sentence_transformers.py``: Complete flow with ST.

ffi/ (Python Bindings)
----------------------
The bridge between Rust and Python.

- ``ffi/`` (**FFI**)
    - ``Cargo.toml``: Dependencies (PyO3, valori-kernel, valori-node).
    - ``src/``:
        - ``lib.rs``: **[FILE]** The PyO3 binding logic. Exposes ``ValoriEngine`` class to Python.
    - ``test_valori.py``: Quick verify script for the built shared object.

embedded/ (Bare Metal)
----------------------
Experiments and implementations for embedded targets.

- ``embedded/`` (**Embedded**)
    - ``Cargo.toml``: Embedded dependencies.
    - ``src/``:
        - ``main.rs``: Entry point for embedded demo.
        - ``flash.rs``: Flash memory storage simulation/driver.
        - ``checkpoint.rs``: Checkpointing logic for embedded.
        - ``recovery.rs``: Recovery logic tailored for low-resource environments.
        - ``proof.rs``: Proof generation for constrained devices.
        - ``shadow.rs``: Shadow execution (dual-run) logic.
        - ``snapshot.rs``: Snapshot handling.
        - ``transport.rs``: Communication logic (Serial/UART etc.).
        - ``wal.rs``: Wal implementation for flash.
        - ``wal_stream.rs``: Streaming WAL logic.

docs/ (Documentation)
---------------------
Project documentation and specifications.

- ``docs/``
    - ``api-reference.md``: HTTP API Endpoint details.
    - ``python-usage-guide.md``: Detailed Python SDK guide.
    - ``core-concepts.md``: Explanations of Fixed-Point, Determinism, Kernel.
    - ``getting-started.md``: General onboarding.
    - ``embedded-quickstart.md``: Specifics for ARM/Embedded usage.
    - ``remote-mode.md``: Guide for running Server/Clustered mode.
    - ``architecture.md``: (See root architecture.md).
    - ``determinism-guarantees.md``: Formal contract of what is guaranteed.
    - ``multi-arch-determinism.md``: Proofs and CI details for cross-arch support.
    - ``verifiable-replication.md``: Explain how replication is verified.
    - ``wal-replay-guarantees.md``: Crash recovery specifics.
    - ``memory_protocol_v0.md``: Legacy protocol specs.
    - ``memory_protocol_v1.md``: Current protocol specs.
    - ``authentication.md``: Auth specs.
    - ``adapter-improvements.md``: Notes on framework adapters.
    - ``publishing-pypi.md``: Guide for releasing the Python package.
    - ``functions.md``: Likely function reference.
    - ``python-reference.md``: Python API reference.

demo/ & examples/
-----------------
Scripts and resources for demonstration.

- ``demo/``
    - ``demo_adapters.py``: Demo of adapter usage.
    - ``demo_run.py``: Generic demo runner.
    - ``e2e_lifecycle.py``: **[TEST]** Lifecycle verification.
    - ``simple_remote.py``: Basic connection script.
- ``examples/`` (Root)
    - ``langchain_example.py``: Example usage with LangChain.
    - ``llamaindex_example.py``: Example usage with LlamaIndex.

verify/ (Tools)
---------------
Standalone verification utilities.

- ``verify/``
    - ``Cargo.toml``
    - ``src/``
        - ``main.rs``: CLI tool to verify database integrity offline.

src/ (Root Source?)
-------------------
*Note: This appears to be a separate crate source directory at the root level, likely for the `src` member defined in root `Cargo.toml` if applicable. It mirrors `crates/kernel/src` structure. It serves as the primary compiled source for the `valori` crate if defined in root.*

- ``src/``
    - ``lib.rs``: Entry point.
    - ``config.rs``: Configuration.
    - ``error.rs``: Error definitions.
    - ``event.rs``: Event definitions.
    - ``proof.rs``: Proof logic.
    - ``verify.rs``: Verify logic.
    - ``replay.rs``, ``replay_events.rs``: Replay logic.
    - ``fxp/``, ``graph/``, ``index/``, ``math/``, ``quant/``, ``snapshot/``, ``state/``, ``storage/``, ``types/``: **[DIR]** Mirrors of Kernel modules.
    - ``tests/``: **[DIR]** Unit tests.

demo_db/
--------
Runtime data directory (typically in `.gitignore`, but present in listing).

- ``demo_db/``
    - ``events.log``: The Event Log file.
    - ``metadata.idx``: Metadata index file.
    - ``snapshot.val``: Snapshot file.

