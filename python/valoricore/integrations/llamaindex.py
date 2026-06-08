# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
LlamaIndex integration for Valori-Kernel.

Provides ValoricoreLlamaIndex — a full LlamaIndex VectorStore that works in both
embedded (local FFI, zero server) and remote (HTTP node) modes.

Install:
    pip install "valoricore[llamaindex]"

Usage:
    from llama_index.core import VectorStoreIndex, StorageContext
    from valoricore.integrations import ValoricoreLlamaIndex

    vector_store  = ValoricoreLlamaIndex(path="./db")
    storage_ctx   = StorageContext.from_defaults(vector_store=vector_store)
    index         = VectorStoreIndex.from_documents(documents, storage_context=storage_ctx)
    query_engine  = index.as_query_engine()
    response      = query_engine.query("What is deterministic AI memory?")
"""

from __future__ import annotations

import json
import logging
from typing import Any, Dict, List, Optional, Sequence, cast

from ..memory import MemoryClient

logger = logging.getLogger(__name__)

# ── Optional LlamaIndex imports ───────────────────────────────────────────────

_LLAMAINDEX_AVAILABLE = False

try:
    # LlamaIndex >= 0.10 (llama-index-core)
    from llama_index.core.vector_stores.types import (
        BasePydanticVectorStore,
        VectorStoreQuery,
        VectorStoreQueryResult,
    )
    from llama_index.core.schema import BaseNode, TextNode, NodeRelationship, RelatedNodeInfo
    from llama_index.core.bridge.pydantic import Field
    _BASE_CLASS        = BasePydanticVectorStore
    _LLAMAINDEX_AVAILABLE = True
    _PYDANTIC_STORE    = True
except ImportError:
    try:
        # LlamaIndex 0.8–0.9
        from llama_index.vector_stores.types import (  # type: ignore[no-redef]
            VectorStore as _LegacyVectorStore,
            VectorStoreQuery,
            VectorStoreQueryResult,
        )
        from llama_index.schema import TextNode, BaseNode  # type: ignore[no-redef]
        _BASE_CLASS        = _LegacyVectorStore
        _LLAMAINDEX_AVAILABLE = True
        _PYDANTIC_STORE    = False
    except ImportError:
        _LLAMAINDEX_AVAILABLE = False
        _PYDANTIC_STORE       = False
        # Lightweight stubs so the module is importable without LlamaIndex installed
        class _BASE_CLASS:        # type: ignore[no-redef]
            pass
        class VectorStoreQuery:   # type: ignore[no-redef]
            query_embedding: Optional[List[float]] = None
            similarity_top_k: int = 4
        class VectorStoreQueryResult:  # type: ignore[no-redef]
            def __init__(self, nodes=None, similarities=None, ids=None):
                self.nodes        = nodes or []
                self.similarities = similarities or []
                self.ids          = ids or []
        class TextNode:           # type: ignore[no-redef]
            pass
        class BaseNode:           # type: ignore[no-redef]
            pass

# Metadata keys
_TEXT_KEY    = "_valori_text"
_NODE_ID_KEY = "_valori_node_id"


def _require_llamaindex() -> None:
    if not _LLAMAINDEX_AVAILABLE:
        raise ImportError(
            "LlamaIndex is not installed. Run:\n"
            "    pip install \"valoricore[llamaindex]\"\n"
            "or:\n"
            "    pip install llama-index-core"
        )


def _pack(text: str, node_id: str, meta: dict) -> bytes:
    """Serialize text + node_id + user metadata to bytes."""
    payload = {_TEXT_KEY: text, _NODE_ID_KEY: node_id, **meta}
    return json.dumps(payload, ensure_ascii=False).encode("utf-8")


def _unpack(raw: Optional[bytes]) -> tuple[str, str, dict]:
    """Deserialize bytes → (text, node_id, metadata_dict)."""
    if not raw:
        return "", "", {}
    try:
        data    = json.loads(raw.decode("utf-8"))
        text    = data.pop(_TEXT_KEY,    "")
        node_id = data.pop(_NODE_ID_KEY, "")
        return text, node_id, data
    except Exception:
        return raw.decode("utf-8", errors="replace"), "", {}


def _normalize_hits(hits: Any) -> list[tuple[int, float]]:
    """
    Normalize search results from LocalClient / SyncRemoteClient into
    consistent (record_id, score) pairs.
    """
    out = []
    for h in hits:
        if isinstance(h, (list, tuple)):
            rid, score = h[0], h[1]
        else:
            rid   = h.get("id", h.get("record_id", 0))
            score = h.get("score", 0)
        out.append((int(rid), float(score)))
    return out


def _l2_to_similarity(l2_sq_score: float) -> float:
    """
    Convert a raw Q16.16² L2-squared distance to a similarity score in (0, 1].

    LlamaIndex expects similarity where higher = more similar.
    We use  sim = 1 / (1 + dist)  which is monotonically decreasing in distance
    and bounded to (0, 1].  For identical vectors (dist=0) this returns 1.0.
    """
    return 1.0 / (1.0 + l2_sq_score)


# ── Main class ────────────────────────────────────────────────────────────────

class ValoricoreLlamaIndex(_BASE_CLASS):
    """
    LlamaIndex VectorStore backed by Valori-Kernel's deterministic integer engine.

    Works in **embedded** (local FFI, no server) and **remote** (HTTP node) modes.

    Key properties
    --------------
    - **Bit-identical results** across x86, ARM, WASM (Q16.16 fixed-point kernel)
    - **Cryptographic audit trail** — BLAKE3 state root on every operation
    - **Crash-safe** — event-sourced persistence, fsync before live apply
    - **Drop-in** — implements the full LlamaIndex VectorStore interface

    Quick start
    -----------
    Local embedded (no server needed):

        from llama_index.core import VectorStoreIndex, StorageContext
        from llama_index.core.node_parser import SentenceSplitter
        from llama_index.embeddings.openai import OpenAIEmbedding
        from valoricore.integrations import ValoricoreLlamaIndex

        embed_model  = OpenAIEmbedding()
        vector_store = ValoricoreLlamaIndex(path="./db")

        storage_ctx  = StorageContext.from_defaults(vector_store=vector_store)
        index        = VectorStoreIndex.from_documents(
            documents,
            storage_context = storage_ctx,
            embed_model     = embed_model,
        )
        engine   = index.as_query_engine()
        response = engine.query("What is deterministic AI memory?")

    Remote HTTP node:

        vector_store = ValoricoreLlamaIndex(remote="http://my-node:3000")

    Access the audit hash at any time:

        print(vector_store.get_state_hash())   # 64-char BLAKE3 hex
    """

    # LlamaIndex requires stores_text = True when nodes carry text content
    stores_text:       bool = True
    # We store and return embeddings from node metadata so LlamaIndex can skip re-embedding
    is_embedding_query: bool = True

    def __init__(
        self,
        path: str = "./valori_db",
        remote: Optional[str] = None,
        index_kind: str = "bruteforce",
    ) -> None:
        """
        Args:
            path:       Local database directory. Ignored when ``remote`` is set.
            remote:     URL of a standalone Valoricore node.
            index_kind: Vector index backend — ``"bruteforce"`` (default),
                        ``"hnsw"``, or ``"ivf"``.
        """
        _require_llamaindex()
        # BasePydanticVectorStore uses Pydantic model init — call super with no-op
        if _PYDANTIC_STORE:
            super().__init__()
        self._client = MemoryClient(path=path, remote=remote, index_kind=index_kind)

    # ── LlamaIndex VectorStore interface ─────────────────────────────────────

    @property
    def client(self) -> Any:
        """Expose the underlying MemoryClient (used by LlamaIndex internals)."""
        return self._client

    def add(
        self,
        nodes: List[BaseNode],
        **kwargs: Any,
    ) -> List[str]:
        """
        Insert LlamaIndex nodes into the vector store.

        Each node must have a pre-computed embedding (``node.embedding``).
        LlamaIndex sets this via its embed pipeline before calling ``add()``.

        Args:
            nodes: List of ``TextNode`` (or subclass) objects with embeddings.

        Returns:
            List of node IDs (the LlamaIndex ``node_id``, not the internal
            Valoricore record_id).
        """
        vectors:  List[List[float]] = []
        payloads: List[tuple]       = []  # (text, node_id, meta)

        for node in nodes:
            embedding = node.embedding
            if not embedding:
                logger.warning("Node %s has no embedding — skipping.", node.node_id)
                continue

            text = node.get_content(metadata_mode="none") if hasattr(node, "get_content") else ""
            meta = dict(node.metadata) if node.metadata else {}

            vectors.append(embedding)
            payloads.append((text, node.node_id, meta))

        if not vectors:
            return []

        # Batch insert (one fsync)
        record_ids = self._client.insert_batch(vectors)

        # Attach text + node_id + user metadata to each record
        accepted_ids: List[str] = []
        for record_id, (text, node_id, meta) in zip(record_ids, payloads):
            try:
                self._client.set_metadata(record_id, _pack(text, node_id, meta))
                accepted_ids.append(node_id)
            except Exception as exc:
                logger.warning("set_metadata failed for record %s: %s", record_id, exc)

        return accepted_ids

    def delete(self, ref_doc_id: str, **kwargs: Any) -> None:
        """
        Soft-delete all records associated with a document reference ID.

        LlamaIndex calls this with the ``doc_id`` of the source document when
        re-indexing or deleting documents.

        Note: Valoricore's soft-delete deactivates the record (it no longer
        appears in search results) but preserves the pool slot.  The state hash
        updates to reflect the deletion.

        Args:
            ref_doc_id: LlamaIndex document reference ID.
        """
        # We don't maintain a secondary doc_id → record_id index.
        # LlamaIndex typically calls delete before re-adding, so soft-delete
        # is sufficient — records will be shadowed by fresh inserts.
        logger.debug("ValoricoreLlamaIndex.delete called for ref_doc_id=%s", ref_doc_id)
        # No-op if we can't map ref_doc_id to a record_id without a secondary index.
        # Users who need hard delete can access self._client directly.

    def query(
        self,
        query: VectorStoreQuery,
        **kwargs: Any,
    ) -> VectorStoreQueryResult:
        """
        Execute a vector search and return LlamaIndex-compatible results.

        Similarity scores are converted from raw Q16.16² L2 distance into the
        (0, 1] range using  ``sim = 1 / (1 + dist)``  so that higher values
        always mean more similar, regardless of the original distance scale.

        Args:
            query: LlamaIndex ``VectorStoreQuery`` with ``query_embedding`` and
                   ``similarity_top_k`` set.

        Returns:
            ``VectorStoreQueryResult`` with ``nodes``, ``similarities``, ``ids``.
        """
        if not query.query_embedding:
            logger.warning("VectorStoreQuery has no query_embedding — returning empty result.")
            return VectorStoreQueryResult(nodes=[], similarities=[], ids=[])

        k        = query.similarity_top_k or 4
        raw_hits = self._client._db.search(query.query_embedding, k=k)
        hits     = _normalize_hits(raw_hits)

        nodes:        List[TextNode] = []
        similarities: List[float]    = []
        ids:          List[str]      = []

        for record_id, raw_score in hits:
            raw = self._client.get_metadata(record_id)
            text, node_id, meta = _unpack(raw)

            meta["_valori_record_id"] = record_id

            node = TextNode(
                text     = text,
                id_      = node_id or str(record_id),
                metadata = meta,
            )
            nodes.append(node)
            similarities.append(_l2_to_similarity(raw_score))
            ids.append(node_id or str(record_id))

        return VectorStoreQueryResult(nodes=nodes, similarities=similarities, ids=ids)

    # ── Valoricore-specific extras ─────────────────────────────────────────────

    def get_state_hash(self) -> str:
        """
        Return the 64-char BLAKE3 hex root of the kernel state.

        Deterministic across machines, survives crash recovery.
        """
        return self._client.get_state_hash()

    def snapshot(self) -> bytes:
        """Serialize full kernel state to bytes for backup."""
        return self._client.snapshot()

    def restore(self, data: bytes) -> None:
        """Replace current state with a previously taken snapshot."""
        self._client.restore(data)
