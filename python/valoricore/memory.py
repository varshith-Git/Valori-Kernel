# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import Callable, List, Dict, Any, Optional
from .factory import Valoricore
from .kinds import (
    NODE_DOCUMENT, NODE_CHUNK, NODE_RECORD,
    EDGE_PARENT_OF, EDGE_REFERS_TO,
)
from .ingest import chunk_text
from .types import Vector, RecordId, NodeId, Proof, Metadata, StateHash
from .exceptions import ValidationError

EmbedFn = Callable[[str], Vector]

EXPECTED_DIM = 384  # must match kernel D

class MemoryClient:
    """High-level semantic memory API for document ingestion and Knowledge Graph management."""
    
    def __init__(
        self,
        path: str = "./valori_db",
        remote: Optional[str] = None,
        index_kind: str = "bruteforce",
        quantization: str = "none",
    ):
        """
        Wraps a Valoricore instance (local or remote).
        If remote is None -> Local (FFI).
        Else -> Remote (HTTP).

        Args:
            path:         Directory for the local embedded database. Only used
                          when ``remote`` is ``None``.
            remote:       HTTP URL of a standalone ``valori-node``
                          (e.g. ``"http://localhost:3000"``).
            index_kind:   Reserved for future indexing strategies.
            quantization: Reserved for future quantization options.
        """
        self._db = Valoricore(remote=remote, path=path)
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

    # ── Lifecycle ──────────────────────────────────────────────────────────

    def delete(self, record_id: int) -> None:
        """
        Permanently remove a record from the vector pool and search index.

        Args:
            record_id: The integer ID returned by ``insert`` or ``add_document``.
        """
        self._db.delete(record_id)

    def soft_delete(self, record_id: int) -> None:
        """
        Mark a record as inactive without physically removing it.
        The slot can be reused by future inserts.  The state hash will
        change to reflect the deletion.

        Args:
            record_id: The integer record ID to deactivate.
        """
        self._db.soft_delete(record_id)

    def record_count(self) -> int:
        """Return the total number of active records in the pool."""
        return self._db.record_count()

    # ── Snapshot / Audit ───────────────────────────────────────────────────

    def snapshot(self) -> bytes:
        """
        Serialize the full kernel state to a binary blob.

        Returns:
            Raw bytes that can be stored anywhere (disk, S3, Redis) and
            later passed to :meth:`restore`.
        """
        return self._db.snapshot()

    def restore(self, data: bytes) -> None:
        """
        Replace the current kernel state with a previously taken snapshot.

        Args:
            data: Binary snapshot bytes from :meth:`snapshot`.
        """
        self._db.restore(data)

    def get_state_hash(self) -> StateHash:
        """
        Returns the 64-character BLAKE3 hex digest of the entire kernel state.

        This hash is cryptographically stable: the same logical state always
        produces the same hash, regardless of machine architecture.

        Returns:
            64-character hex string (BLAKE3 root).
        """
        return self._db.get_state_hash()

    def get_timeline(self) -> List[str]:
        """
        Return a chronological list of every state transition as human-readable
        strings (parsed from the immutable append-only event log).

        Returns:
            List of event strings in insertion order.
        """
        return self._db.get_timeline()

    # ── Knowledge Graph ────────────────────────────────────────────────────

    def create_node(self, kind: int, record_id: Optional[int] = None) -> NodeId:
        """
        Create a Knowledge Graph node.

        Args:
            kind:      Integer node kind (see :mod:`valoricore.kinds`).
            record_id: Optional record ID to attach to this node.

        Returns:
            New node ID.
        """
        return self._db.create_node(kind=kind, record_id=record_id)

    def create_edge(self, from_id: int, to_id: int, kind: int) -> int:
        """
        Create a directed edge between two nodes.

        Args:
            from_id: Source node ID.
            to_id:   Target node ID.
            kind:    Integer edge kind (see :mod:`valoricore.kinds`).

        Returns:
            New edge ID.
        """
        return self._db.create_edge(from_id=from_id, to_id=to_id, kind=kind)

    def get_node(self, node_id: int) -> Optional[Dict[str, Any]]:
        """Fetch a node's ``kind`` and ``record_id``."""
        return self._db.get_node(node_id)

    def get_edges(self, node_id: int) -> List[Dict[str, Any]]:
        """Fetch all outgoing edges for a node."""
        return self._db.get_edges(node_id)

    def walk(self, start_node: int, max_depth: int = 2) -> List[int]:
        """
        Breadth-first traversal of the Knowledge Graph.

        Args:
            start_node: Node ID to start from.
            max_depth:  Maximum traversal depth.

        Returns:
            Ordered list of visited node IDs.
        """
        return self._db.walk(start_node, max_depth)

    def expand(self, start_node: int, max_depth: int = 2) -> List[int]:
        """
        Walk the graph and collect all unique record IDs found along the path.

        Args:
            start_node: Node ID to start from.
            max_depth:  Maximum traversal depth.

        Returns:
            List of unique record IDs reachable from ``start_node``.
        """
        return self._db.expand(start_node, max_depth)
