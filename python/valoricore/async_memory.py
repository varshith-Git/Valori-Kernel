# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
Async wrapper for the high-level MemoryClient.

All blocking FFI / HTTP calls are off-loaded to a thread-pool executor via
``asyncio.to_thread``.  A threading.Lock serialises concurrent access to the
synchronous Rust FFI to prevent data races in local mode.
"""

import asyncio
import threading
from typing import List, Dict, Optional, Any, Callable, Tuple
from .memory import MemoryClient, EmbedFn, EXPECTED_DIM
from .types import Vector, RecordId, NodeId, Proof, StateHash
from .exceptions import ValidationError


class AsyncMemoryClient:
    """
    Async high-level memory client — a complete async mirror of :class:`MemoryClient`.

    Uses ``asyncio.to_thread`` and a ``threading.Lock`` to safely interact with
    the synchronous Rust FFI kernel.  Suitable for use inside **FastAPI**,
    **Starlette**, and any other ``asyncio``-based framework.

    All methods on this class have identical semantics to their sync counterparts
    on :class:`MemoryClient`; they just return awaitables.

    Args:
        path:         Local database directory.  Ignored when ``remote`` is set.
        remote:       HTTP URL of a standalone ``valori-node``.  When set, all
                      operations are forwarded over the network.
        index_kind:   Vector index backend: ``"bruteforce"``, ``"hnsw"``, or ``"ivf"``.
        quantization: Reserved for future use.

    Example::

        import asyncio
        from valoricore import AsyncMemoryClient
        from valoricore.embeddings import SentenceTransformerEmbedder

        embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")

        async def main():
            client = AsyncMemoryClient(path="./my_db")

            result = await client.add_document(
                text  = "Valoricore is deterministic.",
                embed = embedder,
            )
            print(result["document_node_id"])

            hits = await client.semantic_search("What is Valoricore?", embed=embedder, k=3)
            for h in hits:
                print(h["id"], h["score"])

        asyncio.run(main())
    """

    def __init__(
        self,
        path: str = "./valori_db",
        remote: Optional[str] = None,
        index_kind: str = "bruteforce",
        quantization: str = "none",
    ) -> None:
        self._sync_client = MemoryClient(
            path=path,
            remote=remote,
            index_kind=index_kind,
            quantization=quantization,
        )
        # Serialises concurrent access to the synchronous Rust FFI
        self._lock = threading.Lock()

    # ── internal helper ────────────────────────────────────────────────────

    def _run_shielded(self, func: Callable, *args, **kwargs) -> Any:
        """
        Runs `func` inside a threading.Lock.

        Why? The Rust FFI engine is NOT thread-safe — concurrent Python threads
        can corrupt its internal state if they call into it simultaneously.
        This lock turns concurrent calls into a queue, so only one thread
        enters the Rust engine at a time.  Performance cost is minimal because
        the FFI calls are fast (microseconds); the lock is rarely contended.
        """
        with self._lock:
            return func(*args, **kwargs)

    async def _thread(self, func: Callable, *args, **kwargs) -> Any:
        """
        Off-loads a blocking sync call to the default thread-pool executor
        (managed by asyncio internally) and waits for the result.

        This keeps the asyncio event loop unblocked while the Rust FFI does its
        work in a background OS thread — essential for FastAPI / Starlette.
        """
        return await asyncio.to_thread(self._run_shielded, func, *args, **kwargs)

    # ── Document ingestion ─────────────────────────────────────────────────

    async def add_document(
        self,
        text: str,
        embed: EmbedFn,
        title: Optional[str] = None,
        doc_id: Optional[str] = None,
        chunk_size: int = 512,
    ) -> Dict[str, Any]:
        """Async version of :meth:`MemoryClient.add_document`."""
        return await self._thread(
            self._sync_client.add_document,
            text, embed, title, doc_id, chunk_size,
        )

    async def add_chunks(
        self,
        chunks: List[str],
        embed: EmbedFn,
        parent_document_node: Optional[int] = None,
        title: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Async version of :meth:`MemoryClient.add_chunks`."""
        return await self._thread(
            self._sync_client.add_chunks,
            chunks, embed, parent_document_node, title,
        )

    async def upsert_vector(
        self,
        vector: Vector,
        attach_to_document_node: Optional[int] = None,
    ) -> Dict[str, Any]:
        """Async version of :meth:`MemoryClient.upsert_vector`."""
        return await self._thread(
            self._sync_client.upsert_vector,
            vector, attach_to_document_node,
        )

    async def semantic_search(
        self,
        query: str,
        embed: EmbedFn,
        k: int = 5,
    ) -> List[Dict[str, Any]]:
        """Async version of :meth:`MemoryClient.semantic_search`."""
        return await self._thread(
            self._sync_client.semantic_search,
            query, embed, k,
        )

    # ── Lifecycle ──────────────────────────────────────────────────────────

    async def delete(self, record_id: int) -> None:
        """Async version of :meth:`MemoryClient.delete`."""
        await self._thread(self._sync_client.delete, record_id)

    async def soft_delete(self, record_id: int) -> None:
        """Async version of :meth:`MemoryClient.soft_delete`."""
        await self._thread(self._sync_client.soft_delete, record_id)

    async def record_count(self) -> int:
        """Async version of :meth:`MemoryClient.record_count`."""
        return await self._thread(self._sync_client.record_count)

    # ── Snapshot / Audit ───────────────────────────────────────────────────

    async def snapshot(self) -> bytes:
        """Async version of :meth:`MemoryClient.snapshot`."""
        return await self._thread(self._sync_client.snapshot)

    async def restore(self, data: bytes) -> None:
        """Async version of :meth:`MemoryClient.restore`."""
        await self._thread(self._sync_client.restore, data)

    async def get_state_hash(self) -> StateHash:
        """Async version of :meth:`MemoryClient.get_state_hash`."""
        return await self._thread(self._sync_client.get_state_hash)

    async def get_timeline(self) -> List[str]:
        """Async version of :meth:`MemoryClient.get_timeline`."""
        return await self._thread(self._sync_client.get_timeline)

    # ── Knowledge Graph ────────────────────────────────────────────────────

    async def create_node(self, kind: int, record_id: Optional[int] = None) -> NodeId:
        """Async version of :meth:`MemoryClient.create_node`."""
        return await self._thread(self._sync_client.create_node, kind, record_id)

    async def create_edge(self, from_id: int, to_id: int, kind: int) -> int:
        """Async version of :meth:`MemoryClient.create_edge`."""
        return await self._thread(self._sync_client.create_edge, from_id, to_id, kind)

    async def get_node(self, node_id: int) -> Optional[Dict[str, Any]]:
        """Async version of :meth:`MemoryClient.get_node`."""
        return await self._thread(self._sync_client.get_node, node_id)

    async def get_edges(self, node_id: int) -> List[Dict[str, Any]]:
        """Async version of :meth:`MemoryClient.get_edges`."""
        return await self._thread(self._sync_client.get_edges, node_id)

    async def walk(self, start_node: int, max_depth: int = 2) -> List[int]:
        """Async version of :meth:`MemoryClient.walk`."""
        return await self._thread(self._sync_client.walk, start_node, max_depth)

    async def expand(self, start_node: int, max_depth: int = 2) -> List[int]:
        """Async version of :meth:`MemoryClient.expand`."""
        return await self._thread(self._sync_client.expand, start_node, max_depth)

    # ── Batch operations ───────────────────────────────────────────────────

    async def insert_batch(self, vectors) -> list:
        """Async version of :meth:`MemoryClient.insert_batch`."""
        return await self._thread(self._sync_client.insert_batch, vectors)

    async def insert_batch_with_proof(self, vectors, tags=None) -> list:
        """Async version of :meth:`MemoryClient.insert_batch_with_proof`."""
        return await self._thread(
            self._sync_client.insert_batch_with_proof, vectors, tags
        )

    # ── Metadata ───────────────────────────────────────────────────────────

    async def get_metadata(self, record_id: int):
        """Async version of :meth:`MemoryClient.get_metadata`."""
        return await self._thread(self._sync_client.get_metadata, record_id)

    async def set_metadata(self, record_id: int, metadata: bytes) -> None:
        """Async version of :meth:`MemoryClient.set_metadata`."""
        await self._thread(self._sync_client.set_metadata, record_id, metadata)

    # ── High-level fluent graph API ────────────────────────────────────────

    async def node(self, kind: int, vector=None, tag: int = 0):
        """Async version of :meth:`MemoryClient.node`."""
        return await self._thread(
            self._sync_client.node, kind, vector, tag
        )

    async def edge(self, from_node, to_node, kind: int) -> int:
        """Async version of :meth:`MemoryClient.edge`."""
        return await self._thread(
            self._sync_client.edge, from_node, to_node, kind
        )

    def build_document(self, title=None):
        """
        Async-compatible version of :meth:`MemoryClient.build_document`.

        Returns a :class:`~valoricore.graph.DocumentGraph` context manager.
        ``DocumentGraph.__enter__`` / ``__exit__`` are synchronous but their
        operations hold the internal lock via the sync client — safe to use
        inside an ``async with`` block when holding the engine lock is
        acceptable for the duration of the build::

            async with client.build_document(title="My Doc") as builder:
                for emb in embeddings:
                    await asyncio.to_thread(builder.add_chunk, emb)

        For fully async chunk addition, use :meth:`node` and :meth:`edge`
        directly instead.
        """
        return self._sync_client.build_document(title=title)

    async def delete_node(self, node_id: int) -> None:
        """Async version of :meth:`MemoryClient.delete_node`."""
        await self._thread(self._sync_client.delete_node, node_id)

    async def delete_edge(self, edge_id: int) -> None:
        """Async version of :meth:`MemoryClient.delete_edge`."""
        await self._thread(self._sync_client.delete_edge, edge_id)

    # ── Cleanup ────────────────────────────────────────────────────────────

    async def close(self) -> None:
        """Release any held resources.  Safe to call multiple times."""
        pass  # No-op for local FFI; remote clients handle their own session lifecycle.

    async def __aenter__(self) -> "AsyncMemoryClient":
        return self

    async def __aexit__(self, *_: Any) -> None:
        await self.close()
