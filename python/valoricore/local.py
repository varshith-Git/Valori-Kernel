# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import List, Dict, Optional, Any, Tuple
import os
from .types import Vector, RecordId, NodeId, Proof, StateHash
from .exceptions import ValidationError, KernelError

# Try to import FFI module.
try:
    from . import valoricore_ffi as _ffi
except ImportError:
    try:
        import valoricore_ffi as _ffi
    except ImportError:
        _ffi = None

class LocalClient:
    """Synchronous FFI client for the embedded Valoricore Kernel."""
    
    def __init__(self, path: str = "./valoricore_db"):
        if _ffi is None:
             raise ImportError("Could not load 'valoricore_ffi' module. Ensure it is compiled and in PYTHONPATH.")
        try:
            self.kernel = _ffi.ValoricoreEngine(path)
        except Exception as e:
            raise KernelError(f"Failed to initialize Valoricore Kernel at {path}: {e}")

    def insert(self, vector: Vector, tag: int = 0) -> RecordId:
        """Insert a vector into the kernel."""
        try:
            return self.kernel.insert(vector, tag)
        except ValueError as e:
            raise ValidationError(str(e))

    def insert_with_proof(self, vector: Vector, tag: int = 0) -> Tuple[RecordId, Proof]:
        """Insert a vector and return its ID and binary Merkle proof."""
        try:
            rid, proof_hex = self.kernel.insert_with_proof(vector, tag)
            return rid, bytes.fromhex(proof_hex)
        except ValueError as e:
            raise ValidationError(str(e))

    def search(self, query: Vector, k: int, filter_tag: Optional[int] = None) -> List[Dict[str, Any]]:
        """Perform nearest neighbor search."""
        try:
            hits = self.kernel.search(query, k, filter_tag)
            return [{"id": h[0], "score": h[1]} for h in hits]
        except ValueError as e:
            raise ValidationError(str(e))

    def create_node(self, kind: int, record_id: Optional[int] = None) -> NodeId:
        return self.kernel.create_node(kind, record_id)

    def create_edge(self, from_id: int, to_id: int, kind: int) -> int:
        return self.kernel.create_edge(from_id, to_id, kind)

    def snapshot(self) -> bytes:
        return bytes(self.kernel.snapshot())

    def restore(self, data: bytes) -> None:
        try:
            self.kernel.restore(data)
        except Exception as e:
            raise KernelError(f"Failed to restore kernel state: {e}")
    
    def insert_batch(self, vectors: List[Vector]) -> List[RecordId]:
        return self.kernel.insert_batch(vectors)
    
    def insert_batch_with_proof(self, vectors: List[Vector], tags: Optional[List[int]] = None) -> List[Tuple[RecordId, Proof]]:
        if tags is None:
            tags = [0] * len(vectors)
        try:
            results = self.kernel.insert_batch_with_proof(vectors, tags)
            return [(r[0], bytes.fromhex(r[1])) for r in results]
        except ValueError as e:
            raise ValidationError(str(e))
    
    def get_metadata(self, record_id: int) -> Optional[bytes]:
        return self.kernel.get_metadata(record_id)
    
    def set_metadata(self, record_id: int, metadata: bytes) -> None:
        try:
            self.kernel.set_metadata(record_id, list(metadata))
        except ValueError as e:
            raise ValidationError(str(e))
    
    def get_state_hash(self) -> StateHash:
        """Returns the hex-encoded BLAKE3 root hash of the kernel state."""
        return self.kernel.get_state_hash()
    
    def record_count(self) -> int:
        return self.kernel.record_count()
    
    def soft_delete(self, record_id: int) -> None:
        self.kernel.soft_delete(record_id)

    def delete(self, record_id: int) -> None:
        """Permanently remove a record from the pool and the search index."""
        self.soft_delete(record_id)
