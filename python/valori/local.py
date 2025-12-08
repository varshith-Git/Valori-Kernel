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
    def __init__(self):
        if _ffi is None:
             raise ImportError("Could not load 'valori_ffi' module. Ensure it is compiled and in PYTHONPATH.")
        self.kernel = _ffi.PyKernel()

    def insert(self, vector: List[float]) -> int:
        return self.kernel.insert(vector)

    def search(self, query: List[float], k: int) -> List[Dict[str, Any]]:
        # FFI returns [(id, score), ...]
        hits = self.kernel.search(query, k)
        return [{"id": h[0], "score": h[1]} for h in hits]

    def create_node(self, kind: int, record_id: Optional[int] = None) -> int:
        return self.kernel.create_node(kind, record_id)

    def create_edge(self, from_id: int, to_id: int, kind: int) -> int:
        return self.kernel.create_edge(from_id, to_id, kind)

    def snapshot(self) -> bytes:
        return bytes(self.kernel.snapshot())

    def restore(self, data: bytes) -> None:
        self.kernel.restore(data)
