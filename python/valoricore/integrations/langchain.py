# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
LangChain integration for Valori-Kernel.

Provides ValoricoreLangChain — a full LangChain VectorStore that works in both
embedded (local FFI, zero server) and remote (HTTP node) modes.

Install:
    pip install "valoricore[langchain]"

Usage:
    from valoricore.integrations import ValoricoreLangChain
    from langchain_openai import OpenAIEmbeddings

    store = ValoricoreLangChain(path="./db", embedding=OpenAIEmbeddings())
    store.add_texts(["Valoricore is deterministic.", "Fixed-point math rocks."])
    docs = store.similarity_search("What is Valoricore?", k=3)
"""

from __future__ import annotations

import json
import logging
from typing import Any, Callable, Iterable, List, Optional, Tuple, Type

from ..memory import MemoryClient

logger = logging.getLogger(__name__)

# ── Optional LangChain imports ────────────────────────────────────────────────

try:
    from langchain_core.documents import Document
    from langchain_core.embeddings import Embeddings
    from langchain_core.vectorstores import VectorStore
    from langchain_core.retrievers import BaseRetriever
    from langchain_core.callbacks.manager import (
        CallbackManagerForRetrieverRun,
        AsyncCallbackManagerForRetrieverRun,
    )
    _LANGCHAIN_AVAILABLE = True
except ImportError:
    try:
        # Older langchain (<0.1) fallback
        from langchain.schema import Document, BaseRetriever  # type: ignore[no-redef]
        from langchain.embeddings.base import Embeddings      # type: ignore[no-redef]
        from langchain.vectorstores.base import VectorStore   # type: ignore[no-redef]
        _LANGCHAIN_AVAILABLE = True
    except ImportError:
        _LANGCHAIN_AVAILABLE = False
        # Lightweight stubs so the module is importable without LangChain installed
        class VectorStore:          # type: ignore[no-redef]
            pass
        class BaseRetriever:        # type: ignore[no-redef]
            pass
        class Document:             # type: ignore[no-redef]
            def __init__(self, page_content: str = "", metadata: Optional[dict] = None):
                self.page_content = page_content
                self.metadata = metadata or {}
        class Embeddings:           # type: ignore[no-redef]
            pass

# Internal metadata key used to store the original text alongside user metadata.
# Prefixed to avoid collision with user-supplied keys.
_TEXT_KEY = "_valori_text"


def _require_langchain() -> None:
    if not _LANGCHAIN_AVAILABLE:
        raise ImportError(
            "LangChain is not installed. Run:\n"
            "    pip install \"valoricore[langchain]\"\n"
            "or:\n"
            "    pip install langchain-core"
        )


def _pack(text: str, meta: dict) -> bytes:
    """Serialize text + user metadata into bytes for Valoricore metadata store."""
    payload = {_TEXT_KEY: text, **meta}
    return json.dumps(payload, ensure_ascii=False).encode("utf-8")


def _unpack(raw: Optional[bytes]) -> Tuple[str, dict]:
    """Deserialize bytes back into (text, metadata_dict)."""
    if not raw:
        return "", {}
    try:
        data: dict = json.loads(raw.decode("utf-8"))
        text = data.pop(_TEXT_KEY, "")
        return text, data
    except Exception:
        return raw.decode("utf-8", errors="replace"), {}


def _normalize_hits(hits: Any) -> List[Tuple[int, float]]:
    """
    Normalize the search result format returned by both LocalClient and
    SyncRemoteClient into a consistent list of (record_id, score) pairs.

    LocalClient  returns: [{"id": int, "score": int}, ...]
    RemoteClient returns: [{"id": int, "score": float}, ...]  (or same)
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


# ── Main class ────────────────────────────────────────────────────────────────

class ValoricoreLangChain(VectorStore):
    """
    LangChain VectorStore backed by Valori-Kernel's deterministic integer engine.

    Works in **embedded** (no server, local FFI) and **remote** (HTTP node) modes
    with identical API — just swap the constructor argument.

    Key properties
    --------------
    - **Bit-identical results** across x86, ARM, WASM (Q16.16 fixed-point kernel)
    - **Cryptographic audit trail** — every state has a BLAKE3 root you can verify
    - **Crash-safe** — event-sourced persistence with fsync before live apply
    - **Drop-in** — implements the same interface as FAISS, Chroma, Pinecone

    Quick start
    -----------
    Local embedded (no server needed):

        from valoricore.integrations import ValoricoreLangChain
        from langchain_openai import OpenAIEmbeddings

        store = ValoricoreLangChain(
            path      = "./my_db",
            embedding = OpenAIEmbeddings(),
            index_kind = "hnsw",          # "bruteforce" | "hnsw" | "ivf"
        )
        store.add_texts(["Hello world", "Fixed-point is great"])
        docs = store.similarity_search("Hello", k=2)

    Remote HTTP node:

        store = ValoricoreLangChain(
            remote    = "http://my-valori-node:3000",
            embedding = OpenAIEmbeddings(),
        )

    From documents (standard LangChain factory pattern):

        from langchain.document_loaders import PyPDFLoader

        docs  = PyPDFLoader("report.pdf").load()
        store = ValoricoreLangChain.from_documents(docs, OpenAIEmbeddings(), path="./db")

    As LangChain retriever:

        retriever = store.as_retriever(search_kwargs={"k": 5})
        docs = retriever.get_relevant_documents("my query")
    """

    def __init__(
        self,
        embedding: "Embeddings",
        path: str = "./valori_db",
        remote: Optional[str] = None,
        index_kind: str = "bruteforce",
    ) -> None:
        """
        Args:
            embedding:  Any LangChain Embeddings object (OpenAI, Cohere,
                        SentenceTransformers, etc.).
            path:       Local database directory. Ignored when ``remote`` is set.
            remote:     URL of a standalone Valoricore node.  When provided,
                        uses HTTP instead of local FFI.
            index_kind: Vector index backend — ``"bruteforce"`` (default),
                        ``"hnsw"``, or ``"ivf"``.
        """
        _require_langchain()
        self._embedding  = embedding
        self._client     = MemoryClient(path=path, remote=remote, index_kind=index_kind)

    # ── LangChain VectorStore interface ──────────────────────────────────────

    @property
    def embeddings(self) -> "Embeddings":
        """Expose the underlying embeddings model (required by some LangChain chains)."""
        return self._embedding

    def add_texts(
        self,
        texts: Iterable[str],
        metadatas: Optional[List[dict]] = None,
        tags: Optional[List[int]] = None,
        **kwargs: Any,
    ) -> List[str]:
        """
        Embed and insert a sequence of texts.

        Args:
            texts:     Iterable of text strings.
            metadatas: Optional list of metadata dicts — one per text.
            tags:      Optional list of integer tags — one per text.
                       Use for tenant isolation or filtered search.
                       Defaults to 0 for all records when omitted.

        Returns:
            List of record IDs (as strings) in insertion order.
        """
        texts_list = list(texts)
        if not texts_list:
            return []
        metadatas = metadatas or [{} for _ in texts_list]
        tags      = tags      or [0] * len(texts_list)

        if len(tags) != len(texts_list):
            raise ValueError(
                f"len(tags)={len(tags)} must match len(texts)={len(texts_list)}"
            )

        # Batch embed (single round-trip to the embedding API)
        vectors = self._embedding.embed_documents(texts_list)

        # Insert each vector with its tag; collect record IDs
        record_ids: List[int] = []
        for vec, tag in zip(vectors, tags):
            rid = self._client._db.insert(vec, tag=tag)
            record_ids.append(rid)

        # Attach text + metadata to each record
        for record_id, text, meta in zip(record_ids, texts_list, metadatas):
            try:
                self._client.set_metadata(record_id, _pack(text, meta))
            except Exception as exc:
                logger.warning("set_metadata failed for record %s: %s", record_id, exc)

        return [str(rid) for rid in record_ids]

    def add_documents(
        self,
        documents: List["Document"],
        **kwargs: Any,
    ) -> List[str]:
        """
        Embed and insert LangChain Document objects.

        Args:
            documents: List of ``Document(page_content=..., metadata=...)`` objects.

        Returns:
            List of record IDs as strings.
        """
        texts = [doc.page_content for doc in documents]
        metas = [doc.metadata     for doc in documents]
        return self.add_texts(texts, metas, **kwargs)

    def similarity_search(
        self,
        query: str,
        k: int = 4,
        filter_tag: Optional[int] = None,
        **kwargs: Any,
    ) -> List["Document"]:
        """
        Return the ``k`` most similar documents to ``query``.

        Args:
            query:      Query string. Will be embedded with the configured
                        embeddings model.
            k:          Number of results to return.
            filter_tag: Optional integer tag to restrict search to a subset
                        of records (tenant isolation, per-user memory, etc.).

        Returns:
            List of ``Document`` objects, closest first.
        """
        return [doc for doc, _ in self.similarity_search_with_score(
            query, k, filter_tag=filter_tag, **kwargs
        )]

    def similarity_search_with_score(
        self,
        query: str,
        k: int = 4,
        filter_tag: Optional[int] = None,
        **kwargs: Any,
    ) -> List[Tuple["Document", float]]:
        """
        Return ``(Document, score)`` pairs, where **lower score = more similar**.

        Score is the raw Q16.16² L2 distance from the kernel — an integer-exact
        squared Euclidean distance.  It is hardware-independent and
        cryptographically stable.

        Args:
            query:      Query string.
            k:          Number of results.
            filter_tag: Optional tag filter.

        Returns:
            List of ``(Document, score)`` tuples in ascending score order.
        """
        q_vec = self._embedding.embed_query(query)
        return self._search_by_vector_with_score(q_vec, k=k, filter_tag=filter_tag)

    def similarity_search_by_vector(
        self,
        embedding: List[float],
        k: int = 4,
        filter_tag: Optional[int] = None,
        **kwargs: Any,
    ) -> List["Document"]:
        """
        Search using a **pre-computed** embedding vector.
        Useful when you have embeddings from an external source.

        Args:
            embedding:  List of floats (must match the dimension of stored vectors).
            k:          Number of results.
            filter_tag: Optional tag filter.

        Returns:
            List of Documents, closest first.
        """
        pairs = self._search_by_vector_with_score(embedding, k=k, filter_tag=filter_tag)
        return [doc for doc, _ in pairs]

    # ── Internal search helper ────────────────────────────────────────────────

    def _search_by_vector_with_score(
        self,
        vector: List[float],
        k: int,
        filter_tag: Optional[int] = None,
    ) -> List[Tuple["Document", float]]:
        raw_hits  = self._client._db.search(vector, k=k, filter_tag=filter_tag)
        hits      = _normalize_hits(raw_hits)
        results   = []
        for record_id, score in hits:
            raw        = self._client.get_metadata(record_id)
            text, meta = _unpack(raw)
            meta["_valori_record_id"] = record_id
            results.append((Document(page_content=text, metadata=meta), score))
        return results

    # ── Retriever ─────────────────────────────────────────────────────────────

    def as_retriever(self, **kwargs: Any) -> "ValoricoreRetriever":
        """
        Return a LangChain-compatible retriever.

        Args:
            **kwargs: Passed to ``ValoricoreRetriever``.  Key options:
                      - ``k``          (int, default 4): number of results
                      - ``filter_tag`` (int | None): tag filter

        Returns:
            A ``ValoricoreRetriever`` instance.

        Example::

            retriever = store.as_retriever(k=8, filter_tag=42)
            docs = retriever.get_relevant_documents("my query")
        """
        search_kwargs = kwargs.pop("search_kwargs", {})
        k          = kwargs.pop("k",          search_kwargs.pop("k",          4))
        filter_tag = kwargs.pop("filter_tag", search_kwargs.pop("filter_tag", None))
        return ValoricoreRetriever(vectorstore=self, k=k, filter_tag=filter_tag)

    # ── Class-method constructors ─────────────────────────────────────────────

    @classmethod
    def from_texts(
        cls,
        texts: List[str],
        embedding: "Embeddings",
        metadatas: Optional[List[dict]] = None,
        tags: Optional[List[int]] = None,
        path: str = "./valori_db",
        remote: Optional[str] = None,
        index_kind: str = "bruteforce",
        **kwargs: Any,
    ) -> "ValoricoreLangChain":
        """
        Create a store, insert texts, and return — the standard LangChain idiom.

        Example::

            store = ValoricoreLangChain.from_texts(
                texts     = ["doc1", "doc2"],
                embedding = OpenAIEmbeddings(),
                path      = "./db",
            )
        """
        store = cls(embedding=embedding, path=path, remote=remote, index_kind=index_kind)
        store.add_texts(texts, metadatas, tags=tags)
        return store

    @classmethod
    def from_documents(
        cls,
        documents: List["Document"],
        embedding: "Embeddings",
        path: str = "./valori_db",
        remote: Optional[str] = None,
        index_kind: str = "bruteforce",
        **kwargs: Any,
    ) -> "ValoricoreLangChain":
        """
        Create a store from LangChain Documents.

        Example::

            from langchain.document_loaders import PyPDFLoader
            docs  = PyPDFLoader("report.pdf").load()
            store = ValoricoreLangChain.from_documents(docs, embeddings, path="./db")
        """
        store = cls(embedding=embedding, path=path, remote=remote, index_kind=index_kind)
        store.add_documents(documents)
        return store

    # ── Valoricore-specific extras ─────────────────────────────────────────────

    def get_state_hash(self) -> str:
        """
        Return the 64-char BLAKE3 hex root of the kernel state.

        This hash is deterministic across machines and survives crash recovery.
        Store it alongside your embeddings to prove integrity at any future point.
        """
        return self._client.get_state_hash()

    def snapshot(self) -> bytes:
        """Serialize full kernel state to bytes for backup or transport."""
        return self._client.snapshot()

    def restore(self, data: bytes) -> None:
        """Replace current state with a previously taken snapshot (bit-exact restore)."""
        self._client.restore(data)


# ── Retriever ─────────────────────────────────────────────────────────────────

class ValoricoreRetriever(BaseRetriever):
    """
    LangChain BaseRetriever backed by ValoricoreLangChain.

    Returned by ``ValoricoreLangChain.as_retriever()``.  Can be used anywhere
    LangChain expects a retriever — RAG chains, agents, QA pipelines.

    Example::

        retriever = store.as_retriever(k=5, filter_tag=tenant_id)

        # In a RAG chain
        from langchain.chains import RetrievalQA
        from langchain_openai import ChatOpenAI

        chain = RetrievalQA.from_chain_type(
            llm       = ChatOpenAI(),
            retriever = retriever,
        )
        answer = chain.run("What is deterministic AI memory?")
    """

    def __init__(
        self,
        vectorstore: ValoricoreLangChain,
        k: int = 4,
        filter_tag: Optional[int] = None,
    ) -> None:
        self._vectorstore = vectorstore
        self._k           = k
        self._filter_tag  = filter_tag

    def _get_relevant_documents(
        self,
        query: str,
        *,
        run_manager: Optional[Any] = None,
    ) -> List["Document"]:
        return self._vectorstore.similarity_search(
            query,
            k=self._k,
            filter_tag=self._filter_tag,
        )

    # Sync alias (older LangChain compat)
    def get_relevant_documents(self, query: str) -> List["Document"]:
        return self._get_relevant_documents(query)

    async def _aget_relevant_documents(
        self,
        query: str,
        *,
        run_manager: Optional[Any] = None,
    ) -> List["Document"]:
        # No async kernel yet — run sync in the event loop
        return self._get_relevant_documents(query)

    async def aget_relevant_documents(self, query: str) -> List["Document"]:
        return await self._aget_relevant_documents(query)
