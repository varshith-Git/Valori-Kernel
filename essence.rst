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
    - ``Cargo.toml``: **[FILE]** Workspace definition. Defines the root ``valori-workspace`` crate and members.
    - ``src/``: **[DIR]** **CORE KERNEL**. The pure, ``no_std``, deterministic core logic of Valori.
        - ``lib.rs``: Kernel entry point.
        - ``config.rs``: Kernel-level configuration.
        - ``error.rs``: ``KernelError`` definitions.
        - ``event.rs``: ``KernelEvent`` enum definitions (Event Sourcing).
        - ``proof.rs``: Cryptographic state hashing and proof generation.
        - ``verify.rs``: Logic to verify proofs and signatures.
        - ``replay.rs``: Command replay logic (Legacy).
        - ``replay_events.rs``: Event Log replay logic (Current).
        - ``fxp/``: **[DIR]** Fixed-Point Arithmetic (Q16.16).
            - ``mod.rs``, ``qformat.rs``, ``ops.rs``: Deterministic math primitives.
        - ``graph/``: **[DIR]** Knowledge Graph.
            - ``node.rs``, ``edge.rs``: Graph primitives.
            - ``adjacency.rs``: Graph traversal logic.
        - ``index/``: **[DIR]** Vector Indexing.
            - ``brute_force.rs``: Deterministic baseline search.
        - ``math/``: **[DIR]** Vector Math.
            - ``l2.rs``, ``dot.rs``: Distance calculations.
        - ``quant/``: **[DIR]** Quantization (Placeholder).
        - ``snapshot/``: **[DIR]** Snapshot Serialization.
            - ``encode.rs``, ``decode.rs``, ``hash.rs``: Deterministic save/load.
        - ``state/``: **[DIR]** State Management.
            - ``kernel.rs``: ``KernelState`` struct (The Truth).
        - ``storage/``: **[DIR]** Vector Storage.
            - ``record.rs``: Data storage.
        - ``types/``: **[DIR]** Core Types.
            - ``vector.rs``, ``id.rs``: Basic data structures.
        - ``tests/``: **[DIR]** Kernel Unit Tests.
            - ``determinism_tests.rs``, ``multi_arch_determinism.rs``: Critical safety checks.
    - ``README.md``: **[FILE]** Project overview.
    - ``architecture.md``: **[FILE]** Architecture specs.
    - ``all-functions.md``: **[FILE]** Function reference.
    - ``build.log``: **[FILE]** Build output log.
    - ``Dockerfile``: **[FILE]** Node container definition.
    - ``LICENSE``, ``COMMERCIAL_LICENSE.md``: Licensing.
    - ``test_replication_e2e.py``: **[TEST]** Top-level system test.
    - ``valori_ffi.pyd``: **[FILE]** Compiled extension.

crates/ (Workspace Members)
---------------------------
Auxiliary libraries and legacy wrappers.

- ``crates/`` (**Workspace**)
    - ``kernel/``: **[DIR]** **LEGACY / WRAPPER**. Thin wrapper around the root kernel or legacy location.
        - ``src/``:
            - ``lib.rs``: Re-exports.
            - ``kernel.rs``: Wrapper struct.
            - ``hnsw.rs``: HNSW implementation (moved from core).
    - ``persistence/``: **[DIR]** Persistence Layer
        - ``src/``:
            - ``wal.rs``, ``snapshot.rs``: Disk I/O traits and implementations.
    - ``cli/``: **[DIR]** Command Line Interface
        - ``src/main.rs``: CLI entry point.
    - ``demo-generator/``: **[DIR]** Demo Data Generator
        - ``src/main.rs``: Synthetic data tool.

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



demo_db/
--------
Runtime data directory (typically in `.gitignore`, but present in listing).

- ``demo_db/``
    - ``events.log``: The Event Log file.
    - ``metadata.idx``: Metadata index file.
    - ``snapshot.val``: Snapshot file.

