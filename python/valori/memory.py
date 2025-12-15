# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import Callable, List, Dict, Any, Optional
from . import Valori
from .kinds import (
    NODE_DOCUMENT, NODE_CHUNK, NODE_RECORD,
    EDGE_PARENT_OF, EDGE_REFERS_TO,
)
from .ingest import chunk_text

EmbedFn = Callable[[str], List[float]]

EXPECTED_DIM = 16  # must match kernel D

class MemoryClient:
    def __init__(
        self,
        remote: Optional[str] = None,
        index_kind: str = "bruteforce",
        quantization: str = "none",
    ):
        """
        Wraps a Valori instance (local or remote).
        If remote is None -> Local (FFI).
        Else -> Remote (HTTP).
        """
        self._db = Valori(remote=remote)
        self._index_kind = index_kind
        self._quantization = quantization

    def add_document(
        self,
        text: str,
        embed: EmbedFn,
        title: Optional[str] = None,
        doc_id: Optional[str] = None,
        chunk_size: int = 512,
    ) -> Dict[str, Any]:
        """
        - Split text into chunks using `chunk_text`.
        - For each chunk:
            - Call embed(chunk) -> List[float] (must match D=16 length in our tests)
            - Insert vector via self._db.insert(...) -> record_id
            - Create a CHUNK node pointing to that record (NodeKind=NODE_CHUNK, record_id=record_id)
        - Create a DOCUMENT node (NodeKind=NODE_DOCUMENT, record_id=None)
        - Link DOCUMENT -> CHUNK nodes via EDGE_PARENT_OF
        - Return a dict with IDs.
        """
        
        # 1. Chunking
        chunks = chunk_text(text, max_chars=chunk_size)
        
        return self.add_chunks(
            chunks=chunks,
            embed=embed,
            parent_document_node=None, # Will create new doc node
            title=title
        )

    def add_chunks(
        self,
        chunks: List[str],
        embed: EmbedFn,
        parent_document_node: Optional[int] = None,
        title: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Lower-level API to register pre-chunked text.
        """
        
        chunk_node_ids = []
        record_ids = []
        
        # 1. Create Document Node if needed
        if parent_document_node is None:
            # We don't have a record_id for the document itself in this simple model (it's pure metadata node)
            doc_node_id = self._db.create_node(kind=NODE_DOCUMENT, record_id=None)
        else:
            doc_node_id = parent_document_node
            
        # 2. Process chunks
        for chunk in chunks:
            # Embed
            vec = embed(chunk)
            if len(vec) != EXPECTED_DIM:
                raise ValueError(f"Embedding function must return {EXPECTED_DIM} dims, got {len(vec)}")
            
            # Kernel insert
            rid = self._db.insert(vec)
            record_ids.append(rid)
            
            # Create Chunk Node
            cid = self._db.create_node(kind=NODE_CHUNK, record_id=rid)
            chunk_node_ids.append(cid)
            
            # Link Doc -> Chunk (ParentOf)
            self._db.create_edge(from_id=doc_node_id, to_id=cid, kind=EDGE_PARENT_OF)
            
        return {
            "document_node_id": doc_node_id,
            "chunk_node_ids": chunk_node_ids,
            "record_ids": record_ids,
            "title": title,
            "chunk_count": len(chunks)
        }

    def upsert_vector(
        self,
        vector: List[float],
        attach_to_document_node: Optional[int] = None,
    ) -> Dict[str, Any]:
        """
        Directly upsert a vector, optionally attaching to a doc node.
        Returns singular dict of IDs.
        """
        if len(vector) != EXPECTED_DIM:
            raise ValueError(f"Embedding must be {EXPECTED_DIM}-dimensional, got {len(vector)}")

        # Insert vector
        rid = self._db.insert(vector)

        # Doc node
        if attach_to_document_node is None:
            doc_node_id = self._db.create_node(kind=NODE_DOCUMENT, record_id=None)
        else:
            doc_node_id = attach_to_document_node
            
        # Chunk node
        chunk_node_id = self._db.create_node(kind=NODE_CHUNK, record_id=rid)
        
        # Link
        self._db.create_edge(from_id=doc_node_id, to_id=chunk_node_id, kind=EDGE_PARENT_OF)
        
        return {
            "record_id": rid,
            "document_node_id": doc_node_id,
            "chunk_node_id": chunk_node_id,
        }

    def semantic_search(
        self,
        query: str,
        embed: EmbedFn,
        k: int = 5,
    ) -> List[Dict[str, Any]]:
        """
        - Compute embedding = embed(query) -> List[float]
        - Call self._db.search(vector, k)
        - Return list of { "id": record_id, "score": score }
        """
        vec = embed(query)
        if len(vec) != EXPECTED_DIM:
            raise ValueError(f"Embedding must be {EXPECTED_DIM}-dimensional, got {len(vec)}")
        hits = self._db.search(vec, k=k)
        
        # hits is list of (id, score) tuples or dicts depending on client implementation?
        # Let's check Python wrapper impl.
        # LocalClient returns [(id, score), ...] tuples from FFI.
        # RemoteClient returns dicts? Let's assume standard behavior or normalize.
        # The prompt says: "search(query: Vec<f32>, k: usize) -> Vec<(u32, i64)>" for kernel.
        # Python wrapper likely returns list of tuples.
        
        normalized_hits = []
        for hit in hits:
            # Handle both dict (remote) and tuple (local) if they differ, 
            # though Valori factory attempts unified behavior.
            # LocalClient wraps FFI directly, so it returns tuples [(id, score)].
            if isinstance(hit, (list, tuple)):
                 rid, score = hit
            else:
                 rid = hit["id"]
                 score = hit["score"]
            
            normalized_hits.append({"id": rid, "score": score})
            
        return normalized_hits
