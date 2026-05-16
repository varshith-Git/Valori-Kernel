# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import requests
import warnings
from typing import List, Dict, Optional, Any, Tuple
from .types import Vector, RecordId, NodeId, Proof
from .exceptions import ConnectionError, ValidationError, NotFoundError

class SyncRemoteClient:
    """Synchronous REST client for a standalone Valoricore node."""
    
    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()
        self._auto_snapshot_interval = None
        self._insert_count = 0
        self._snapshot_dir = "./valoricore_snapshots"

    def _check_auto_snapshot(self, count: int = 1):
        if self._auto_snapshot_interval:
            old_count = self._insert_count
            self._insert_count += count
            if (old_count // self._auto_snapshot_interval) < (self._insert_count // self._auto_snapshot_interval):
                import os
                snap_bytes = self.snapshot()
                os.makedirs(self._snapshot_dir, exist_ok=True)
                file_path = os.path.join(self._snapshot_dir, f"auto_snapshot_{self._insert_count}.snap")
                with open(file_path, "wb") as f:
                    f.write(snap_bytes)

    def _post(self, path: str, json_data: Dict[str, Any]) -> Dict[str, Any]:
        url = self.base_url + path
        try:
            resp = self.session.post(url, json=json_data, timeout=10)
            if resp.status_code == 404:
                raise NotFoundError(f"Resource not found: {path}")
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to connect to Valoricore node at {url}: {e}")

    def insert(self, vector: Vector) -> RecordId:
        """Insert a vector record. Returns the new Record ID."""
        data = {"values": vector}
        resp = self._post("/records", data)
        self._check_auto_snapshot(1)
        return resp["id"]

    def insert_with_proof(self, vector: Vector) -> Tuple[RecordId, Proof]:
        """Insert a vector and return (id, proof_bytes)."""
        import valoricore
        fixed_vals = valoricore.ingest_embedding(vector)
        # Convert hex proof from generate_proof to bytes for consistency
        proof_hex = valoricore.generate_proof(fixed_vals)
        proof_bytes = bytes.fromhex(proof_hex)
        rid = self.insert(vector)
        return (rid, proof_bytes)

    def insert_batch(self, batch: List[Vector]) -> List[RecordId]:
        """Insert a batch of vectors. Returns list of new Record IDs."""
        data = {"batch": batch}
        resp = self._post("/v1/vectors/batch_insert", data)
        self._check_auto_snapshot(len(batch))
        return resp["ids"]

    def search(self, query: Vector, k: int) -> List[Dict[str, Any]]:
        """Search for nearest vectors. Returns list of hits [{'id': int, 'score': int}]."""
        data = {"query": query, "k": k}
        resp = self._post("/search", data)
        return resp["results"]

    def create_node(self, kind: int, record_id: Optional[int] = None) -> NodeId:
        """Create a graph node. Returns Node ID."""
        data = {"kind": kind, "record_id": record_id}
        resp = self._post("/graph/node", data)
        return resp["node_id"]

    def create_edge(self, from_id: int, to_id: int, kind: int) -> int:
        """Create a graph edge. Returns Edge ID."""
        data = {"from": from_id, "to": to_id, "kind": kind}
        resp = self._post("/graph/edge", data)
        return resp["edge_id"]

    def delete(self, record_id: int) -> None:
        """Permanently remove a record from the remote pool."""
        self._post("/v1/vectors/delete", {"id": record_id})

    def get_metadata(self, record_id: int) -> Optional[bytes]:
        """Retrieve metadata for a remote record."""
        url = f"{self.base_url}/v1/memory/meta/get?key=rec:{record_id}"
        try:
            resp = self.session.get(url, timeout=5)
            if resp.status_code == 404:
                return None
            resp.raise_for_status()
            data = resp.json()
            val = data.get("value")
            return val.encode() if val else None
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to retrieve metadata: {e}")

    def set_metadata(self, record_id: int, metadata: bytes) -> None:
        """Set metadata for a remote record."""
        data = {
            "key": f"rec:{record_id}",
            "value": metadata.decode(errors='replace')
        }
        self._post("/v1/memory/meta/set", data)

    def record_count(self) -> int:
        """Get the total record count from the remote node."""
        try:
            resp = self.session.get(f"{self.base_url}/health", timeout=5)
            resp.raise_for_status()
            return resp.json().get("record_count", 0)
        except requests.exceptions.RequestException:
            return 0

    def snapshot(self, auto_interval: Optional[int] = None, save_dir: str = "./valoricore_snapshots") -> bytes:
        """Download snapshot from remote."""
        if auto_interval is not None:
            self._auto_snapshot_interval = auto_interval
            self._insert_count = 0
            self._snapshot_dir = save_dir
            
        url = self.base_url + "/snapshot"
        try:
            resp = self.session.post(url, timeout=30)
            resp.raise_for_status()
            return resp.content
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to download snapshot: {e}")

    def restore(self, data: bytes) -> None:
        """Upload snapshot to remote."""
        url = self.base_url + "/restore"
        headers = {"Content-Type": "application/octet-stream"}
        try:
            resp = self.session.post(url, data=data, headers=headers, timeout=60)
            resp.raise_for_status()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to restore snapshot: {e}")

class AsyncRemoteClient:
    """Asynchronous REST client for a standalone Valoricore node using httpx."""
    
    def __init__(self, base_url: str):
        import httpx
        self.base_url = base_url.rstrip("/")
        self.client = httpx.AsyncClient(timeout=10.0)
        self._auto_snapshot_interval = None
        self._insert_count = 0
        self._snapshot_dir = "./valoricore_snapshots"

    async def _check_auto_snapshot(self, count: int = 1):
        if self._auto_snapshot_interval:
            old_count = self._insert_count
            self._insert_count += count
            if (old_count // self._auto_snapshot_interval) < (self._insert_count // self._auto_snapshot_interval):
                import os
                snap_bytes = await self.snapshot()
                os.makedirs(self._snapshot_dir, exist_ok=True)
                file_path = os.path.join(self._snapshot_dir, f"auto_snapshot_{self._insert_count}.snap")
                with open(file_path, "wb") as f:
                    f.write(snap_bytes)

    async def _post(self, path: str, json_data: Dict[str, Any]) -> Dict[str, Any]:
        url = self.base_url + path
        try:
            resp = await self.client.post(url, json=json_data)
            if resp.status_code == 404:
                raise NotFoundError(f"Resource not found: {path}")
            resp.raise_for_status()
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to connect to Valoricore node at {url}: {e}")

    async def insert(self, vector: Vector) -> RecordId:
        data = {"values": vector}
        resp = await self._post("/records", data)
        await self._check_auto_snapshot(1)
        return resp["id"]

    async def insert_with_proof(self, vector: Vector) -> Tuple[RecordId, Proof]:
        import valoricore
        fixed_vals = valoricore.ingest_embedding(vector)
        proof_hex = valoricore.generate_proof(fixed_vals)
        proof_bytes = bytes.fromhex(proof_hex)
        rid = await self.insert(vector)
        return (rid, proof_bytes)

    async def insert_batch(self, batch: List[Vector]) -> List[RecordId]:
        data = {"batch": batch}
        resp = await self._post("/v1/vectors/batch_insert", data)
        await self._check_auto_snapshot(len(batch))
        return resp["ids"]

    async def search(self, query: Vector, k: int) -> List[Dict[str, Any]]:
        data = {"query": query, "k": k}
        resp = await self._post("/search", data)
        return resp["results"]

    async def create_node(self, kind: int, record_id: Optional[int] = None) -> NodeId:
        data = {"kind": kind, "record_id": record_id}
        resp = await self._post("/graph/node", data)
        return resp["node_id"]

    async def create_edge(self, from_id: int, to_id: int, kind: int) -> int:
        data = {"from": from_id, "to": to_id, "kind": kind}
        resp = await self._post("/graph/edge", data)
        return resp["edge_id"]

    async def delete(self, record_id: int) -> None:
        await self._post("/v1/vectors/delete", {"id": record_id})

    async def get_metadata(self, record_id: int) -> Optional[bytes]:
        url = f"{self.base_url}/v1/memory/meta/get?key=rec:{record_id}"
        try:
            resp = await self.client.get(url)
            if resp.status_code == 404:
                return None
            resp.raise_for_status()
            data = resp.json()
            val = data.get("value")
            return val.encode() if val else None
        except Exception as e:
            raise ConnectionError(f"Failed to retrieve metadata: {e}")

    async def set_metadata(self, record_id: int, metadata: bytes) -> None:
        data = {
            "key": f"rec:{record_id}",
            "value": metadata.decode(errors='replace')
        }
        await self._post("/v1/memory/meta/set", data)

    async def record_count(self) -> int:
        try:
            resp = await self.client.get(f"{self.base_url}/health")
            resp.raise_for_status()
            return resp.json().get("record_count", 0)
        except Exception:
            return 0

    async def snapshot(self, auto_interval: Optional[int] = None, save_dir: str = "./valoricore_snapshots") -> bytes:
        if auto_interval is not None:
            self._auto_snapshot_interval = auto_interval
            self._insert_count = 0
            self._snapshot_dir = save_dir
            
        url = self.base_url + "/snapshot"
        try:
            resp = await self.client.post(url)
            resp.raise_for_status()
            return resp.content
        except Exception as e:
            raise ConnectionError(f"Failed to download snapshot: {e}")

    async def restore(self, data: bytes) -> None:
        url = self.base_url + "/restore"
        headers = {"Content-Type": "application/octet-stream"}
        try:
            resp = await self.client.post(url, content=data, headers=headers)
            resp.raise_for_status()
        except Exception as e:
            raise ConnectionError(f"Failed to restore snapshot: {e}")

    async def close(self):
        """Close the underlying httpx client."""
        await self.client.aclose()

# Backward Compatibility Alias
class RemoteClient(SyncRemoteClient):
    """Deprecated: Use SyncRemoteClient instead."""
    def __init__(self, *args, **kwargs):
        warnings.warn(
            "RemoteClient is deprecated and will be removed in a future version. Use SyncRemoteClient instead.",
            DeprecationWarning,
            stacklevel=2
        )
        super().__init__(*args, **kwargs)
