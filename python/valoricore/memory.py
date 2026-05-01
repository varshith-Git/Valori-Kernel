# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import Callable, List, Dict, Any, Optional
from .factory import Valoricore
from .kinds import (
    NODE_DOCUMENT, NODE_CHUNK, NODE_RECORD,
    EDGE_PARENT_OF, EDGE_REFERS_TO,
)
from .ingest import chunk_text
from .types import Vector, RecordId, NodeId, Proof, Metadata
from .exceptions import ValidationError

EmbedFn = Callable[[str], Vector]

EXPECTED_DIM = 384  # must match kernel D

class MemoryClient:
    """High-level semantic memory API for document ingestion and Knowledge Graph management."""
    
    def __init__(
        self,
        remote: Optional[str] = None,
        index_kind: str = "bruteforce",
        quantization: str = "none",
    ):
        """
        Wraps a Valoricore instance (local or remote).
        If remote is None -> Local (FFI).
        Else -> Remote (HTTP).
        """
        self._db = Valoricore(remote=remote)
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
        Split text into chunks, embed them, and link them to a document node.
        """
        chunks = chunk_text(text, max_chars=chunk_size)
        
        return self.add_chunks(
            chunks=chunks,
            embed=embed,
            parent_document_node=None,
            title=title
        )

    def add_chunks(
        self,
        chunks: List[str],
        embed: EmbedFn,
        parent_document_node: Optional[int] = None,
        title: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Lower-level API to register pre-chunked text."""
        chunk_node_ids = []
        record_ids = []
        proof_hashes = []
        
        if parent_document_node is None:
            doc_node_id = self._db.create_node(kind=NODE_DOCUMENT, record_id=None)
        else:
            doc_node_id = parent_document_node
            
        for chunk in chunks:
            vec = embed(chunk)
            rid, proof = self._db.insert_with_proof(vec)
            record_ids.append(rid)
            proof_hashes.append(proof.hex())
            
            cid = self._db.create_node(kind=NODE_CHUNK, record_id=rid)
            chunk_node_ids.append(cid)
            self._db.create_edge(from_id=doc_node_id, to_id=cid, kind=EDGE_PARENT_OF)
            
        return {
            "document_node_id": doc_node_id,
            "chunk_node_ids": chunk_node_ids,
            "record_ids": record_ids,
            "proof_hashes": proof_hashes,
            "title": title,
            "chunk_count": len(chunks)
        }

    def upsert_vector(
        self,
        vector: Vector,
        attach_to_document_node: Optional[int] = None,
    ) -> Dict[str, Any]:
        """Directly upsert a vector, optionally attaching to a doc node."""
        if len(vector) != EXPECTED_DIM:
            raise ValidationError(f"Embedding must be {EXPECTED_DIM}-dimensional, got {len(vector)}")

        rid, proof = self._db.insert_with_proof(vector)

        if attach_to_document_node is None:
            doc_node_id = self._db.create_node(kind=NODE_DOCUMENT, record_id=None)
        else:
            doc_node_id = attach_to_document_node
            
        chunk_node_id = self._db.create_node(kind=NODE_CHUNK, record_id=rid)
        self._db.create_edge(from_id=doc_node_id, to_id=chunk_node_id, kind=EDGE_PARENT_OF)
        
        return {
            "record_id": rid,
            "document_node_id": doc_node_id,
            "chunk_node_id": chunk_node_id,
            "proof_hash": proof.hex(),
        }

    def semantic_search(
        self,
        query: str,
        embed: EmbedFn,
        k: int = 5,
    ) -> List[Dict[str, Any]]:
        """Encode query string and perform nearest neighbor search."""
        vec = embed(query)
        if len(vec) != EXPECTED_DIM:
            raise ValidationError(f"Embedding must be {EXPECTED_DIM}-dimensional, got {len(vec)}")
            
        hits = self._db.search(vec, k=k)
        
        normalized_hits = []
        for hit in hits:
            if isinstance(hit, (list, tuple)):
                 rid, score = hit
            else:
                 rid = hit["id"]
                 score = hit["score"]
            normalized_hits.append({"id": rid, "score": score})
            
        return normalized_hits
