# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import List, Dict, Optional, Any
import sys
import os

# Try to import FFI module.
# In a real package, this would be installed.
# For local dev, we assume it's in the path or we hint the user.
try:
    import valori_ffi as _ffi 
except ImportError:
    try:
        import valori_ffi as _ffi
    except ImportError:
        _ffi = None

class LocalClient:
    def __init__(self, path: str = "./valori_db"):
        if _ffi is None:
             raise ImportError("Could not load 'valori_ffi' module. Ensure it is compiled and in PYTHONPATH.")
        self.kernel = _ffi.ValoriEngine(path)

    def insert(self, vector: List[float], tag: int = 0) -> int:
        return self.kernel.insert(vector, tag)

    def search(self, query: List[float], k: int, filter_tag: Optional[int] = None) -> List[Dict[str, Any]]:
        # FFI returns [(id, score), ...]
        hits = self.kernel.search(query, k, filter_tag)
        return [{"id": h[0], "score": h[1]} for h in hits]

    def create_node(self, kind: int, record_id: Optional[int] = None) -> int:
        return self.kernel.create_node(kind, record_id)

    def create_edge(self, from_id: int, to_id: int, kind: int) -> int:
        return self.kernel.create_edge(from_id, to_id, kind)

    def snapshot(self) -> bytes:
        return bytes(self.kernel.snapshot())

    def restore(self, data: bytes) -> None:
        self.kernel.restore(data)
    
    def insert_batch(self, vectors: List[List[float]]) -> List[int]:
        """Insert multiple vectors atomically.
        
        Args:
            vectors: List of vectors to insert
            
        Returns:
            List of assigned record IDs
            
        Example:
            >>> client = LocalClient()
            >>> vectors = [[0.1]*16, [0.2]*16, [0.3]*16]
            >>> ids = client.insert_batch(vectors)
            >>> print(ids)  # [0, 1, 2]
        """
        return self.kernel.insert_batch(vectors)
    
    def get_metadata(self, record_id: int) -> Optional[bytes]:
        """Get metadata for a record.
        
        Args:
            record_id: Record ID to query
            
        Returns:
            Metadata bytes or None if no metadata
            
        Example:
            >>> meta = client.get_metadata(5)
            >>> if meta:
            >>>     print(f"Metadata: {meta.decode()}")
        """
        return self.kernel.get_metadata(record_id)
    
    def set_metadata(self, record_id: int, metadata: bytes) -> None:
        """Set metadata for a record.
        
        Args:
            record_id: Record ID to update
            metadata: Metadata bytes (up to 64KB)
            
        Example:
            >>> client.set_metadata(5, b"user_id:12345")
            >>> meta = client.get_metadata(5)
            >>> print(meta)  # b"user_id:12345"
        """
        self.kernel.set_metadata(record_id, list(metadata))
    
    def get_state_hash(self) -> str:
        """Get cryptographic hash of current kernel state.
        
        Returns:
            Hex string of state hash (BLAKE3)
            
        Example:
            >>> hash_before = client.get_state_hash()
            >>> client.insert([0.1]*16)
            >>> hash_after = client.get_state_hash()
            >>> assert hash_before != hash_after
        """
        return self.kernel.get_state_hash()
    
    def record_count(self) -> int:
        """Get number of records in database.
        
        Returns:
            Total record count
            
        Example:
            >>> count = client.record_count()
            >>> print(f"Database has {count} records")
        """
        return self.kernel.record_count()
    
    def soft_delete(self, record_id: int) -> None:
        """Mark a record as deleted without removing it.
        
        Args:
            record_id: Record ID to delete
            
        Example:
            >>> client.soft_delete(5)
            >>> # Record 5 will be excluded from searches
        """
        self.kernel.soft_delete(record_id)
