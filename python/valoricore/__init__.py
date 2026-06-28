# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
valoricore — Python SDK for Valori
====================================

**Start here — pick one client:**

+----------------------+----------------------------------------------+------------------------------+
| Client               | Import                                       | Use when                     |
+======================+==============================================+==============================+
| ``MemoryClient``     | ``from valoricore import MemoryClient``      | Local process, no server     |
|                      |                                              | (PyO3 FFI, offline-capable)  |
+----------------------+----------------------------------------------+------------------------------+
| ``SyncRemoteClient`` | ``from valoricore.remote import             | Running valori-node over     |
|                      | SyncRemoteClient``                           | HTTP (standalone or cluster) |
+----------------------+----------------------------------------------+------------------------------+
| ``AsyncRemoteClient``| ``from valoricore.remote import             | Same as above, async/await   |
|                      | AsyncRemoteClient``                          | (FastAPI, asyncio)           |
+----------------------+----------------------------------------------+------------------------------+
| ``ClusterClient``    | ``from valoricore.remote import             | 3/5-node Raft cluster,       |
|                      | ClusterClient``                              | automatic leader failover    |
+----------------------+----------------------------------------------+------------------------------+

Everything else (``Valoricore``, ``AsyncValoricore``, ``ValoricoreAdapter``,
``LocalClient``) is an advanced wrapper or legacy alias — you do not need them
to get started.

Embedded quick-start (no server)::

    from valoricore import MemoryClient
    from valoricore.embeddings import SentenceTransformerEmbedder

    embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")  # pip install "valoricore[local]"
    db = MemoryClient(path="./my_db", dim=384)
    db.add_document(text="Hello world", embed=embedder)
    hits = db.semantic_search("Hello", embed=embedder, k=5)
    print(db.get_state_hash())   # 64-char BLAKE3 hex — reproducible on any machine

Remote quick-start (valori-node running on :3000)::

    from valoricore.remote import SyncRemoteClient

    db = SyncRemoteClient("http://localhost:3000")
    db.insert([0.1, 0.2, 0.3])
    hits = db.search([0.1, 0.2, 0.3], k=5)
    print(db.get_state_hash())
"""

from .local import LocalClient
from .remote import SyncRemoteClient, AsyncRemoteClient, ClusterClient, AsyncClusterClient
from .memory import MemoryClient
from .graph import Node, DocumentGraph
from .async_memory import AsyncMemoryClient
from .factory import Valoricore, AsyncValoricore
from .adapter import ValoricoreAdapter
from .integrations import ValoricoreLangChain, ValoricoreRetriever, ValoricoreLlamaIndex
from .exceptions import (
    ValoricoreError,
    IntegrityError,
    ValidationError,
    ConnectionError,
    NotFoundError,
    NotLeaderError,
    KernelError,
    TamperDetected,
)
from .verify import AnchorVerifier, TamperFinding, VerifyReport, verify_log
from .kinds import (
    NODE_RECORD,
    NODE_CONCEPT,
    NODE_AGENT,
    NODE_USER,
    NODE_TOOL,
    NODE_DOCUMENT,
    NODE_CHUNK,
    EDGE_RELATION,
    EDGE_FOLLOWS,
    EDGE_IN_EPISODE,
    EDGE_BY_AGENT,
    EDGE_MENTIONS,
    EDGE_REFERS_TO,
    EDGE_PARENT_OF,
)
from .types import Vector, FixedVector, Proof, StateHash, NodeId, RecordId, Metadata
from .ingest import load_text_from_file, chunk_text
from .chunking import split_by_sentences, naive_paragraph_chunker

# Cryptographic helpers (from compiled Rust FFI)
try:
    from .valoricore_ffi import ingest_embedding, generate_proof, verify_embedding
except ImportError:
    # Graceful degradation – allows importing the pure-Python SDK layer
    # without the compiled extension (useful for docs builds, type stubs, etc.)
    ingest_embedding = None   # type: ignore[assignment]
    generate_proof   = None   # type: ignore[assignment]
    verify_embedding = None   # type: ignore[assignment]

try:
    from importlib.metadata import version as _pkg_version
    __version__ = _pkg_version("valoricore")
except Exception:
    __version__ = "0.0.0"   # fallback: package not installed (editable dev, docs build)
__author__  = "Varshith Gudur"
__license__ = "MIT OR Apache-2.0"

__all__ = [
    # ── Factories ─────────────────────────────────────────────────
    "Valoricore",
    "AsyncValoricore",

    # ── High-level clients ─────────────────────────────────────────
    "MemoryClient",
    "AsyncMemoryClient",

    # ── High-level graph objects ───────────────────────────────────
    "Node",
    "DocumentGraph",

    # ── Base clients ───────────────────────────────────────────────
    "LocalClient",
    "SyncRemoteClient",
    "AsyncRemoteClient",
    "ClusterClient",
    "AsyncClusterClient",

    # ── Adapters ───────────────────────────────────────────────────
    "ValoricoreAdapter",

    # ── Framework integrations ─────────────────────────────────────
    "ValoricoreLangChain",
    "ValoricoreRetriever",
    "ValoricoreLlamaIndex",

    # ── Cryptographic helpers ──────────────────────────────────────
    "ingest_embedding",
    "generate_proof",
    "verify_embedding",

    # ── Exceptions ─────────────────────────────────────────────────
    "ValoricoreError",
    "IntegrityError",
    "TamperDetected",
    "ValidationError",
    "ConnectionError",
    "NotFoundError",
    "NotLeaderError",
    "KernelError",

    # ── Verification ───────────────────────────────────────────────
    "AnchorVerifier",
    "TamperFinding",
    "VerifyReport",
    "verify_log",

    # ── Node / Edge kind constants ─────────────────────────────────
    "NODE_RECORD",
    "NODE_CONCEPT",
    "NODE_AGENT",
    "NODE_USER",
    "NODE_TOOL",
    "NODE_DOCUMENT",
    "NODE_CHUNK",
    "EDGE_RELATION",
    "EDGE_FOLLOWS",
    "EDGE_IN_EPISODE",
    "EDGE_BY_AGENT",
    "EDGE_MENTIONS",
    "EDGE_REFERS_TO",
    "EDGE_PARENT_OF",

    # ── Type aliases ───────────────────────────────────────────────
    "Vector",
    "FixedVector",
    "Proof",
    "StateHash",
    "NodeId",
    "RecordId",
    "Metadata",

    # ── Ingest utilities ───────────────────────────────────────────
    "load_text_from_file",
    "chunk_text",
    "split_by_sentences",
    "naive_paragraph_chunker",

    # ── Package metadata ───────────────────────────────────────────
    "__version__",
    "__author__",
    "__license__",
]
