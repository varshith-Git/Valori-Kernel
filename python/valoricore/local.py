# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import List, Dict, Optional, Any, Tuple
import os
import threading
from .types import Vector, RecordId, NodeId, Proof, StateHash
from .exceptions import ValidationError, KernelError

# H-1: Process-global lock that serialises the env-var mutation → engine-init →
# env-restore block.  Without this, two threads racing through LocalClient.__init__
# with different `dim` values will corrupt each other's VALORI_DIM before either
# ValoricoreEngine() call completes.
_INIT_LOCK = threading.Lock()

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

    def __init__(
        self,
        path: str = "./valoricore_db",
        index_kind: str = "bruteforce",
        max_records: int = 0,
        dim: int = 0,
        max_nodes: int = 0,
        max_edges: int = 0,
    ):
        """
        Args:
            path:        Database directory.
            index_kind:  ``"bruteforce"`` | ``"hnsw"``
            max_records: Vector pool capacity. Overrides ``VALORI_MAX_RECORDS``.
                         Defaults to the env var value (1024 if unset — too small
                         for production; always pass an explicit value).
            dim:         Vector dimension. Overrides ``VALORI_DIM``.
                         Must match the embedding model output (e.g. 384).
            max_nodes:   Knowledge Graph node capacity. Overrides ``VALORI_MAX_NODES``.
            max_edges:   Knowledge Graph edge capacity. Overrides ``VALORI_MAX_EDGES``.
        """
        if _ffi is None:
            raise ImportError(
                "Could not load 'valoricore_ffi' module. "
                "Ensure it is compiled and in PYTHONPATH."
            )
        # H-1 fix: hold the module-level lock for the entire env-mutate → init →
        # restore window.  This makes LocalClient.__init__ thread-safe when multiple
        # instances are created concurrently (e.g. in a FastAPI startup handler).
        # The lock is released even if ValoricoreEngine() raises.
        _vars: Dict[str, Optional[str]] = {}
        def _set_var(name: str, value: Optional[str]) -> None:
            _vars[name] = os.environ.get(name)  # save previous value
            if value is not None:
                os.environ[name] = value
            elif name in os.environ:
                del os.environ[name]

        try:
            with _INIT_LOCK:
                if max_records > 0:
                    _set_var("VALORI_MAX_RECORDS", str(max_records))
                if dim > 0:
                    _set_var("VALORI_DIM", str(dim))
                if max_nodes > 0:
                    _set_var("VALORI_MAX_NODES", str(max_nodes))
                if max_edges > 0:
                    _set_var("VALORI_MAX_EDGES", str(max_edges))
                try:
                    self.kernel = _ffi.ValoricoreEngine(path, index_kind)
                finally:
                    # Always restore env to its previous state so sibling threads
                    # and subsequent LocalClient() calls see an unmodified environment.
                    for name, prev in _vars.items():
                        if prev is None:
                            os.environ.pop(name, None)
                        else:
                            os.environ[name] = prev
            self.path = path
            self._auto_snapshot_interval = None
            self._insert_count = 0
        except KernelError:
            raise
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

    # ── High-level fluent graph API ────────────────────────────────────────────

    def node(self, kind: int, vector=None, tag: int = 0):
        """
        Create a graph node and return a :class:`~valoricore.graph.Node` object.

        If *vector* is provided the embedding is inserted first and the record
        is automatically linked — no manual ID juggling::

            chunk = db.node(NODE_CHUNK, vector=my_embedding)
            # replaces the old three-liner:
            #   rid = db.insert(my_embedding)
            #   nid = db.create_node(NODE_CHUNK, record_id=rid)
        """
        from . import graph as _g
        record_id = None
        if vector is not None:
            record_id = self.insert(vector, tag=tag)
        node_id = self.create_node(kind=kind, record_id=record_id)
        return _g.Node(node_id, kind, record_id, self)

    def edge(self, from_node, to_node, kind: int) -> int:
        """
        Create a directed edge. Accepts :class:`~valoricore.graph.Node` objects
        **or** raw integer IDs, so both styles work freely::

            db.edge(doc_node, chunk_node, EDGE_PARENT_OF)
            db.edge(3, 7, EDGE_REFERS_TO)   # raw ints still work
        """
        from . import graph as _g
        from_id = from_node.id if isinstance(from_node, _g.Node) else int(from_node)
        to_id   = to_node.id   if isinstance(to_node,   _g.Node) else int(to_node)
        return self.create_edge(from_id=from_id, to_id=to_id, kind=kind)

    def build_document(self, title=None):
        """
        Return a :class:`~valoricore.graph.DocumentGraph` context manager for
        building a document-root + chunk-nodes graph without any ID management::

            with db.build_document(title="My Article") as builder:
                for emb in embeddings:
                    builder.add_chunk(emb)

            doc  = builder.document    # root Node
            rids = builder.record_ids  # [0, 1, 2, …]
        """
        from . import graph as _g
        return _g.DocumentGraph(self, title=title)

    def delete_node(self, node_id: int) -> None:
        """Delete a graph node and all its incident edges (cascade)."""
        try:
            self.kernel.delete_node(node_id)
        except Exception as e:
            from .exceptions import KernelError
            raise KernelError(f"Failed to delete node {node_id}: {e}")

    def delete_edge(self, edge_id: int) -> None:
        """Delete a single graph edge by ID."""
        try:
            self.kernel.delete_edge(edge_id)
        except Exception as e:
            from .exceptions import KernelError
            raise KernelError(f"Failed to delete edge {edge_id}: {e}")

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

    # L-5: cap BFS depth to prevent unbounded memory/time on dense graphs.
    _MAX_WALK_DEPTH = 10

    def walk(self, start_node: int, max_depth: int = 2) -> List[int]:
        """
        Breadth-first search traversal of the knowledge graph.
        Returns a list of visited node IDs up to max_depth (capped at 10).
        """
        max_depth = min(max_depth, self._MAX_WALK_DEPTH)
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
        try:
            self.kernel.delete(record_id)
            self._check_auto_snapshot()
        except Exception as e:
            raise KernelError(f"Failed to delete record: {e}")

    def get_timeline(self) -> List[str]:
        """
        Reads the underlying events.log directly from the engine and returns a chronological
        list of all append-only state transitions.
        """
        try:
            return self.kernel.get_timeline()
        except Exception as e:
            raise KernelError(f"Failed to read timeline: {e}")
