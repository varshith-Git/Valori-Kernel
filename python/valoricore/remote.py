# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import time
import requests
import warnings
from typing import List, Dict, Optional, Any, Tuple
from .types import Vector, RecordId, NodeId, Proof
from .exceptions import ConnectionError, ValidationError, NotFoundError, NotLeaderError


class _Retryable(Exception):
    """Internal marker for a transient cluster condition worth retrying."""
    pass


def _base_of(final_url: str, path: str) -> Optional[str]:
    """Strip a known request path off a resolved redirect URL to recover the
    leader's base URL (e.g. 'http://leader:3000/records' + '/records' ->
    'http://leader:3000'). Returns None if the path doesn't match."""
    if path and final_url.endswith(path):
        return final_url[: -len(path)]
    return None


class SyncRemoteClient:
    """Synchronous REST client for a Valoricore node — standalone or clustered.

    Against a Raft cluster, point ``base_url`` at *any* node. Reads
    (``search``, ``get_*``) are served locally on whichever node you hit;
    writes are transparently redirected to the current leader (HTTP 307),
    and the resolved leader is cached so subsequent writes skip the hop.
    During a leader election the client retries with backoff before raising
    :class:`NotLeaderError`.
    """

    def __init__(self, base_url: str, max_retries: int = 3, retry_backoff: float = 0.5):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()
        self._auto_snapshot_interval = None
        self._insert_count = 0
        self._snapshot_dir = "./valoricore_snapshots"
        # Cluster resilience knobs.
        self._max_retries = max_retries
        self._retry_backoff = retry_backoff
        # Cached leader base URL, learned from a 307 redirect. Writes prefer it.
        self._leader_url: Optional[str] = None

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
        """POST with cluster awareness.

        ``requests`` follows the leader's 307 redirect automatically (the POST
        body and method are preserved). We additionally (a) prefer a cached
        leader URL so the common case skips the redirect, (b) learn the leader
        from any redirect that did occur, and (c) retry on transient 503
        / connection errors during an election.
        """
        last_err: Optional[Exception] = None
        for attempt in range(self._max_retries + 1):
            base = self._leader_url or self.base_url
            url = base + path
            try:
                resp = self.session.post(url, json=json_data, timeout=10)

                # A 307 we did NOT auto-follow means the follower could not name
                # a leader (no Location header) — election in flight.
                if resp.status_code == 307:
                    self._leader_url = None
                    raise _Retryable("no leader to redirect to (307 without Location)")
                if resp.status_code == 503:
                    self._leader_url = None
                    raise _Retryable("node reports no leader (503)")
                if resp.status_code == 404:
                    raise NotFoundError(f"Resource not found: {path}")
                resp.raise_for_status()

                # Learn the leader if requests followed a redirect to get here.
                if resp.history:
                    self._leader_url = _base_of(resp.url, path)
                return resp.json()

            except (_Retryable, requests.exceptions.ConnectionError) as e:
                last_err = e
                # A cached leader may have failed over — drop it and retry base.
                self._leader_url = None
                if attempt < self._max_retries:
                    time.sleep(self._retry_backoff * (2 ** attempt))
                    continue
            except requests.exceptions.RequestException as e:
                raise ConnectionError(f"Failed to connect to Valoricore node at {url}: {e}")

        raise NotLeaderError(
            f"no leader available after {self._max_retries + 1} attempts to {self.base_url}{path}: {last_err}"
        )

    def insert(self, vector: Vector, tag: int = 0) -> RecordId:
        """Insert a vector record. Returns the new Record ID."""
        data = {"values": vector, "tag": tag}
        resp = self._post("/records", data)
        self._check_auto_snapshot(1)
        return resp["id"]

    def insert_with_proof(self, vector: Vector, tag: int = 0) -> Tuple[RecordId, Proof]:
        """Insert a vector and return (id, proof_bytes)."""
        import valoricore
        fixed_vals = valoricore.ingest_embedding(vector)
        proof_hex = valoricore.generate_proof(fixed_vals)
        proof_bytes = bytes.fromhex(proof_hex)
        rid = self.insert(vector, tag=tag)
        return (rid, proof_bytes)

    def insert_batch(self, batch: List[Vector]) -> List[RecordId]:
        """Insert a batch of vectors. Returns list of new Record IDs."""
        data = {"batch": batch}
        resp = self._post("/v1/vectors/batch_insert", data)
        self._check_auto_snapshot(len(batch))
        return resp["ids"]

    def insert_batch_with_proof(self, vectors: List[Vector], tags: Optional[List[int]] = None) -> List[Tuple[RecordId, Proof]]:
        """Insert a batch of vectors and return [(id, proof_bytes)] for each."""
        import valoricore
        if tags is None:
            tags = [0] * len(vectors)
        results = []
        for vector, tag in zip(vectors, tags):
            rid, proof = self.insert_with_proof(vector, tag=tag)
            results.append((rid, proof))
        self._check_auto_snapshot(len(vectors))
        return results

    def soft_delete(self, record_id: int) -> None:
        """Mark a record as inactive without physically removing it."""
        self._post("/v1/soft-delete", {"id": record_id})

    def search(self, query: Vector, k: int, filter_tag: Optional[int] = None) -> List[Dict[str, Any]]:
        """Search for nearest vectors. Returns list of hits [{'id': int, 'score': int}]."""
        data: Dict[str, Any] = {"query": query, "k": k}
        if filter_tag is not None:
            data["filter_tag"] = filter_tag
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

    def get_node(self, node_id: int) -> Optional[Dict[str, Any]]:
        """Fetch node data (kind, record_id)."""
        url = self.base_url + f"/graph/node/{node_id}"
        try:
            resp = self.session.get(url, timeout=5)
            if resp.status_code == 404:
                return None
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to retrieve node: {e}")

    def get_edges(self, node_id: int) -> List[Dict[str, Any]]:
        """Fetch all outgoing edges for a given node."""
        url = self.base_url + f"/graph/edges/{node_id}"
        try:
            resp = self.session.get(url, timeout=5)
            if resp.status_code == 404:
                return []
            resp.raise_for_status()
            return resp.json().get("edges", [])
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to retrieve edges: {e}")

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

    def delete(self, record_id: int) -> None:
        """Permanently remove a record from the remote pool."""
        self._post("/v1/delete", {"id": record_id})

    # ── Cluster operations ──────────────────────────────────────────────────

    def cluster_status(self) -> Dict[str, Any]:
        """Leadership, term, and member table from the node at ``base_url``.

        Works against any cluster node. Raises :class:`ConnectionError` if the
        node isn't running in cluster mode (the endpoint 404s on standalone).
        """
        url = self.base_url + "/v1/cluster/status"
        try:
            resp = self.session.get(url, timeout=5)
            if resp.status_code == 404:
                raise ConnectionError("node is not running in cluster mode (/v1/cluster/status not found)")
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to fetch cluster status from {url}: {e}")

    def cluster_health(self) -> bool:
        """True when the node sees an elected leader (HTTP 200 on /v1/cluster/health)."""
        url = self.base_url + "/v1/cluster/health"
        try:
            resp = self.session.get(url, timeout=5)
            return resp.status_code == 200
        except requests.exceptions.RequestException:
            return False

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
        """Download a binary snapshot of the remote engine state."""
        if auto_interval is not None:
            self._auto_snapshot_interval = auto_interval
            self._insert_count = 0
            self._snapshot_dir = save_dir

        url = self.base_url + "/v1/snapshot/download"
        try:
            resp = self.session.get(url, timeout=30)
            resp.raise_for_status()
            return resp.content
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to download snapshot: {e}")

    def restore(self, data: bytes) -> None:
        """Upload a binary snapshot to restore the remote engine state."""
        url = self.base_url + "/v1/snapshot/upload"
        headers = {"Content-Type": "application/octet-stream"}
        try:
            resp = self.session.post(url, data=data, headers=headers, timeout=60)
            resp.raise_for_status()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to restore snapshot: {e}")

    def get_state_hash(self) -> str:
        """Returns the hex-encoded BLAKE3 root hash of the kernel state."""
        url = self.base_url + "/v1/proof/state"
        try:
            resp = self.session.get(url, timeout=5)
            resp.raise_for_status()
            hash_array = resp.json()["final_state_hash"]
            if isinstance(hash_array, list):
                return bytes(hash_array).hex()
            return hash_array
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to retrieve state hash: {e}")

    def get_timeline(self) -> List[str]:
        """
        Reads the underlying events.log directly from the remote engine and returns a chronological
        list of all append-only state transitions.
        """
        url = self.base_url + "/timeline"
        try:
            resp = self.session.get(url, timeout=10)
            if resp.status_code == 404:
                raise NotFoundError("Timeline endpoint not found on remote node.")
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to fetch timeline from {url}: {e}")

class AsyncRemoteClient:
    """Asynchronous REST client for a standalone Valoricore node using httpx."""
    
    def __init__(self, base_url: str, max_retries: int = 3, retry_backoff: float = 0.5):
        import httpx
        self.base_url = base_url.rstrip("/")
        # follow_redirects=True is essential for clusters: writes to a follower
        # answer 307 + Location pointing at the leader. httpx does NOT follow
        # redirects by default, so without this every write to a non-leader fails.
        self.client = httpx.AsyncClient(timeout=10.0, follow_redirects=True)
        self._auto_snapshot_interval = None
        self._insert_count = 0
        self._snapshot_dir = "./valoricore_snapshots"
        self._max_retries = max_retries
        self._retry_backoff = retry_backoff
        self._leader_url: Optional[str] = None

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
        import asyncio
        import httpx
        last_err: Optional[Exception] = None
        for attempt in range(self._max_retries + 1):
            base = self._leader_url or self.base_url
            url = base + path
            try:
                resp = await self.client.post(url, json=json_data)
                if resp.status_code == 307:
                    self._leader_url = None
                    raise _Retryable("no leader to redirect to (307 without Location)")
                if resp.status_code == 503:
                    self._leader_url = None
                    raise _Retryable("node reports no leader (503)")
                if resp.status_code == 404:
                    raise NotFoundError(f"Resource not found: {path}")
                resp.raise_for_status()
                if resp.history:
                    self._leader_url = _base_of(str(resp.url), path)
                return resp.json()
            except (_Retryable, httpx.ConnectError) as e:
                last_err = e
                self._leader_url = None
                if attempt < self._max_retries:
                    await asyncio.sleep(self._retry_backoff * (2 ** attempt))
                    continue
            except httpx.HTTPError as e:
                raise ConnectionError(f"Failed to connect to Valoricore node at {url}: {e}")
        raise NotLeaderError(
            f"no leader available after {self._max_retries + 1} attempts to {self.base_url}{path}: {last_err}"
        )

    async def insert(self, vector: Vector, tag: int = 0) -> RecordId:
        data = {"values": vector, "tag": tag}
        resp = await self._post("/records", data)
        await self._check_auto_snapshot(1)
        return resp["id"]

    async def insert_with_proof(self, vector: Vector, tag: int = 0) -> Tuple[RecordId, Proof]:
        import valoricore
        fixed_vals = valoricore.ingest_embedding(vector)
        proof_hex = valoricore.generate_proof(fixed_vals)
        proof_bytes = bytes.fromhex(proof_hex)
        rid = await self.insert(vector, tag=tag)
        return (rid, proof_bytes)

    async def insert_batch(self, batch: List[Vector]) -> List[RecordId]:
        data = {"batch": batch}
        resp = await self._post("/v1/vectors/batch_insert", data)
        await self._check_auto_snapshot(len(batch))
        return resp["ids"]

    async def insert_batch_with_proof(self, vectors: List[Vector], tags: Optional[List[int]] = None) -> List[Tuple[RecordId, Proof]]:
        """Insert a batch of vectors and return [(id, proof_bytes)] for each."""
        import valoricore
        if tags is None:
            tags = [0] * len(vectors)
        results = []
        for vector, tag in zip(vectors, tags):
            rid, proof = await self.insert_with_proof(vector, tag=tag)
            results.append((rid, proof))
        await self._check_auto_snapshot(len(vectors))
        return results

    async def soft_delete(self, record_id: int) -> None:
        """Mark a record as inactive without physically removing it."""
        await self._post("/v1/soft-delete", {"id": record_id})

    async def search(self, query: Vector, k: int, filter_tag: Optional[int] = None) -> List[Dict[str, Any]]:
        data: Dict[str, Any] = {"query": query, "k": k}
        if filter_tag is not None:
            data["filter_tag"] = filter_tag
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

    async def get_node(self, node_id: int) -> Optional[Dict[str, Any]]:
        url = self.base_url + f"/graph/node/{node_id}"
        try:
            resp = await self.client.get(url)
            if resp.status_code == 404:
                return None
            resp.raise_for_status()
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to retrieve node: {e}")

    async def get_edges(self, node_id: int) -> List[Dict[str, Any]]:
        url = self.base_url + f"/graph/edges/{node_id}"
        try:
            resp = await self.client.get(url)
            if resp.status_code == 404:
                return []
            resp.raise_for_status()
            return resp.json().get("edges", [])
        except Exception as e:
            raise ConnectionError(f"Failed to retrieve edges: {e}")

    async def neighbors(self, node_id: int) -> List[int]:
        edges = await self.get_edges(node_id)
        return [e["to_node"] for e in edges]

    async def walk(self, start_node: int, max_depth: int = 2) -> List[int]:
        visited = set([start_node])
        queue = [(start_node, 0)]
        result = []
        
        while queue:
            current, depth = queue.pop(0)
            result.append(current)
            if depth >= max_depth:
                continue
                
            edges = await self.get_edges(current)
            for edge in edges:
                nxt = edge["to_node"]
                if nxt not in visited:
                    visited.add(nxt)
                    queue.append((nxt, depth + 1))
                    
        return result

    async def expand(self, start_node: int, max_depth: int = 2) -> List[int]:
        visited_nodes = await self.walk(start_node, max_depth)
        record_ids = set()
        
        for node_id in visited_nodes:
            n = await self.get_node(node_id)
            if n and n["record_id"] is not None:
                record_ids.add(n["record_id"])
                
        return list(record_ids)

    async def delete(self, record_id: int) -> None:
        await self._post("/v1/delete", {"id": record_id})

    # ── Cluster operations ──────────────────────────────────────────────────

    async def cluster_status(self) -> Dict[str, Any]:
        """Leadership, term, and member table from the node at ``base_url``."""
        import httpx
        url = self.base_url + "/v1/cluster/status"
        try:
            resp = await self.client.get(url)
            if resp.status_code == 404:
                raise ConnectionError("node is not running in cluster mode (/v1/cluster/status not found)")
            resp.raise_for_status()
            return resp.json()
        except httpx.HTTPError as e:
            raise ConnectionError(f"Failed to fetch cluster status from {url}: {e}")

    async def cluster_health(self) -> bool:
        """True when the node sees an elected leader (HTTP 200 on /v1/cluster/health)."""
        import httpx
        url = self.base_url + "/v1/cluster/health"
        try:
            resp = await self.client.get(url)
            return resp.status_code == 200
        except httpx.HTTPError:
            return False

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
        """Download a binary snapshot of the remote engine state."""
        if auto_interval is not None:
            self._auto_snapshot_interval = auto_interval
            self._insert_count = 0
            self._snapshot_dir = save_dir

        url = self.base_url + "/v1/snapshot/download"
        try:
            resp = await self.client.get(url)
            resp.raise_for_status()
            return resp.content
        except Exception as e:
            raise ConnectionError(f"Failed to download snapshot: {e}")

    async def restore(self, data: bytes) -> None:
        """Upload a binary snapshot to restore the remote engine state."""
        url = self.base_url + "/v1/snapshot/upload"
        headers = {"Content-Type": "application/octet-stream"}
        try:
            resp = await self.client.post(url, content=data, headers=headers)
            resp.raise_for_status()
        except Exception as e:
            raise ConnectionError(f"Failed to restore snapshot: {e}")

    async def get_state_hash(self) -> str:
        """Returns the hex-encoded BLAKE3 root hash of the kernel state."""
        url = self.base_url + "/v1/proof/state"
        try:
            resp = await self.client.get(url)
            resp.raise_for_status()
            hash_array = resp.json()["final_state_hash"]
            if isinstance(hash_array, list):
                return bytes(hash_array).hex()
            return hash_array
        except Exception as e:
            raise ConnectionError(f"Failed to retrieve state hash: {e}")

    async def get_timeline(self) -> List[str]:
        """
        Reads the underlying events.log directly from the remote engine and returns a chronological
        list of all append-only state transitions.
        """
        url = self.base_url + "/timeline"
        try:
            resp = await self.client.get(url)
            if resp.status_code == 404:
                raise NotFoundError("Timeline endpoint not found on remote node.")
            resp.raise_for_status()
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to fetch timeline from {url}: {e}")

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
