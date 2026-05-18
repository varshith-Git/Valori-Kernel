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
            self.path = path
            self._auto_snapshot_interval = None
            self._insert_count = 0
        except Exception as e:
            raise KernelError(f"Failed to initialize Valoricore Kernel at {path}: {e}")

    def _check_auto_snapshot(self, count: int = 1):
        """Internal helper to track inserts and auto-save snapshot if configured."""
        if self._auto_snapshot_interval:
            old_count = self._insert_count
            self._insert_count += count
            if (old_count // self._auto_snapshot_interval) < (self._insert_count // self._auto_snapshot_interval):
                snap_bytes = bytes(self.kernel.snapshot())
                os.makedirs(self.path, exist_ok=True)
                file_path = os.path.join(self.path, f"auto_snapshot_{self._insert_count}.snap")
                with open(file_path, "wb") as f:
                    f.write(snap_bytes)

    def insert(self, vector: Vector, tag: int = 0) -> RecordId:
        """Insert a vector into the kernel."""
        try:
            res = self.kernel.insert(vector, tag)
            self._check_auto_snapshot(1)
            return res
        except ValueError as e:
            raise ValidationError(str(e))

    def insert_with_proof(self, vector: Vector, tag: int = 0) -> Tuple[RecordId, Proof]:
        """Insert a vector and return its ID and binary Merkle proof."""
        try:
            rid, proof_hex = self.kernel.insert_with_proof(vector, tag)
            self._check_auto_snapshot(1)
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

    def get_node(self, node_id: int) -> Optional[Dict[str, Any]]:
        """Fetch node data (kind, record_id)."""
        res = self.kernel.get_node(node_id)
        if res is None:
            return None
        return {"kind": res[0], "record_id": res[1]}

    def get_edges(self, node_id: int) -> List[Dict[str, Any]]:
        """Fetch all outgoing edges for a given node."""
        raw_edges = self.kernel.get_edges(node_id)
        return [{"edge_id": e[0], "to_node": e[1], "kind": e[2]} for e in raw_edges]

    def neighbors(self, node_id: int) -> List[int]:
        """Return immediate neighbor node IDs for a given node."""
        return [e["to_node"] for e in self.get_edges(node_id)]

    def walk(self, start_node: int, max_depth: int = 2) -> List[int]:
        """
        Breadth-first search traversal of the knowledge graph.
        Returns a list of visited node IDs up to max_depth.
        """
        visited = set([start_node])
        queue = [(start_node, 0)]
        result = []
        
        while queue:
            current, depth = queue.pop(0)
            result.append(current)
            if depth >= max_depth:
                continue
                
            for edge in self.get_edges(current):
                nxt = edge["to_node"]
                if nxt not in visited:
                    visited.add(nxt)
                    queue.append((nxt, depth + 1))
                    
        return result

    def expand(self, start_node: int, max_depth: int = 2) -> List[int]:
        """
        Uses walk() to traverse the graph and returns all unique Record IDs
        found attached to any node in the traversal path.
        """
        visited_nodes = self.walk(start_node, max_depth)
        record_ids = set()
        
        for node_id in visited_nodes:
            n = self.get_node(node_id)
            if n and n["record_id"] is not None:
                record_ids.add(n["record_id"])
                
        return list(record_ids)

    def snapshot(self, auto_interval: Optional[int] = None) -> bytes:
        """
        Take a full snapshot of the memory engine.
        If auto_interval is provided (e.g. 1_000_000), configures the engine to 
        automatically take and save a snapshot to the database directory every N inserts.
        """
        if auto_interval is not None:
            self._auto_snapshot_interval = auto_interval
            self._insert_count = 0
            
        return bytes(self.kernel.snapshot())

    def restore(self, data: bytes) -> None:
        try:
            self.kernel.restore(data)
        except Exception as e:
            raise KernelError(f"Failed to restore kernel state: {e}")
    
    def insert_batch(self, vectors: List[Vector]) -> List[RecordId]:
        res = self.kernel.insert_batch(vectors)
        self._check_auto_snapshot(len(vectors))
        return res
    
    def insert_batch_with_proof(self, vectors: List[Vector], tags: Optional[List[int]] = None) -> List[Tuple[RecordId, Proof]]:
        if tags is None:
            tags = [0] * len(vectors)
        try:
            results = self.kernel.insert_batch_with_proof(vectors, tags)
            self._check_auto_snapshot(len(vectors))
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

    def get_timeline(self) -> List[str]:
        """
        Reads the underlying events.log directly from the engine and returns a chronological
        list of all append-only state transitions.
        """
        try:
            return self.kernel.get_timeline()
        except Exception as e:
            raise KernelError(f"Failed to read timeline: {e}")
