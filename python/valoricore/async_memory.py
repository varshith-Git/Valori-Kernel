# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import asyncio
import threading
from typing import List, Dict, Optional, Any, Callable, Tuple
from .memory import MemoryClient, EmbedFn, EXPECTED_DIM
from .types import Vector, RecordId, NodeId, Proof
from .exceptions import ValidationError

class AsyncMemoryClient:
    """
    Asynchronous wrapper for MemoryClient.
    Uses asyncio.to_thread and a threading.Lock to safely interact with the synchronous Rust FFI.
    """
    
    def __init__(
        self,
        remote: Optional[str] = None,
        index_kind: str = "bruteforce",
        quantization: str = "none",
    ):
        # We wrap a sync client
        self._sync_client = MemoryClient(remote=remote, index_kind=index_kind, quantization=quantization)
        # Thread lock to prevent concurrent FFI access in Local mode
        self._lock = threading.Lock()

    async def add_document(
        self,
        text: str,
        embed: EmbedFn,
        title: Optional[str] = None,
        doc_id: Optional[str] = None,
        chunk_size: int = 512,
    ) -> Dict[str, Any]:
        """Async version of add_document."""
        return await asyncio.to_thread(
            self._run_shielded,
            self._sync_client.add_document,
            text, embed, title, doc_id, chunk_size
        )

    async def semantic_search(
        self,
        query: str,
        embed: EmbedFn,
        k: int = 5,
    ) -> List[Dict[str, Any]]:
        """Async version of semantic_search."""
        return await asyncio.to_thread(
            self._run_shielded,
            self._sync_client.semantic_search,
            query, embed, k
        )

    async def upsert_vector(
        self,
        vector: Vector,
        attach_to_document_node: Optional[int] = None,
    ) -> Dict[str, Any]:
        """Async version of upsert_vector."""
        return await asyncio.to_thread(
            self._run_shielded,
            self._sync_client.upsert_vector,
            vector, attach_to_document_node
        )

    def _run_shielded(self, func: Callable, *args, **kwargs) -> Any:
        """Executes a function under the safety of the global FFI lock."""
        with self._lock:
            return func(*args, **kwargs)
            
    async def close(self):
        """Clean up resources."""
        # For remote clients, we might need to close sessions
        # For local, the lock is sufficient for cleanup on GC
        pass
