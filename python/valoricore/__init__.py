# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
valoricore - Python SDK for Valori

Pick one client
---------------
MemoryClient          -- embedded FFI, no server required
  from valoricore import MemoryClient

SyncRemoteClient      -- HTTP, blocking (requests)
  from valoricore.remote import SyncRemoteClient

AsyncRemoteClient     -- HTTP, async/await (httpx + asyncio / FastAPI)
  from valoricore.remote import AsyncRemoteClient

ClusterClient         -- 3/5-node Raft cluster, automatic leader failover
  from valoricore.remote import ClusterClient

Embedded quick-start (no server):

    from valoricore import MemoryClient
    from valoricore.embeddings import SentenceTransformerEmbedder

    embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
    db = MemoryClient(path="./my_db", dim=384)
    db.add_document(text="Hello world", embed=embedder)
    hits = db.semantic_search("Hello", embed=embedder, k=5)
    print(db.get_state_hash())   # 64-char BLAKE3 hex

Remote quick-start (valori-node on :3000):

    from valoricore.remote import SyncRemoteClient

    with SyncRemoteClient("http://localhost:3000") as db:
        db.insert([0.1, 0.2, 0.3])
        hits = db.search([0.1, 0.2, 0.3], k=5)
        print(db.get_state_hash())
"""

# Suppress urllib3's LibreSSL warning on stock macOS Python before any import
# that pulls in requests/urllib3.  The warning is cosmetic; LibreSSL works
# fine for TLS 1.2/1.3 — urllib3 added it as a nudge, not a functional error.
import warnings as _warnings
_warnings.filterwarnings(
    "ignore",
    message=r"urllib3 v2 only supports OpenSSL",
    module=r"urllib3",
)

from .local import LocalClient
from .remote import SyncRemoteClient, AsyncRemoteClient, ClusterClient, AsyncClusterClient
from .memory import MemoryClient
from .protocol import ProtocolClient, ProtocolRemoteClient
from .graph import Node, DocumentGraph
from .async_memory import AsyncMemoryClient
from .factory import Valoricore, AsyncValoricore
from .adapter import ValoricoreAdapter
from .integrations import ValoricoreLangChain, ValoricoreRetriever, ValoricoreLlamaIndex
from .exceptions import (
    ValoricoreError,
    AuthenticationError,
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
from .base import ValoriClient
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
    from importlib.metadata import version as _pkg_version, PackageNotFoundError as _PkgNFE
    __version__ = _pkg_version("valoricore")
except _PkgNFE:
    # Package is not registered (editable install not yet run, docs build, etc.)
    # "dev" is intentionally distinct from any real version so callers can detect it.
    __version__ = "dev"
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

    # ── Protocol clients ───────────────────────────────────────────
    "ProtocolClient",
    "ProtocolRemoteClient",

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
    "ValoriClient",

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
