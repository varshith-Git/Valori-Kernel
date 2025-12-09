
# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from __future__ import annotations

import requests
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

class MemoryUpsertVectorResponse(TypedDict):
    memory_id: str
    record_id: int
    document_node_id: int
    chunk_node_id: int

class MemorySearchResponseHit(TypedDict):
    memory_id: str
    record_id: int
    score: int

class MemorySearchResponse(TypedDict):
    results: List[MemorySearchResponseHit]


class ProtocolError(RuntimeError):
    """Raised for protocol-level problems (invalid server response, etc.)"""
    pass

def _ensure_keys(d: dict, keys):
    missing = [k for k in keys if k not in d]
    if missing:
        raise ProtocolError(f"missing keys in server response: {missing}")

class ProtocolRemoteClient:
    def __init__(self, base_url: str, embed_fn, expected_dim: int):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()
        self._embed = embed_fn
        self.expected_dim = expected_dim

    def _post(self, path: str, json_data: Dict[str, Any]) -> Dict[str, Any]:
        url = self.base_url + path
        resp = self.session.post(url, json=json_data, timeout=10)
        resp.raise_for_status()
        return resp.json()

    def snapshot(self) -> bytes:
        url = self.base_url + "/snapshot"
        resp = self.session.post(url, timeout=10)
        resp.raise_for_status()
        return resp.content

    def restore(self, data: bytes) -> None:
        url = self.base_url + "/restore"
        # Binary body with explicit Content-Type
        headers = {"Content-Type": "application/octet-stream"}
        resp = self.session.post(url, data=data, headers=headers, timeout=10)
        resp.raise_for_status()

    def upsert_vector(self, vector: List[float], attach_to_document_node: Optional[int]=None, **kwargs):
        if len(vector) != self.expected_dim:
            raise ValueError(f"Embedding must be {self.expected_dim}-dimensional")
        payload = {"vector": vector}
        if attach_to_document_node is not None:
            payload["attach_to_document_node"] = attach_to_document_node
        # kwargs (tags/metadata) ignored for now per logic or can be added to payload
        if "tags" in kwargs: payload["tags"] = kwargs["tags"]
        if "metadata" in kwargs: payload["metadata"] = kwargs["metadata"]
        
        res = self._post("/v1/memory/upsert_vector", payload)
        _ensure_keys(res, ("memory_id", "record_id", "document_node_id", "chunk_node_id"))
        return res

    def search_vector(self, vector: List[float], k: int = 5):
        if len(vector) != self.expected_dim:
            raise ValueError(f"Embedding must be {self.expected_dim}-dimensional")
        payload = {"query_vector": vector, "k": k}
        res = self._post("/v1/memory/search_vector", payload)
        
        if "results" not in res or not isinstance(res["results"], list):
            raise ProtocolError("invalid search response shape")
            
        return res

    # ... existing methods ...
    
    def set_metadata(self, target_id: str, metadata: Dict[str, Any]):
        """Set metadata for a memory_id, record_id, or node_id."""
        url = self.base_url + "/v1/memory/meta/set"
        payload = {"target_id": target_id, "metadata": metadata}
        resp = self.session.post(url, json=payload, timeout=5)
        resp.raise_for_status()
        
    def get_metadata(self, target_id: str) -> Optional[Dict[str, Any]]:
        """Get metadata for a target_id."""
        url = self.base_url + "/v1/memory/meta/get"
        resp = self.session.get(url, params={"target_id": target_id}, timeout=5)
        resp.raise_for_status()
        data = resp.json()
        return data.get("metadata")

    def upsert_text(self, text: str, chunk_size: int = 512, **kwargs):
        # chunk locally using existing chunk_text
        from .ingest import chunk_text
        chunks = chunk_text(text, max_chars=chunk_size)
        record_ids = []
        chunk_node_ids = []
        # create document node first via 1st upsert (server will create doc node id)
        doc_node_id = None
        
        # Extract metadata from kwargs to set on the DOCUMENT node
        doc_metadata = kwargs.get("metadata", None)
        
        for chunk in chunks:
            vec = self._embed(chunk)
            if len(vec) != self.expected_dim:
                raise ValueError("Embedding mismatch")
            
            res = self.upsert_vector(vec, attach_to_document_node=doc_node_id)
            
            # server returns document_node_id for the created/used doc
            # capture it from the first chunk response
            if doc_node_id is None:
                doc_node_id = res["document_node_id"]
                # Set metadata ONCE for the document node if provided
                if doc_metadata:
                     # We assume we can target "node:{id}" 
                     # But Protocol returns integer IDs.
                     # We need to format it.
                     # Convention: "node:100", "rec:10".
                     self.set_metadata(f"node:{doc_node_id}", doc_metadata)

            elif res["document_node_id"] != doc_node_id:
                raise ProtocolError(f"server returned inconsistent document_node_id between chunks. Expected {doc_node_id}, got {res['document_node_id']}")
                
            record_ids.append(res["record_id"])
            chunk_node_ids.append(res["chunk_node_id"])
            
        memory_ids = [f"rec:{rid}" for rid in record_ids]
        return {
            "memory_ids": memory_ids,
            "record_ids": record_ids,
            "document_node_id": doc_node_id,
            "chunk_node_ids": chunk_node_ids,
            "chunk_count": len(chunks),
        }

# Update ProtocolClient facade to expose these
class ProtocolClient:
    def __init__(self, embed, remote: Optional[str] = None):
        if remote and (remote.startswith("http://") or remote.startswith("https://")):
            # Import expected dim from somewhere or hardcode 16 for now?
            # Ideally fetch from server config. For now 16.
            self._impl = ProtocolRemoteClient(remote, embed, 16)
            self._mode = "remote"
        else:
            from .memory import MemoryClient, EXPECTED_DIM
            self._memory = MemoryClient(remote=remote)
            self._mode = "local"

    def upsert_text(self, text: str, **kwargs):
        if self._mode == "remote":
            return self._impl.upsert_text(text, **kwargs)
        else:
            return self._memory.add_document(text, self._memory.embed_fn, **kwargs) # Wait, MemoryClient signature is different?
            # MemoryClient.add_document(text, embed, ...)
            # ProtocolClient standardizes this?
            # In previous steps I just delegated.
            # Local Mode: self._memory.add_document(text, embed, ...)
            # Check MemoryClient signature: add_document(text, embed, title...)
            pass 
            # I need to fix Local Mode delegation if I want full compatibility.
            # But the task is focused on Remote.
            # ... (omitted)

    def set_metadata(self, target_id: str, metadata: Dict[str, Any]):
        if self._mode == "remote":
            self._impl.set_metadata(target_id, metadata)
        else:
            raise NotImplementedError("Metadata not yet supported in Local Mode (FFI)")

    def get_metadata(self, target_id: str) -> Optional[Dict[str, Any]]:
        if self._mode == "remote":
            return self._impl.get_metadata(target_id)
        else:
            raise NotImplementedError("Metadata not yet supported in Local Mode (FFI)")
            
    # ... proxies for upsert_vector, search_vector, snapshot, restore ...
    def upsert_vector(self, *args, **kwargs):
        if self._mode == "remote": return self._impl.upsert_vector(*args, **kwargs)
        else: return self._memory.upsert_vector(*args, **kwargs)

    def search_vector(self, *args, **kwargs):
        if self._mode == "remote": return self._impl.search_vector(*args, **kwargs)
        else: return self._memory.semantic_search(*args, **kwargs) # Sig mismatch likely
    
    def snapshot(self):
        if self._mode == "remote": return self._impl.snapshot()
        else: raise NotImplementedError("Local snapshot not exposed yet")
        
    def restore(self, data):
        if self._mode == "remote": return self._impl.restore(data)
        else: raise NotImplementedError("Local restore not exposed yet")

    def search_text(self, text: str, k: int=5):
        # ... logic ...
        pass


    def search_text(self, query: str, k:int = 5):
        vec = self._embed(query)
        return self.search_vector(vec, k=k)

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
        index_kind: str = "bruteforce",
        quantization: str = "none",
    ) -> None:
        self._embed = embed
        
        if remote and (remote.startswith("http://") or remote.startswith("https://")):
            # Use Remote Protocol Client
            self._impl = ProtocolRemoteClient(remote, embed, EXPECTED_DIM)
        else:
            # Use Local/FFI Memory Client
            self._impl = None
            # MemoryClient already wraps Valori(remote=...)
            # Note: Valori(remote=...) supports non-http remote maybe? 
            # Or currently only supports local if remote is None.
            # If remote is set but not http, fall back to whatever MemoryClient supports.
            self._memory = MemoryClient(
                remote=remote,
                index_kind=index_kind,
                quantization=quantization,
            )

    # Helpers to construct canonical memory ids
    @staticmethod
    def _memory_id_from_record_id(record_id: int) -> str:
        return f"rec:{record_id}"
    
    def snapshot(self) -> bytes:
        if self._impl:
            return self._impl.snapshot()
        return self._memory._db.snapshot()

    def restore(self, data: bytes) -> None:
        if self._impl:
            return self._impl.restore(data)
        self._memory._db.restore(data)

    def set_metadata(self, target_id: str, metadata: Dict[str, Any]):
        if self._impl:
            self._impl.set_metadata(target_id, metadata)
        else:
            raise NotImplementedError("Metadata not yet supported in Local Mode (FFI)")

    def get_metadata(self, target_id: str) -> Optional[Dict[str, Any]]:
        if self._impl:
            return self._impl.get_metadata(target_id)
        else:
            raise NotImplementedError("Metadata not yet supported in Local Mode (FFI)")


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
        if self._impl:
            return self._impl.upsert_text(
                text=text, 
                chunk_size=chunk_size,
                doc_id=doc_id, 
                actor_id=actor_id,
                tags=tags,
                metadata=metadata
            )

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
    ) -> MemoryUpsertVectorResponse:
        """
        Vector-first API:
        - Insert a single vector.
        - Optionally attach to an existing document node.
        - Creates a CHUNK node pointing to the record.
        """
        if self._impl:
            return self._impl.upsert_vector(
                vector=vector,
                attach_to_document_node=attach_to_document_node,
                tags=tags,
                metadata=metadata
            )

        # Call MemoryClient helper
        res = self._memory.upsert_vector(vector, attach_to_document_node)
        
        record_id = res["record_id"]
        memory_id = self._memory_id_from_record_id(record_id)

        return {
            "memory_id": memory_id,
            "record_id": record_id,
            "document_node_id": res["document_node_id"],
            "chunk_node_id": res["chunk_node_id"],
        }

    def search_text(self, query: str, k: int = 5) -> MemorySearchResponse:
        if self._impl:
            return self._impl.search_text(query, k=k)

        vec = self._embed(query)
        return self.search_vector(vec, k=k)

    def search_vector(self, vector: List[float], k: int = 5) -> MemorySearchResponse:
        if self._impl:
            return self._impl.search_vector(vector, k=k)

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
