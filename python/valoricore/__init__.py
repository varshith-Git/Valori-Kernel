# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
valoricore – The Official Python SDK for Valori-Kernel
=======================================================

Quick-start::

    from valoricore import MemoryClient
    from valoricore.embeddings import SentenceTransformerEmbedder

    embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
    client   = MemoryClient(path="./my_db")

    result = client.add_document(text="Hello world", embed=embedder)
    hits   = client.semantic_search("Hello", embed=embedder, k=5)
    print(client.get_state_hash())   # 64-char BLAKE3 audit root
"""

from .local import LocalClient
from .remote import SyncRemoteClient, AsyncRemoteClient
from .memory import MemoryClient
from .async_memory import AsyncMemoryClient
from .factory import Valoricore, AsyncValoricore
from .adapter import ValoricoreAdapter
from .exceptions import (
    ValoricoreError,
    IntegrityError,
    ValidationError,
    ConnectionError,
    NotFoundError,
    KernelError,
)
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
    from valoricore_ffi import ingest_embedding, generate_proof, verify_embedding
except ImportError:
    # Graceful degradation – allows importing the pure-Python SDK layer
    # without the compiled extension (useful for docs builds, type stubs, etc.)
    ingest_embedding = None   # type: ignore[assignment]
    generate_proof   = None   # type: ignore[assignment]
    verify_embedding = None   # type: ignore[assignment]

__version__ = "0.1.2"
__author__  = "Varshith Gudur"
__license__ = "AGPL-3.0"

__all__ = [
    # ── Factories ─────────────────────────────────────────────────
    "Valoricore",
    "AsyncValoricore",

    # ── High-level clients ─────────────────────────────────────────
    "MemoryClient",
    "AsyncMemoryClient",

    # ── Base clients ───────────────────────────────────────────────
    "LocalClient",
    "SyncRemoteClient",
    "AsyncRemoteClient",

    # ── Adapters ───────────────────────────────────────────────────
    "ValoricoreAdapter",

    # ── Cryptographic helpers ──────────────────────────────────────
    "ingest_embedding",
    "generate_proof",
    "verify_embedding",

    # ── Exceptions ─────────────────────────────────────────────────
    "ValoricoreError",
    "IntegrityError",
    "ValidationError",
    "ConnectionError",
    "NotFoundError",
    "KernelError",

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
