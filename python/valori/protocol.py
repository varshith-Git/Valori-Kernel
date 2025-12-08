from __future__ import annotations

from typing import Callable, List, Dict, Any, Optional, TypedDict

from . import Valori
from .memory import MemoryClient, EXPECTED_DIM
from .kinds import NODE_DOCUMENT, NODE_CHUNK, EDGE_PARENT_OF

EmbedFn = Callable[[str], List[float]]

class MemoryUpsertTextRequest(TypedDict, total=False):
    text: str
    doc_id: Optional[str]
    actor_id: Optional[str]
    tags: Optional[List[str]]
    metadata: Optional[Dict[str, Any]]

class MemoryUpsertResponse(TypedDict):
    memory_ids: List[str]
    record_ids: List[int]
    document_node_id: int
    chunk_node_ids: List[int]
    chunk_count: int

class MemorySearchResponseHit(TypedDict):
    memory_id: str
    record_id: int
    score: int

class MemorySearchResponse(TypedDict):
    results: List[MemorySearchResponseHit]

class ProtocolClient:
    """
    High-level Memory Protocol client.

    - If remote is None, uses local FFI kernel.
    - If remote is a URL, uses HTTP-backed node.
    - Uses a user-provided embed() function for text operations.
    """

    def __init__(
        self,
        embed: EmbedFn,
        remote: Optional[str] = None,
    ) -> None:
        self._embed = embed
        # MemoryClient already wraps Valori(remote=...)
        self._memory = MemoryClient(remote=remote)

    # Helpers to construct canonical memory ids
    @staticmethod
    def _memory_id_from_record_id(record_id: int) -> str:
        return f"rec:{record_id}"

    def upsert_text(
        self,
        text: str,
        *,
        doc_id: Optional[str] = None,
        actor_id: Optional[str] = None,
        tags: Optional[List[str]] = None,
        metadata: Optional[Dict[str, Any]] = None,
        chunk_size: int = 512,
    ) -> MemoryUpsertResponse:
        """
        Text-first API:
        - chunk text
        - embed each chunk
        - insert into Valori
        - create document + chunk nodes
        - link document -> chunk
        """
        # For v0 we ignore doc_id/actor_id/tags/metadata at kernel level,
        # but they are kept here for future host-layer storage.

        res = self._memory.add_document(
            text=text,
            embed=self._embed,
            title=doc_id, # Mapping doc_id to title for now as per MemoryClient api
            chunk_size=chunk_size,
        )

        record_ids = res["record_ids"]
        memory_ids = [self._memory_id_from_record_id(rid) for rid in record_ids]

        return {
            "memory_ids": memory_ids,
            "record_ids": record_ids,
            "document_node_id": res["document_node_id"],
            "chunk_node_ids": res["chunk_node_ids"],
            "chunk_count": res["chunk_count"],
        }

    def upsert_vector(
        self,
        vector: List[float],
        *,
        attach_to_document_node: Optional[int] = None,
        tags: Optional[List[str]] = None,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> MemoryUpsertResponse:
        """
        Vector-first API:
        - Insert a single vector.
        - Optionally attach to an existing document node.
        - Creates a CHUNK node pointing to the record.
        """
        # Validate dimension
        if len(vector) != EXPECTED_DIM:
            raise ValueError(f"Embedding must be {EXPECTED_DIM}-dimensional, got {len(vector)}")

        # Insert vector
        record_id = self._memory._db.insert(vector)  # _db is the underlying Valori client

        # Create or reuse document node
        if attach_to_document_node is None:
            doc_node_id = self._memory._db.create_node(kind=NODE_DOCUMENT, record_id=None)
        else:
            doc_node_id = attach_to_document_node

        # Create chunk node
        chunk_node_id = self._memory._db.create_node(kind=NODE_CHUNK, record_id=record_id)
        
        # Link doc -> chunk
        self._memory._db.create_edge(from_id=doc_node_id, to_id=chunk_node_id, kind=EDGE_PARENT_OF)

        memory_id = self._memory_id_from_record_id(record_id)

        return {
            "memory_ids": [memory_id],
            "record_ids": [record_id],
            "document_node_id": doc_node_id,
            "chunk_node_ids": [chunk_node_id],
            "chunk_count": 1,
        }

    def search_text(self, query: str, k: int = 5) -> MemorySearchResponse:
        vec = self._embed(query)
        return self.search_vector(vec, k=k)

    def search_vector(self, vector: List[float], k: int = 5) -> MemorySearchResponse:
        # Validate dimension
        if len(vector) != EXPECTED_DIM:
             raise ValueError(f"Embedding must be {EXPECTED_DIM}-dimensional, got {len(vector)}")
            
        # semantic_search in MemoryClient expects text and an embedder.
        # simpler to just call _db.search directly for pre-computed vectors.
        hits = self._memory._db.search(vector, k=k)

        # Normalization
        normalized = []
        for hit in hits:
            # Handle both dict (remote/client wrapper) and tuple (local FFI)
            if isinstance(hit, dict):
                rid = hit["id"]
                score = hit["score"]
            else:
                rid, score = hit
            
            normalized.append({
                "memory_id": self._memory_id_from_record_id(rid),
                "record_id": rid,
                "score": score,
            })
        return {"results": normalized}
