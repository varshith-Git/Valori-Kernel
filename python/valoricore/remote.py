# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
import time
import re
import warnings
from collections import deque
from typing import List, Dict, Optional, Any, Tuple
from uuid import uuid4
import requests

from .base import ValoriClient
from .types import Vector, RecordId, NodeId, Proof
from .exceptions import (
    AuthenticationError, ConnectionError, ValidationError,
    NotFoundError, NotLeaderError,
)


class _Retryable(Exception):
    """Internal marker for a transient cluster condition worth retrying."""


def _raise_for_status(resp, path: str = "") -> None:
    if resp.status_code in (401, 403):
        action = "set token=" if resp.status_code == 401 else "check token permissions for"
        raise AuthenticationError(
            f"[HTTP {resp.status_code}] Authentication failed{' for ' + path if path else ''} — "
            f"{action} this operation. "
            f"Pass token= to the client or set VALORI_AUTH_TOKEN on the node."
        )
    resp.raise_for_status()


def _base_of(final_url: str, path: str) -> Optional[str]:
    if path and final_url.endswith(path):
        return final_url[: -len(path)]
    return None


class _BearerAuth(requests.auth.AuthBase):
    """Per-request auth injector that redacts itself in repr/tracebacks."""
    def __init__(self, token: str) -> None:
        self._token = token

    def __call__(self, r: requests.PreparedRequest) -> requests.PreparedRequest:
        r.headers["Authorization"] = f"Bearer {self._token}"
        return r

    def __repr__(self) -> str:
        return "<BearerAuth [REDACTED]>"


# ── Transport layer (DIP) ────────────────────────────────────────────────────

class _SyncTransport:
    """Cluster-aware synchronous HTTP transport.

    Encapsulates the requests.Session, bearer auth, timeout defaults, and the
    cluster-aware POST-with-retry loop. Injected into SyncRemoteClient so tests
    can swap in a fake transport without touching the domain logic.
    """

    def __init__(
        self,
        base_url: str,
        auth: Optional[_BearerAuth],
        timeout: int,
        max_retries: int,
        retry_backoff: float,
    ) -> None:
        self.base_url = base_url
        self._timeout = timeout
        self._max_retries = max_retries
        self._retry_backoff = retry_backoff
        self._leader_url: Optional[str] = None
        self._session = requests.Session()
        self._session.auth = auth

    # ── Low-level verbs ───────────────────────────────────────────────────────

    def get(self, url: str, **kw) -> requests.Response:
        kw.setdefault("timeout", self._timeout)
        return self._session.get(url, **kw)

    def post(self, url: str, **kw) -> requests.Response:
        kw.setdefault("timeout", self._timeout)
        return self._session.post(url, **kw)

    def patch(self, url: str, **kw) -> requests.Response:
        kw.setdefault("timeout", self._timeout)
        return self._session.patch(url, **kw)

    def delete(self, url: str, **kw) -> requests.Response:
        kw.setdefault("timeout", self._timeout)
        return self._session.delete(url, **kw)

    # ── Cluster-aware RPC ─────────────────────────────────────────────────────

    def post_rpc(
        self,
        path: str,
        json_data: Dict[str, Any],
        idempotency_key: Optional[bytes] = None,
    ) -> Dict[str, Any]:
        """POST with leader discovery, idempotency, and retry on transient errors."""
        if idempotency_key is not None:
            json_data = {**json_data, "request_id": list(idempotency_key)}
        last_err: Optional[Exception] = None
        for attempt in range(self._max_retries + 1):
            base = self._leader_url or self.base_url
            url = base + path
            try:
                resp = self._session.post(url, json=json_data, timeout=self._timeout)
                if resp.status_code == 307:
                    self._leader_url = None
                    raise _Retryable("no leader to redirect to (307 without Location)")
                if resp.status_code == 503:
                    self._leader_url = None
                    raise _Retryable("node reports no leader (503)")
                if resp.status_code == 404:
                    raise NotFoundError(f"Resource not found: {path}")
                if resp.status_code in (401, 403):
                    action = "set token=" if resp.status_code == 401 else "check token permissions for"
                    raise AuthenticationError(
                        f"[HTTP {resp.status_code}] Authentication failed for {path} — "
                        f"{action} this operation. "
                        f"Pass token= to the client or set VALORI_AUTH_TOKEN on the node."
                    )
                if resp.status_code in (400, 409, 413, 422):
                    try:
                        detail = resp.json().get("error") or resp.text
                    except Exception:
                        detail = resp.text
                    raise ValidationError(f"[HTTP {resp.status_code}] {detail}")
                _raise_for_status(resp)
                if resp.history:
                    self._leader_url = _base_of(resp.url, path)
                return resp.json()
            except (_Retryable, requests.exceptions.ConnectionError) as e:
                last_err = e
                self._leader_url = None
                if attempt < self._max_retries:
                    time.sleep(self._retry_backoff * (2 ** attempt))
                    continue
            except (AuthenticationError, ValidationError, NotFoundError):
                raise
            except requests.exceptions.RequestException as e:
                raise ConnectionError(f"Failed to connect to Valoricore node at {url}: {e}")
        raise NotLeaderError(
            f"no leader available after {self._max_retries + 1} attempts to {self.base_url}{path}: {last_err}"
        )

    def close(self) -> None:
        self._session.close()


class _AsyncTransport:
    """Cluster-aware asynchronous HTTP transport (httpx-backed).

    Injected into AsyncRemoteClient so tests can swap in a fake transport.
    """

    def __init__(
        self,
        base_url: str,
        token: Optional[str],
        timeout: float,
        max_retries: int,
        retry_backoff: float,
    ) -> None:
        import httpx
        self.base_url = base_url
        self._timeout = timeout
        self._max_retries = max_retries
        self._retry_backoff = retry_backoff
        self._leader_url: Optional[str] = None
        headers = {"Authorization": f"Bearer {token}"} if token else {}
        self._client = httpx.AsyncClient(
            timeout=timeout, follow_redirects=True, headers=headers
        )

    async def get(self, url: str, **kw):
        return await self._client.get(url, **kw)

    async def post(self, url: str, **kw):
        return await self._client.post(url, **kw)

    async def patch(self, url: str, **kw):
        return await self._client.patch(url, **kw)

    async def delete(self, url: str, **kw):
        return await self._client.delete(url, **kw)

    async def post_rpc(
        self, path: str, json_data: Dict[str, Any]
    ) -> Dict[str, Any]:
        import asyncio
        import httpx
        last_err: Optional[Exception] = None
        for attempt in range(self._max_retries + 1):
            base = self._leader_url or self.base_url
            url = base + path
            try:
                resp = await self._client.post(url, json=json_data)
                if resp.status_code == 307:
                    self._leader_url = None
                    raise _Retryable("no leader (307)")
                if resp.status_code == 503:
                    self._leader_url = None
                    raise _Retryable("no leader (503)")
                if resp.status_code == 404:
                    raise NotFoundError(f"Resource not found: {path}")
                if resp.status_code in (401, 403):
                    action = "set token=" if resp.status_code == 401 else "check token permissions for"
                    raise AuthenticationError(
                        f"[HTTP {resp.status_code}] Authentication failed for {path} — "
                        f"{action} this operation. "
                        f"Pass token= to the client or set VALORI_AUTH_TOKEN on the node."
                    )
                if resp.status_code in (400, 409, 413, 422):
                    try:
                        detail = resp.json().get("error") or resp.text
                    except Exception:
                        detail = resp.text
                    raise ValidationError(f"[HTTP {resp.status_code}] {detail}")
                _raise_for_status(resp)
                if resp.history:
                    self._leader_url = _base_of(str(resp.url), path)
                return resp.json()
            except (_Retryable, httpx.ConnectError) as e:
                last_err = e
                self._leader_url = None
                if attempt < self._max_retries:
                    await asyncio.sleep(self._retry_backoff * (2 ** attempt))
                    continue
            except (AuthenticationError, ValidationError, NotFoundError):
                raise
            except httpx.HTTPError as e:
                raise ConnectionError(f"Failed to connect to Valoricore node at {url}: {e}")
        raise NotLeaderError(
            f"no leader available after {self._max_retries + 1} attempts to {self.base_url}{path}: {last_err}"
        )

    async def close(self) -> None:
        await self._client.aclose()


# ── Sync domain mixins (SRP + OCP) ──────────────────────────────────────────
# Each mixin owns one domain. All methods delegate to self._t (_SyncTransport).


class _SyncAutoSnapshotMixin:
    _t: _SyncTransport
    _auto_snapshot_interval: Optional[int]
    _insert_count: int
    _snapshot_dir: str

    def _check_auto_snapshot(self, count: int = 1) -> None:
        if self._auto_snapshot_interval:
            import os
            old = self._insert_count
            self._insert_count += count
            if (old // self._auto_snapshot_interval) < (self._insert_count // self._auto_snapshot_interval):
                snap_bytes = self.snapshot()
                os.makedirs(self._snapshot_dir, exist_ok=True)
                path = os.path.join(self._snapshot_dir, f"auto_snapshot_{self._insert_count}.snap")
                with open(path, "wb") as f:
                    f.write(snap_bytes)


class _SyncRecordsMixin(_SyncAutoSnapshotMixin):
    _t: _SyncTransport

    def insert(
        self,
        vector: Vector,
        tag: int = 0,
        collection: str = "default",
        idempotency_key: Optional[bytes] = None,
        text: Optional[str] = None,
    ) -> RecordId:
        data: Dict[str, Any] = {"values": vector, "tag": tag}
        if collection != "default":
            data["collection"] = collection
        if text is not None:
            data["text"] = text
        key = idempotency_key if idempotency_key is not None else uuid4().bytes
        resp = self._t.post_rpc("/v1/records", data, idempotency_key=key)
        self._check_auto_snapshot(1)
        return resp["id"]

    def insert_with_proof(
        self, vector: Vector, tag: int = 0, collection: str = "default"
    ) -> Tuple[RecordId, Proof]:
        try:
            import valoricore as _vc
            fixed_vals = _vc.ingest_embedding(vector)
            proof_bytes: Proof = bytes.fromhex(_vc.generate_proof(fixed_vals))
        except (ImportError, AttributeError):
            proof_bytes = b""
        rid = self.insert(vector, tag=tag, collection=collection)
        return (rid, proof_bytes)

    def insert_with_receipt(
        self,
        vector: Vector,
        tag: int = 0,
        collection: str = "default",
        text: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Insert a vector and return the cryptographic InsertReceipt.

        Returns a dict with: record_id, old_root, new_root, proof,
        sequence, timestamp, state_hash — all as hex strings where applicable.
        The receipt's state_hash can be independently verified by recomputing
        BLAKE3("valori-insert-receipt-v1" || fields).
        """
        data: Dict[str, Any] = {"values": vector, "tag": tag}
        if collection != "default":
            data["collection"] = collection
        if text is not None:
            data["text"] = text
        key = __import__("uuid").uuid4().bytes
        resp = self._t.post_rpc("/v1/records", data, idempotency_key=key)
        self._check_auto_snapshot(1)
        return resp.get("receipt", {"record_id": resp["id"]})

    def insert_batch(
        self,
        batch: List[Vector],
        collection: str = "default",
        metadata: Optional[List[Optional[Dict[str, Any]]]] = None,
        request_ids: Optional[List[Optional[str]]] = None,
        texts: Optional[List[Optional[str]]] = None,
        **kwargs: Any,
    ) -> List[RecordId]:
        import json as _json
        data: Dict[str, Any] = {"batch": batch}
        if collection != "default":
            data["collection"] = collection
        if metadata is not None:
            data["metadata"] = [
                _json.dumps(m, separators=(",", ":")) if m is not None else None
                for m in metadata
            ]
        if request_ids is not None:
            data["request_ids"] = request_ids
        if texts is not None:
            data["texts"] = texts
        resp = self._t.post_rpc("/v1/vectors/batch-insert", data)
        self._check_auto_snapshot(len(batch))
        return resp["ids"]

    def insert_batch_with_proof(
        self, vectors: List[Vector], tags: Optional[List[int]] = None
    ) -> List[Tuple[RecordId, Proof]]:
        if tags is None:
            tags = [0] * len(vectors)
        ids = self.insert_batch(vectors, metadata=None)
        try:
            import valoricore as _vc
            proofs: List[Proof] = [
                bytes.fromhex(_vc.generate_proof(_vc.ingest_embedding(v)))
                for v in vectors
            ]
        except (ImportError, AttributeError):
            proofs = [b""] * len(vectors)
        self._check_auto_snapshot(len(vectors))
        return list(zip(ids, proofs))

    def delete(
        self,
        record_id: int,
        collection: str = "default",
        idempotency_key: Optional[bytes] = None,
    ) -> None:
        key = idempotency_key if idempotency_key is not None else uuid4().bytes
        data: Dict[str, Any] = {"id": record_id}
        if collection != "default":
            data["collection"] = collection
        self._t.post_rpc("/v1/delete", data, idempotency_key=key)

    def soft_delete(
        self,
        record_id: int,
        collection: str = "default",
        idempotency_key: Optional[bytes] = None,
    ) -> None:
        key = idempotency_key if idempotency_key is not None else uuid4().bytes
        data: Dict[str, Any] = {"id": record_id}
        if collection != "default":
            data["collection"] = collection
        self._t.post_rpc("/v1/soft-delete", data, idempotency_key=key)

    def get_record(self, record_id: int, collection: str = "default") -> Dict[str, Any]:
        url = self._t.base_url + f"/v1/records/{record_id}"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = self._t.get(url, params=params)
            if resp.status_code == 404:
                raise NotFoundError(f"Record {record_id} not found")
            _raise_for_status(resp, f"/v1/records/{record_id}")
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to fetch record {record_id}: {e}")

    def update_record_metadata(
        self,
        record_id: int,
        metadata: Dict[str, Any],
        collection: str = "default",
    ) -> None:
        url = self._t.base_url + f"/v1/records/{record_id}/metadata"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = self._t.patch(url, json=metadata, params=params)
            if resp.status_code == 404:
                raise NotFoundError(f"Record {record_id} not found")
            _raise_for_status(resp, f"/v1/records/{record_id}/metadata")
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to update metadata for record {record_id}: {e}")


class _SyncSearchMixin:
    _t: _SyncTransport

    def search(
        self,
        query: Vector,
        k: int,
        filter_tag: Optional[int] = None,
        consistency: Optional[str] = None,
        collection: str = "default",
        as_of: Optional[str] = None,
        as_of_log_index: Optional[int] = None,
        decay_half_life_secs: Optional[int] = None,
        rerank: bool = True,
        query_text: Optional[str] = None,
        metadata_filter: Optional[Dict[str, Any]] = None,
    ) -> List[Dict[str, Any]]:
        data: Dict[str, Any] = {"query": query, "k": k}
        if filter_tag is not None:
            data["filter_tag"] = filter_tag
        if consistency is not None:
            data["consistency"] = consistency
        if collection != "default":
            data["collection"] = collection
        if as_of_log_index is not None:
            data["as_of_log_index"] = as_of_log_index
        elif as_of is not None:
            data["as_of"] = as_of
        if decay_half_life_secs is not None:
            data["decay_half_life_secs"] = decay_half_life_secs
        data["rerank"] = rerank
        if query_text is not None:
            data["query_text"] = query_text
        if metadata_filter is not None:
            data["metadata_filter"] = metadata_filter
        resp = self._t.post_rpc("/v1/search", data)
        if as_of is not None or as_of_log_index is not None:
            return resp
        return resp["results"]

    def graphrag(
        self,
        query_vector: Vector,
        k: int = 5,
        depth: int = 2,
        collection: str = "default",
        consistency: Optional[str] = None,
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"query_vector": query_vector, "k": k, "depth": depth}
        if collection != "default":
            data["collection"] = collection
        if consistency is not None:
            data["consistency"] = consistency
        return self._t.post_rpc("/v1/graphrag", data)


class _SyncGraphMixin:
    _t: _SyncTransport
    _MAX_WALK_DEPTH = 10

    def create_node(self, kind: int, record_id: Optional[int] = None, collection: str = "default") -> NodeId:
        data: Dict[str, Any] = {"kind": kind, "record_id": record_id}
        if collection != "default":
            data["collection"] = collection
        return self._t.post_rpc("/v1/graph/node", data)["node_id"]

    def create_edge(self, from_id: int, to_id: int, kind: int, collection: str = "default") -> int:
        data: Dict[str, Any] = {"from": from_id, "to": to_id, "kind": kind}
        if collection != "default":
            data["collection"] = collection
        return self._t.post_rpc("/v1/graph/edge", data)["edge_id"]

    def get_node(self, node_id: int, collection: str = "default") -> Optional[Dict[str, Any]]:
        url = self._t.base_url + f"/v1/graph/node/{node_id}"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = self._t.get(url, params=params)
            if resp.status_code == 404:
                return None
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to retrieve node: {e}")

    def get_edges(self, node_id: int, collection: str = "default") -> List[Dict[str, Any]]:
        url = self._t.base_url + f"/v1/graph/edges/{node_id}"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = self._t.get(url, params=params)
            if resp.status_code == 404:
                return []
            _raise_for_status(resp)
            return resp.json().get("edges", [])
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to retrieve edges: {e}")

    def delete_node(self, node_id: int, collection: str = "default") -> None:
        params = {} if collection == "default" else {"collection": collection}
        url = self._t.base_url + f"/v1/graph/node/{node_id}"
        resp = self._t.delete(url, params=params)
        _raise_for_status(resp, f"/v1/graph/node/{node_id}")

    def list_nodes(self, collection: str = "default") -> Dict[str, Any]:
        url = self._t.base_url + "/v1/graph/nodes"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = self._t.get(url, params=params)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list nodes: {e}")

    def neighbors(self, node_id: int, collection: str = "default") -> List[int]:
        return [e["to_node"] for e in self.get_edges(node_id, collection=collection)]

    def walk(self, start_node: int, max_depth: int = 2, collection: str = "default") -> List[int]:
        max_depth = min(max_depth, self._MAX_WALK_DEPTH)
        visited = {start_node}
        queue = deque([(start_node, 0)])
        result = []
        while queue:
            current, depth = queue.popleft()
            result.append(current)
            if depth >= max_depth:
                continue
            for edge in self.get_edges(current, collection=collection):
                nxt = edge["to_node"]
                if nxt not in visited:
                    visited.add(nxt)
                    queue.append((nxt, depth + 1))
        return result

    def expand(self, start_node: int, max_depth: int = 2, collection: str = "default") -> List[int]:
        record_ids = set()
        for node_id in self.walk(start_node, max_depth, collection=collection):
            n = self.get_node(node_id, collection=collection)
            if n and n["record_id"] is not None:
                record_ids.add(n["record_id"])
        return list(record_ids)

    def subgraph(self, root_node: int, depth: int = 2, collection: str = "default") -> Dict[str, Any]:
        url = self._t.base_url + f"/v1/graph/subgraph?root={root_node}&depth={depth}"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = self._t.get(url, params=params)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"subgraph failed: {e}")


class _SyncProofMixin:
    _t: _SyncTransport

    def get_proof(self) -> Dict[str, Any]:
        try:
            resp = self._t.get(self._t.base_url + "/v1/proof/state")
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get proof: {e}")

    def event_log_proof(self) -> Dict[str, Any]:
        try:
            resp = self._t.get(self._t.base_url + "/v1/proof/event-log", timeout=5)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get event-log proof: {e}")

    def get_receipt(self) -> Dict[str, Any]:
        try:
            resp = self._t.get(self._t.base_url + "/v1/proof/receipt", timeout=5)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get receipt: {e}")

    def get_receipt_by_id(self, receipt_id: str) -> Dict[str, Any]:
        try:
            resp = self._t.get(f"{self._t.base_url}/v1/proof/receipt/{receipt_id}", timeout=5)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get receipt '{receipt_id}': {e}")

    def get_state_hash(self) -> str:
        try:
            resp = self._t.get(self._t.base_url + "/v1/proof/state", timeout=5)
            _raise_for_status(resp)
            h = resp.json()["final_state_hash"]
            return bytes(h).hex() if isinstance(h, list) else h
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to retrieve state hash: {e}")


class _SyncSnapshotMixin:
    _t: _SyncTransport
    _auto_snapshot_interval: Optional[int]
    _insert_count: int
    _snapshot_dir: str

    def snapshot(self, auto_interval: Optional[int] = None, save_dir: str = "./valoricore_snapshots") -> bytes:
        if auto_interval is not None:
            self._auto_snapshot_interval = auto_interval
            self._insert_count = 0
            self._snapshot_dir = save_dir
        try:
            resp = self._t.get(self._t.base_url + "/v1/snapshot/download", timeout=30)
            _raise_for_status(resp)
            return resp.content
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to download snapshot: {e}")

    def restore(self, data: bytes) -> None:
        url = self._t.base_url + "/v1/snapshot/upload"
        try:
            resp = self._t.post(url, data=data, headers={"Content-Type": "application/octet-stream"}, timeout=60)
            _raise_for_status(resp)
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to restore snapshot: {e}")

    def save_snapshot(self, path: Optional[str] = None) -> Dict[str, Any]:
        data: Dict[str, Any] = {}
        if path is not None:
            data["path"] = path
        return self._t.post_rpc("/v1/snapshot/save", data)

    def restore_snapshot(self, path: str) -> Dict[str, Any]:
        return self._t.post_rpc("/v1/snapshot/restore", {"path": path})

    def list_remote_snapshots(self) -> Dict[str, Any]:
        try:
            resp = self._t.get(self._t.base_url + "/v1/storage/snapshots", timeout=15)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list remote snapshots: {e}")

    def upload_snapshot_to_store(self) -> Dict[str, Any]:
        return self._t.post_rpc("/v1/storage/snapshots/upload", {})

    def restore_from_store(self, key: str) -> Dict[str, Any]:
        return self._t.post_rpc("/v1/storage/snapshots/restore", {"key": key})

    def list_remote_wal(self) -> Dict[str, Any]:
        try:
            resp = self._t.get(self._t.base_url + "/v1/storage/wal", timeout=15)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list remote WAL: {e}")

    def archive_wal_segment(self, path: str) -> Dict[str, Any]:
        return self._t.post_rpc("/v1/storage/wal/archive", {"path": path})


class _SyncCollectionsMixin:
    _t: _SyncTransport

    def create_collection(self, name: str) -> Dict[str, Any]:
        return self._t.post_rpc("/v1/namespaces", {"name": name})

    def list_collections(self) -> List[Dict[str, Any]]:
        try:
            resp = self._t.get(self._t.base_url + "/v1/namespaces", timeout=5)
            _raise_for_status(resp)
            return resp.json().get("collections", [])
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list collections: {e}")

    def drop_collection(self, name: str) -> None:
        url = self._t.base_url + f"/v1/namespaces/{name}"
        try:
            resp = self._t.delete(url, timeout=5)
            if resp.status_code == 400:
                raise ValueError(resp.json().get("error", resp.text))
            _raise_for_status(resp)
        except (ValueError, NotFoundError, AuthenticationError):
            raise
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to drop collection '{name}': {e}")

    def health(self) -> str:
        try:
            resp = self._t.get(self._t.base_url + "/health", timeout=5)
            _raise_for_status(resp)
            return resp.json().get("status", "unknown")
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to reach node: {e}")


class _SyncMemoryMixin:
    _t: _SyncTransport

    def memory_upsert(
        self,
        vector: Vector,
        collection: str = "default",
        attach_to_document_node: Optional[int] = None,
        metadata: Optional[Dict[str, Any]] = None,
        tags: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"vector": vector}
        if collection != "default":
            data["collection"] = collection
        if attach_to_document_node is not None:
            data["attach_to_document_node"] = attach_to_document_node
        if metadata is not None:
            data["metadata"] = metadata
        if tags is not None:
            data["tags"] = tags
        return self._t.post_rpc("/v1/memory/upsert_vector", data)

    def memory_search(
        self,
        query_vector: Vector,
        k: int = 5,
        collection: str = "default",
        decay_half_life_secs: Optional[int] = None,
    ) -> List[Dict[str, Any]]:
        data: Dict[str, Any] = {"query_vector": query_vector, "k": k}
        if collection != "default":
            data["collection"] = collection
        if decay_half_life_secs is not None:
            data["decay_half_life_secs"] = decay_half_life_secs
        return self._t.post_rpc("/v1/memory/search_vector", data)["results"]

    def consolidate(
        self,
        old_record_id: int,
        new_vector: Vector,
        collection: str = "default",
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"old_record_id": old_record_id, "new_vector": new_vector}
        if collection != "default":
            data["collection"] = collection
        if metadata is not None:
            data["metadata"] = metadata
        return self._t.post_rpc("/v1/memory/consolidate", data)

    def contradict(
        self,
        record_a: int,
        record_b: int,
        threshold: Optional[float] = None,
        collection: str = "default",
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"record_a": record_a, "record_b": record_b}
        if threshold is not None:
            data["threshold"] = threshold
        if collection != "default":
            data["collection"] = collection
        return self._t.post_rpc("/v1/memory/contradict", data)


class _SyncTreeMixin:
    _t: _SyncTransport

    def tree_build(self, text: str, doc_name: Optional[str] = None) -> dict:
        data: dict = {"text": text}
        if doc_name is not None:
            data["doc_name"] = doc_name
        return self._t.post_rpc("/v1/tree/build", data)

    def tree_query(
        self,
        tree: dict,
        query: str,
        k: int = 2,
        prev_hash: Optional[str] = None,
    ) -> dict:
        data: dict = {"tree": tree, "query": query, "k": k}
        if prev_hash is not None:
            data["prev_hash"] = prev_hash
        return self._t.post_rpc("/v1/tree/query", data)

    def tree_verify(self, tree: dict, receipt: dict) -> bool:
        return bool(self._t.post_rpc("/v1/tree/verify", {"tree": tree, "receipt": receipt}).get("valid", False))

    def tree_chain_verify(self, receipts: list) -> dict:
        return self._t.post_rpc("/v1/tree/chain-verify", {"receipts": receipts})

    def tree_hybrid(
        self,
        query: str,
        *,
        text: Optional[str] = None,
        tree: Optional[dict] = None,
        cache_key: Optional[str] = None,
        namespace: Optional[str] = None,
        k: int = 5,
        tree_weight: float = 0.6,
        prev_hash: Optional[str] = None,
        doc_name: Optional[str] = None,
    ) -> dict:
        body: dict = {"query": query, "k": k, "tree_weight": tree_weight}
        if text is not None:
            body["text"] = text
        if tree is not None:
            body["tree"] = tree
        if cache_key is not None:
            body["cache_key"] = cache_key
        if namespace is not None:
            body["namespace"] = namespace
        if prev_hash is not None:
            body["prev_hash"] = prev_hash
        if doc_name is not None:
            body["doc_name"] = doc_name
        return self._t.post_rpc("/v1/tree/hybrid", body)


class _SyncCommunityMixin:
    _t: _SyncTransport

    def community_detect(
        self,
        *,
        namespace: Optional[str] = None,
        max_iter: Optional[int] = None,
    ) -> dict:
        body: dict = {}
        if namespace is not None:
            body["namespace"] = namespace
        if max_iter is not None:
            body["max_iter"] = max_iter
        return self._t.post_rpc("/v1/community/detect", body)

    def community_search(
        self,
        vector: Vector,
        *,
        k: int = 5,
        namespace: Optional[str] = None,
        depth: int = 1,
        drill_in: bool = False,
    ) -> dict:
        body: dict = {"vector": list(vector), "k": k, "depth": depth, "drill_in": drill_in}
        if namespace is not None:
            body["namespace"] = namespace
        return self._t.post_rpc("/v1/community/search", body)

    def community_overview(self) -> dict:
        try:
            resp = self._t.get(self._t.base_url + "/v1/community/overview", timeout=30)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get community overview: {e}")

    def extract_entities(
        self,
        text: str,
        *,
        namespace: Optional[str] = None,
        entity_types: Optional[List[str]] = None,
        model: Optional[str] = None,
    ) -> dict:
        body: dict = {"text": text}
        if namespace is not None:
            body["namespace"] = namespace
        if entity_types is not None:
            body["entity_types"] = entity_types
        if model is not None:
            body["model"] = model
        return self._t.post_rpc("/v1/ingest/extract-entities", body)


class _SyncIngestMixin:
    _t: _SyncTransport

    def chunk_document(
        self,
        text: str,
        strategy: str = "auto",
        collection: str = "default",
        source: Optional[str] = None,
        chunk_size: int = 1000,
        chunk_overlap: int = 200,
    ) -> dict:
        data: dict = {"text": text, "strategy": strategy, "collection": collection,
                      "chunk_size": chunk_size, "chunk_overlap": chunk_overlap}
        if source is not None:
            data["source"] = source
        return self._t.post_rpc("/v1/ingest/document", data)

    def ingest(
        self,
        text: str,
        source: Optional[str] = None,
        strategy: str = "auto",
        collection: str = "default",
        chunk_size: int = 1000,
        chunk_overlap: int = 200,
    ) -> dict:
        data: dict = {"text": text, "strategy": strategy, "collection": collection,
                      "chunk_size": chunk_size, "chunk_overlap": chunk_overlap}
        if source is not None:
            data["source"] = source
        return self._t.post_rpc("/v1/ingest", data)

    def ingest_update(
        self,
        document_node_id: int,
        text: str,
        source: Optional[str] = None,
        strategy: str = "auto",
        collection: str = "default",
        chunk_size: int = 1000,
        chunk_overlap: int = 200,
    ) -> dict:
        data: dict = {"document_node_id": document_node_id, "text": text, "strategy": strategy,
                      "collection": collection, "chunk_size": chunk_size, "chunk_overlap": chunk_overlap}
        if source is not None:
            data["source"] = source
        return self._t.post_rpc("/v1/ingest/update", data)

    def ingest_async(
        self,
        text: str,
        source: Optional[str] = None,
        strategy: str = "auto",
        collection: str = "default",
        chunk_size: int = 1000,
        chunk_overlap: int = 200,
    ) -> str:
        data: dict = {"text": text, "strategy": strategy, "collection": collection,
                      "chunk_size": chunk_size, "chunk_overlap": chunk_overlap, "async": True}
        if source is not None:
            data["source"] = source
        return self._t.post_rpc("/v1/ingest", data)["job_id"]

    def ingest_status(self, job_id: str) -> dict:
        url = self._t.base_url + f"/v1/ingest/status/{job_id}"
        try:
            resp = self._t.get(url)
            _raise_for_status(resp, f"/v1/ingest/status/{job_id}")
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to fetch ingest status: {e}")


class _SyncCryptoMixin:
    _t: _SyncTransport

    def insert_encrypted(
        self,
        payload: bytes,
        tag: int = 0,
        collection: str = "default",
        key_id: Optional[str] = None,
    ) -> Dict[str, Any]:
        import base64
        body: Dict[str, Any] = {"payload": base64.b64encode(payload).decode(),
                                 "tag": tag, "collection": collection}
        if key_id is not None:
            body["key_id"] = key_id
        return self._t.post_rpc("/v1/records/encrypted", body)

    def shred_key(self, key_id: str) -> Dict[str, Any]:
        try:
            resp = self._t.delete(self._t.base_url + f"/v1/crypto/shred/{key_id}", timeout=5)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to shred key '{key_id}': {e}")

    def shred_key_status(self, key_id: str) -> Dict[str, Any]:
        try:
            resp = self._t.get(self._t.base_url + f"/v1/crypto/status/{key_id}", timeout=5)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to check key status '{key_id}': {e}")


class _SyncKeysMixin:
    _t: _SyncTransport

    def create_key(
        self,
        scope: str = "read_write",
        collection: Optional[str] = None,
        description: Optional[str] = None,
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"scope": scope}
        if collection is not None:
            data["collection"] = collection
        if description is not None:
            data["description"] = description
        return self._t.post_rpc("/v1/keys", data)

    def list_keys(self) -> List[Dict[str, Any]]:
        try:
            resp = self._t.get(self._t.base_url + "/v1/keys", timeout=5)
            _raise_for_status(resp)
            return resp.json().get("keys", [])
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list keys: {e}")

    def revoke_key(self, key_id: str) -> None:
        try:
            resp = self._t.delete(self._t.base_url + f"/v1/keys/{key_id}", timeout=5)
            if resp.status_code == 404:
                raise NotFoundError(f"Key not found: {key_id}")
            _raise_for_status(resp)
        except (NotFoundError, AuthenticationError):
            raise
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to revoke key '{key_id}': {e}")


class _SyncClusterMixin:
    _t: _SyncTransport

    def cluster_status(self) -> Dict[str, Any]:
        url = self._t.base_url + "/v1/cluster/status"
        try:
            resp = self._t.get(url, timeout=5)
            if resp.status_code == 404:
                raise ConnectionError("node is not running in cluster mode")
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to fetch cluster status from {url}: {e}")

    def cluster_health(self) -> bool:
        try:
            resp = self._t.get(self._t.base_url + "/v1/cluster/health", timeout=5)
            return resp.status_code == 200
        except requests.exceptions.RequestException:
            return False

    def leader_url(self) -> Optional[str]:
        return self._t._leader_url

    def get_cluster_role(self) -> str:
        url = self._t.base_url + "/v1/cluster/role"
        try:
            resp = self._t.get(url, timeout=5)
            if resp.status_code == 404:
                raise ConnectionError("node is not running in cluster mode")
            _raise_for_status(resp)
            return resp.json().get("role", "unknown")
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to fetch cluster role from {url}: {e}")


class _SyncIndexMixin:
    _t: _SyncTransport

    def get_index_config(self) -> Dict[str, Any]:
        try:
            resp = self._t.get(self._t.base_url + "/v1/index/config", timeout=5)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get index config: {e}")

    def set_index(self, index: str) -> Dict[str, Any]:
        try:
            resp = self._t.post(self._t.base_url + "/v1/index/rebuild", json={"index": index}, timeout=30)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to rebuild index: {e}")

    def shard_routing(self) -> Dict[str, Any]:
        try:
            resp = self._t.get(self._t.base_url + "/v1/shard/routing", timeout=10)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get shard routing: {e}")

    def get_version(self) -> str:
        try:
            resp = self._t.get(self._t.base_url + "/v1/version", timeout=5)
            _raise_for_status(resp)
            return resp.text.strip()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get version: {e}")


class _SyncMetaMixin:
    _t: _SyncTransport
    ui_url: str

    def get_metadata(self, record_id: int) -> Optional[Dict[str, Any]]:
        import json as _json
        url = f"{self._t.base_url}/v1/memory/meta/get?key=rec:{record_id}"
        try:
            resp = self._t.get(url, timeout=5)
            if resp.status_code == 404:
                return None
            _raise_for_status(resp)
            val = resp.json().get("value")
            if val is None:
                return None
            return _json.loads(val) if isinstance(val, str) else val
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to retrieve metadata: {e}")

    def set_metadata(self, record_id: int, metadata: Dict[str, Any]) -> None:
        import json as _json
        self._t.post_rpc("/v1/memory/meta/set", {
            "key": f"rec:{record_id}",
            "value": _json.dumps(metadata, separators=(",", ":")),
        })

    def meta_get(self, key: str) -> Optional[str]:
        url = f"{self._t.base_url}/v1/memory/meta/get?key={key}"
        try:
            resp = self._t.get(url, timeout=5)
            if resp.status_code == 404:
                return None
            _raise_for_status(resp)
            return resp.json().get("value")
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to retrieve meta key '{key}': {e}")

    def meta_set(self, key: str, value: str) -> None:
        self._t.post_rpc("/v1/memory/meta/set", {"key": key, "value": value})

    def record_count(self) -> int:
        resp = self._t.get(f"{self._t.base_url}/health", timeout=5)
        _raise_for_status(resp)
        return resp.json().get("records", {}).get("live", 0)

    def timeline(
        self,
        from_ts: Optional[str] = None,
        to_ts: Optional[str] = None,
        collection: Optional[str] = None,
    ) -> Dict[str, Any]:
        params: Dict[str, str] = {}
        if from_ts:
            params["from"] = from_ts
        if to_ts:
            params["to"] = to_ts
        if collection:
            params["collection"] = collection
        try:
            resp = self._t.get(f"{self._t.base_url}/v1/timeline", params=params)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to fetch timeline: {e}")

    def get_timeline(self) -> List[str]:
        url = self._t.base_url + "/v1/timeline"
        try:
            resp = self._t.get(url, timeout=10)
            if resp.status_code == 404:
                raise NotFoundError("Timeline endpoint not found on remote node.")
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to fetch timeline from {url}: {e}")

    def list_contradictions(self, collection: str = "default", status: str = "pending") -> Dict[str, Any]:
        warnings.warn(
            "list_contradictions() is deprecated; use contradict() instead.",
            DeprecationWarning, stacklevel=2,
        )
        url = self.ui_url + f"/api/contradictions?collection={collection}&status={status}"
        try:
            resp = self._t.get(url, timeout=10)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"list_contradictions failed: {e}")

    def resolve_contradiction(self, contradiction_id: str, action: str) -> Dict[str, Any]:
        warnings.warn(
            "resolve_contradiction() is deprecated; use consolidate() or contradict() instead.",
            DeprecationWarning, stacklevel=2,
        )
        url = self.ui_url + "/api/contradictions"
        try:
            resp = self._t.post(url, json={"id": contradiction_id, "action": action}, timeout=5)
            _raise_for_status(resp)
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"resolve_contradiction failed: {e}")


# ── Async domain mixins ──────────────────────────────────────────────────────


class _AsyncAutoSnapshotMixin:
    _t: _AsyncTransport
    _auto_snapshot_interval: Optional[int]
    _insert_count: int
    _snapshot_dir: str

    async def _check_auto_snapshot(self, count: int = 1) -> None:
        if self._auto_snapshot_interval:
            import os
            old = self._insert_count
            self._insert_count += count
            if (old // self._auto_snapshot_interval) < (self._insert_count // self._auto_snapshot_interval):
                snap_bytes = await self.snapshot()
                os.makedirs(self._snapshot_dir, exist_ok=True)
                path = os.path.join(self._snapshot_dir, f"auto_snapshot_{self._insert_count}.snap")
                with open(path, "wb") as f:
                    f.write(snap_bytes)


class _AsyncRecordsMixin(_AsyncAutoSnapshotMixin):
    _t: _AsyncTransport

    async def insert(
        self,
        vector: Vector,
        tag: int = 0,
        collection: str = "default",
        text: Optional[str] = None,
    ) -> RecordId:
        data: Dict[str, Any] = {"values": vector, "tag": tag}
        if collection != "default":
            data["collection"] = collection
        if text is not None:
            data["text"] = text
        resp = await self._t.post_rpc("/v1/records", data)
        await self._check_auto_snapshot(1)
        return resp["id"]

    async def insert_with_proof(
        self, vector: Vector, tag: int = 0, collection: str = "default"
    ) -> Tuple[RecordId, Proof]:
        try:
            import valoricore as _vc
            fixed_vals = _vc.ingest_embedding(vector)
            proof_bytes: Proof = bytes.fromhex(_vc.generate_proof(fixed_vals))
        except (ImportError, AttributeError):
            proof_bytes = b""
        rid = await self.insert(vector, tag=tag, collection=collection)
        return (rid, proof_bytes)

    async def insert_with_receipt(
        self,
        vector: Vector,
        tag: int = 0,
        collection: str = "default",
        text: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Insert a vector and return the cryptographic InsertReceipt.

        Returns a dict with: record_id, old_root, new_root, proof,
        sequence, timestamp, state_hash — all as hex strings where applicable.
        """
        data: Dict[str, Any] = {"values": vector, "tag": tag}
        if collection != "default":
            data["collection"] = collection
        if text is not None:
            data["text"] = text
        resp = await self._t.post_rpc("/v1/records", data)
        await self._check_auto_snapshot(1)
        return resp.get("receipt", {"record_id": resp["id"]})

    async def insert_batch(
        self,
        batch: List[Vector],
        collection: str = "default",
        metadata: Optional[List[Optional[str]]] = None,
        request_ids: Optional[List[Optional[str]]] = None,
        texts: Optional[List[Optional[str]]] = None,
    ) -> List[RecordId]:
        data: Dict[str, Any] = {"batch": batch}
        if collection != "default":
            data["collection"] = collection
        if metadata is not None:
            data["metadata"] = metadata
        if request_ids is not None:
            data["request_ids"] = request_ids
        if texts is not None:
            data["texts"] = texts
        resp = await self._t.post_rpc("/v1/vectors/batch-insert", data)
        await self._check_auto_snapshot(len(batch))
        return resp["ids"]

    async def insert_batch_with_proof(
        self, vectors: List[Vector], tags: Optional[List[int]] = None
    ) -> List[Tuple[RecordId, Proof]]:
        if tags is None:
            tags = [0] * len(vectors)
        results = []
        for vector, tag in zip(vectors, tags):
            rid, proof = await self.insert_with_proof(vector, tag=tag)
            results.append((rid, proof))
        await self._check_auto_snapshot(len(vectors))
        return results

    async def delete(self, record_id: int, collection: str = "default") -> None:
        data: Dict[str, Any] = {"id": record_id}
        if collection != "default":
            data["collection"] = collection
        await self._t.post_rpc("/v1/delete", data)

    async def soft_delete(self, record_id: int, collection: str = "default") -> None:
        data: Dict[str, Any] = {"id": record_id}
        if collection != "default":
            data["collection"] = collection
        await self._t.post_rpc("/v1/soft-delete", data)

    async def get_record(self, record_id: int, collection: str = "default") -> Dict[str, Any]:
        url = self._t.base_url + f"/v1/records/{record_id}"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = await self._t.get(url, params=params)
            if resp.status_code == 404:
                raise NotFoundError(f"Record {record_id} not found")
            _raise_for_status(resp, f"/v1/records/{record_id}")
            return resp.json()
        except (NotFoundError, AuthenticationError):
            raise
        except Exception as e:
            raise ConnectionError(f"Failed to fetch record {record_id}: {e}")

    async def update_record_metadata(
        self, record_id: int, metadata: Dict[str, Any], collection: str = "default"
    ) -> None:
        url = self._t.base_url + f"/v1/records/{record_id}/metadata"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = await self._t.patch(url, json=metadata, params=params)
            if resp.status_code == 404:
                raise NotFoundError(f"Record {record_id} not found")
            _raise_for_status(resp, f"/v1/records/{record_id}/metadata")
        except (NotFoundError, AuthenticationError):
            raise
        except Exception as e:
            raise ConnectionError(f"Failed to update metadata for record {record_id}: {e}")


class _AsyncSearchMixin:
    _t: _AsyncTransport

    async def search(
        self,
        query: Vector,
        k: int,
        filter_tag: Optional[int] = None,
        consistency: Optional[str] = None,
        collection: str = "default",
        as_of: Optional[str] = None,
        as_of_log_index: Optional[int] = None,
        decay_half_life_secs: Optional[int] = None,
        rerank: bool = True,
        query_text: Optional[str] = None,
        metadata_filter: Optional[Dict[str, Any]] = None,
    ) -> List[Dict[str, Any]]:
        data: Dict[str, Any] = {"query": query, "k": k}
        if filter_tag is not None:
            data["filter_tag"] = filter_tag
        if consistency is not None:
            data["consistency"] = consistency
        if collection != "default":
            data["collection"] = collection
        if as_of_log_index is not None:
            data["as_of_log_index"] = as_of_log_index
        elif as_of is not None:
            data["as_of"] = as_of
        if decay_half_life_secs is not None:
            data["decay_half_life_secs"] = decay_half_life_secs
        data["rerank"] = rerank
        if query_text is not None:
            data["query_text"] = query_text
        if metadata_filter is not None:
            data["metadata_filter"] = metadata_filter
        resp = await self._t.post_rpc("/v1/search", data)
        if as_of is not None or as_of_log_index is not None:
            return resp
        return resp["results"]

    async def graphrag(
        self,
        query_vector: Vector,
        k: int = 5,
        depth: int = 2,
        collection: str = "default",
        consistency: Optional[str] = None,
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"query_vector": query_vector, "k": k, "depth": depth}
        if collection != "default":
            data["collection"] = collection
        if consistency is not None:
            data["consistency"] = consistency
        return await self._t.post_rpc("/v1/graphrag", data)


class _AsyncGraphMixin:
    _t: _AsyncTransport
    _MAX_WALK_DEPTH = 10

    async def create_node(self, kind: int, record_id: Optional[int] = None, collection: str = "default") -> NodeId:
        data: Dict[str, Any] = {"kind": kind, "record_id": record_id}
        if collection != "default":
            data["collection"] = collection
        return (await self._t.post_rpc("/v1/graph/node", data))["node_id"]

    async def create_edge(self, from_id: int, to_id: int, kind: int, collection: str = "default") -> int:
        data: Dict[str, Any] = {"from": from_id, "to": to_id, "kind": kind}
        if collection != "default":
            data["collection"] = collection
        return (await self._t.post_rpc("/v1/graph/edge", data))["edge_id"]

    async def get_node(self, node_id: int, collection: str = "default") -> Optional[Dict[str, Any]]:
        url = self._t.base_url + f"/v1/graph/node/{node_id}"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = await self._t.get(url, params=params)
            if resp.status_code == 404:
                return None
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to retrieve node: {e}")

    async def get_edges(self, node_id: int, collection: str = "default") -> List[Dict[str, Any]]:
        url = self._t.base_url + f"/v1/graph/edges/{node_id}"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = await self._t.get(url, params=params)
            if resp.status_code == 404:
                return []
            _raise_for_status(resp)
            return resp.json().get("edges", [])
        except Exception as e:
            raise ConnectionError(f"Failed to retrieve edges: {e}")

    async def delete_node(self, node_id: int, collection: str = "default") -> None:
        url = self._t.base_url + f"/v1/graph/node/{node_id}"
        params = {} if collection == "default" else {"collection": collection}
        resp = await self._t.delete(url, params=params)
        _raise_for_status(resp, f"/v1/graph/node/{node_id}")

    async def list_nodes(self, collection: str = "default") -> Dict[str, Any]:
        url = self._t.base_url + "/v1/graph/nodes"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = await self._t.get(url, params=params)
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to list nodes: {e}")

    async def neighbors(self, node_id: int, collection: str = "default") -> List[int]:
        return [e["to_node"] for e in await self.get_edges(node_id, collection=collection)]

    async def walk(self, start_node: int, max_depth: int = 2, collection: str = "default") -> List[int]:
        max_depth = min(max_depth, self._MAX_WALK_DEPTH)
        visited = {start_node}
        queue = deque([(start_node, 0)])
        result = []
        while queue:
            current, depth = queue.popleft()
            result.append(current)
            if depth >= max_depth:
                continue
            for edge in await self.get_edges(current, collection=collection):
                nxt = edge["to_node"]
                if nxt not in visited:
                    visited.add(nxt)
                    queue.append((nxt, depth + 1))
        return result

    async def expand(self, start_node: int, max_depth: int = 2, collection: str = "default") -> List[int]:
        record_ids = set()
        for node_id in await self.walk(start_node, max_depth, collection=collection):
            n = await self.get_node(node_id, collection=collection)
            if n and n["record_id"] is not None:
                record_ids.add(n["record_id"])
        return list(record_ids)

    async def subgraph(self, root_node: int, depth: int = 2, collection: str = "default") -> Dict[str, Any]:
        url = self._t.base_url + f"/v1/graph/subgraph?root={root_node}&depth={depth}"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = await self._t.get(url, params=params)
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"subgraph failed: {e}")


class _AsyncProofMixin:
    _t: _AsyncTransport

    async def get_proof(self) -> Dict[str, Any]:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/proof/state")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to get proof: {e}")

    async def event_log_proof(self) -> Dict[str, Any]:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/proof/event-log")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to get event-log proof: {e}")

    async def get_receipt(self) -> Dict[str, Any]:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/proof/receipt")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to get receipt: {e}")

    async def get_receipt_by_id(self, receipt_id: str) -> Dict[str, Any]:
        try:
            resp = await self._t.get(f"{self._t.base_url}/v1/proof/receipt/{receipt_id}")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to get receipt '{receipt_id}': {e}")

    async def get_state_hash(self) -> str:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/proof/state")
            _raise_for_status(resp)
            h = resp.json()["final_state_hash"]
            return bytes(h).hex() if isinstance(h, list) else h
        except Exception as e:
            raise ConnectionError(f"Failed to retrieve state hash: {e}")


class _AsyncSnapshotMixin:
    _t: _AsyncTransport
    _auto_snapshot_interval: Optional[int]
    _insert_count: int
    _snapshot_dir: str

    async def snapshot(self, auto_interval: Optional[int] = None, save_dir: str = "./valoricore_snapshots") -> bytes:
        if auto_interval is not None:
            self._auto_snapshot_interval = auto_interval
            self._insert_count = 0
            self._snapshot_dir = save_dir
        try:
            resp = await self._t.get(self._t.base_url + "/v1/snapshot/download")
            _raise_for_status(resp)
            return resp.content
        except Exception as e:
            raise ConnectionError(f"Failed to download snapshot: {e}")

    async def restore(self, data: bytes) -> None:
        url = self._t.base_url + "/v1/snapshot/upload"
        try:
            resp = await self._t.post(url, content=data, headers={"Content-Type": "application/octet-stream"})
            _raise_for_status(resp)
        except Exception as e:
            raise ConnectionError(f"Failed to restore snapshot: {e}")

    async def save_snapshot(self, path: Optional[str] = None) -> Dict[str, Any]:
        data: Dict[str, Any] = {}
        if path is not None:
            data["path"] = path
        return await self._t.post_rpc("/v1/snapshot/save", data)

    async def restore_snapshot(self, path: str) -> Dict[str, Any]:
        return await self._t.post_rpc("/v1/snapshot/restore", {"path": path})

    async def list_remote_snapshots(self) -> Dict[str, Any]:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/storage/snapshots")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to list remote snapshots: {e}")

    async def upload_snapshot_to_store(self) -> Dict[str, Any]:
        return await self._t.post_rpc("/v1/storage/snapshots/upload", {})

    async def restore_from_store(self, key: str) -> Dict[str, Any]:
        return await self._t.post_rpc("/v1/storage/snapshots/restore", {"key": key})

    async def list_remote_wal(self) -> Dict[str, Any]:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/storage/wal")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to list remote WAL: {e}")

    async def archive_wal_segment(self, path: str) -> Dict[str, Any]:
        return await self._t.post_rpc("/v1/storage/wal/archive", {"path": path})


class _AsyncCollectionsMixin:
    _t: _AsyncTransport

    async def create_collection(self, name: str) -> Dict[str, Any]:
        return await self._t.post_rpc("/v1/namespaces", {"name": name})

    async def list_collections(self) -> List[Dict[str, Any]]:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/namespaces")
            _raise_for_status(resp)
            return resp.json().get("collections", [])
        except Exception as e:
            raise ConnectionError(f"Failed to list collections: {e}")

    async def drop_collection(self, name: str) -> None:
        url = self._t.base_url + f"/v1/namespaces/{name}"
        try:
            resp = await self._t.delete(url)
            if resp.status_code == 400:
                raise ValueError(resp.json().get("error", resp.text))
            _raise_for_status(resp)
        except (ValueError, NotFoundError, AuthenticationError):
            raise
        except Exception as e:
            raise ConnectionError(f"Failed to drop collection '{name}': {e}")

    async def health(self) -> str:
        try:
            resp = await self._t.get(self._t.base_url + "/health")
            _raise_for_status(resp)
            return resp.json().get("status", "unknown")
        except Exception as e:
            raise ConnectionError(f"Failed to reach node: {e}")


class _AsyncMemoryMixin:
    _t: _AsyncTransport

    async def memory_upsert(
        self,
        vector: Vector,
        collection: str = "default",
        attach_to_document_node: Optional[int] = None,
        metadata: Optional[Dict[str, Any]] = None,
        tags: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"vector": vector}
        if collection != "default":
            data["collection"] = collection
        if attach_to_document_node is not None:
            data["attach_to_document_node"] = attach_to_document_node
        if metadata is not None:
            data["metadata"] = metadata
        if tags is not None:
            data["tags"] = tags
        return await self._t.post_rpc("/v1/memory/upsert_vector", data)

    async def memory_search(
        self,
        query_vector: Vector,
        k: int = 5,
        collection: str = "default",
        decay_half_life_secs: Optional[int] = None,
    ) -> List[Dict[str, Any]]:
        data: Dict[str, Any] = {"query_vector": query_vector, "k": k}
        if collection != "default":
            data["collection"] = collection
        if decay_half_life_secs is not None:
            data["decay_half_life_secs"] = decay_half_life_secs
        return (await self._t.post_rpc("/v1/memory/search_vector", data))["results"]

    async def consolidate(
        self,
        old_record_id: int,
        new_vector: Vector,
        collection: str = "default",
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"old_record_id": old_record_id, "new_vector": new_vector}
        if collection != "default":
            data["collection"] = collection
        if metadata is not None:
            data["metadata"] = metadata
        return await self._t.post_rpc("/v1/memory/consolidate", data)

    async def contradict(
        self,
        record_a: int,
        record_b: int,
        threshold: Optional[float] = None,
        collection: str = "default",
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"record_a": record_a, "record_b": record_b}
        if threshold is not None:
            data["threshold"] = threshold
        if collection != "default":
            data["collection"] = collection
        return await self._t.post_rpc("/v1/memory/contradict", data)


class _AsyncTreeMixin:
    _t: _AsyncTransport

    async def tree_build(self, text: str, doc_name: Optional[str] = None) -> dict:
        data: dict = {"text": text}
        if doc_name is not None:
            data["doc_name"] = doc_name
        return await self._t.post_rpc("/v1/tree/build", data)

    async def tree_query(
        self, tree: dict, query: str, k: int = 2, prev_hash: Optional[str] = None
    ) -> dict:
        data: dict = {"tree": tree, "query": query, "k": k}
        if prev_hash is not None:
            data["prev_hash"] = prev_hash
        return await self._t.post_rpc("/v1/tree/query", data)

    async def tree_verify(self, tree: dict, receipt: dict) -> bool:
        resp = await self._t.post_rpc("/v1/tree/verify", {"tree": tree, "receipt": receipt})
        return bool(resp.get("valid", False))

    async def tree_chain_verify(self, receipts: list) -> dict:
        return await self._t.post_rpc("/v1/tree/chain-verify", {"receipts": receipts})

    async def tree_hybrid(
        self,
        query: str,
        *,
        text: Optional[str] = None,
        tree: Optional[dict] = None,
        cache_key: Optional[str] = None,
        namespace: Optional[str] = None,
        k: int = 5,
        tree_weight: float = 0.6,
        prev_hash: Optional[str] = None,
        doc_name: Optional[str] = None,
    ) -> dict:
        body: dict = {"query": query, "k": k, "tree_weight": tree_weight}
        if text is not None:
            body["text"] = text
        if tree is not None:
            body["tree"] = tree
        if cache_key is not None:
            body["cache_key"] = cache_key
        if namespace is not None:
            body["namespace"] = namespace
        if prev_hash is not None:
            body["prev_hash"] = prev_hash
        if doc_name is not None:
            body["doc_name"] = doc_name
        return await self._t.post_rpc("/v1/tree/hybrid", body)


class _AsyncCommunityMixin:
    _t: _AsyncTransport

    async def community_detect(
        self, *, namespace: Optional[str] = None, max_iter: Optional[int] = None
    ) -> dict:
        body: dict = {}
        if namespace is not None:
            body["namespace"] = namespace
        if max_iter is not None:
            body["max_iter"] = max_iter
        return await self._t.post_rpc("/v1/community/detect", body)

    async def community_search(
        self, vector: Vector, *, k: int = 5, namespace: Optional[str] = None,
        depth: int = 1, drill_in: bool = False
    ) -> dict:
        body: dict = {"vector": list(vector), "k": k, "depth": depth, "drill_in": drill_in}
        if namespace is not None:
            body["namespace"] = namespace
        return await self._t.post_rpc("/v1/community/search", body)

    async def community_overview(self) -> dict:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/community/overview")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to get community overview: {e}")

    async def extract_entities(
        self, text: str, *, namespace: Optional[str] = None,
        entity_types: Optional[List[str]] = None, model: Optional[str] = None
    ) -> dict:
        body: dict = {"text": text}
        if namespace is not None:
            body["namespace"] = namespace
        if entity_types is not None:
            body["entity_types"] = entity_types
        if model is not None:
            body["model"] = model
        return await self._t.post_rpc("/v1/ingest/extract-entities", body)


class _AsyncIngestMixin:
    _t: _AsyncTransport

    async def chunk_document(
        self, text: str, strategy: str = "auto", collection: str = "default",
        source: Optional[str] = None, chunk_size: int = 1000, chunk_overlap: int = 200
    ) -> dict:
        data: dict = {"text": text, "strategy": strategy, "collection": collection,
                      "chunk_size": chunk_size, "chunk_overlap": chunk_overlap}
        if source is not None:
            data["source"] = source
        return await self._t.post_rpc("/v1/ingest/document", data)

    async def ingest(
        self, text: str, source: Optional[str] = None, strategy: str = "auto",
        collection: str = "default", chunk_size: int = 1000, chunk_overlap: int = 200
    ) -> dict:
        data: dict = {"text": text, "strategy": strategy, "collection": collection,
                      "chunk_size": chunk_size, "chunk_overlap": chunk_overlap}
        if source is not None:
            data["source"] = source
        return await self._t.post_rpc("/v1/ingest", data)

    async def ingest_update(
        self, document_node_id: int, text: str, source: Optional[str] = None,
        strategy: str = "auto", collection: str = "default",
        chunk_size: int = 1000, chunk_overlap: int = 200
    ) -> dict:
        data: dict = {"document_node_id": document_node_id, "text": text, "strategy": strategy,
                      "collection": collection, "chunk_size": chunk_size, "chunk_overlap": chunk_overlap}
        if source is not None:
            data["source"] = source
        return await self._t.post_rpc("/v1/ingest/update", data)

    async def ingest_async(
        self, text: str, source: Optional[str] = None, strategy: str = "auto",
        collection: str = "default", chunk_size: int = 1000, chunk_overlap: int = 200
    ) -> str:
        data: dict = {"text": text, "strategy": strategy, "collection": collection,
                      "chunk_size": chunk_size, "chunk_overlap": chunk_overlap, "async": True}
        if source is not None:
            data["source"] = source
        return (await self._t.post_rpc("/v1/ingest", data))["job_id"]

    async def ingest_status(self, job_id: str) -> dict:
        url = self._t.base_url + f"/v1/ingest/status/{job_id}"
        resp = await self._t.get(url)
        _raise_for_status(resp, f"/v1/ingest/status/{job_id}")
        return resp.json()


class _AsyncCryptoMixin:
    _t: _AsyncTransport

    async def insert_encrypted(
        self, payload: bytes, tag: int = 0, collection: str = "default",
        key_id: Optional[str] = None
    ) -> Dict[str, Any]:
        import base64
        body: Dict[str, Any] = {"payload": base64.b64encode(payload).decode(),
                                 "tag": tag, "collection": collection}
        if key_id is not None:
            body["key_id"] = key_id
        return await self._t.post_rpc("/v1/records/encrypted", body)

    async def shred_key(self, key_id: str) -> Dict[str, Any]:
        try:
            resp = await self._t.delete(self._t.base_url + f"/v1/crypto/shred/{key_id}")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to shred key '{key_id}': {e}")

    async def shred_key_status(self, key_id: str) -> Dict[str, Any]:
        try:
            resp = await self._t.get(self._t.base_url + f"/v1/crypto/status/{key_id}")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to get shred status for '{key_id}': {e}")


class _AsyncKeysMixin:
    _t: _AsyncTransport

    async def create_key(
        self, scope: str = "read_write", collection: Optional[str] = None,
        description: Optional[str] = None
    ) -> Dict[str, Any]:
        data: Dict[str, Any] = {"scope": scope}
        if collection is not None:
            data["collection"] = collection
        if description is not None:
            data["description"] = description
        return await self._t.post_rpc("/v1/keys", data)

    async def list_keys(self) -> List[Dict[str, Any]]:
        try:
            resp = await self._t.get(f"{self._t.base_url}/v1/keys")
            _raise_for_status(resp)
            return resp.json().get("keys", [])
        except Exception as e:
            raise ConnectionError(f"Failed to list keys: {e}")

    async def revoke_key(self, key_id: str) -> None:
        try:
            resp = await self._t.delete(f"{self._t.base_url}/v1/keys/{key_id}")
            if resp.status_code == 404:
                raise NotFoundError(f"Key not found: {key_id}")
            _raise_for_status(resp)
        except (NotFoundError, AuthenticationError):
            raise
        except Exception as e:
            raise ConnectionError(f"Failed to revoke key '{key_id}': {e}")


class _AsyncClusterMixin:
    _t: _AsyncTransport

    async def cluster_status(self) -> Dict[str, Any]:
        url = self._t.base_url + "/v1/cluster/status"
        try:
            resp = await self._t.get(url)
            if resp.status_code == 404:
                raise ConnectionError("node is not running in cluster mode")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to fetch cluster status from {url}: {e}")

    async def cluster_health(self) -> bool:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/cluster/health")
            return resp.status_code == 200
        except Exception:
            return False

    def leader_url(self) -> Optional[str]:
        return self._t._leader_url

    async def get_cluster_role(self) -> str:
        url = self._t.base_url + "/v1/cluster/role"
        try:
            resp = await self._t.get(url)
            if resp.status_code == 404:
                raise ConnectionError("node is not running in cluster mode")
            _raise_for_status(resp)
            return resp.json().get("role", "unknown")
        except Exception as e:
            raise ConnectionError(f"Failed to fetch cluster role from {url}: {e}")


class _AsyncIndexMixin:
    _t: _AsyncTransport

    async def get_index_config(self) -> Dict[str, Any]:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/index/config")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to get index config: {e}")

    async def set_index(self, index: str) -> Dict[str, Any]:
        try:
            resp = await self._t.post(self._t.base_url + "/v1/index/rebuild", json={"index": index})
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to set index to '{index}': {e}")

    async def shard_routing(self) -> Dict[str, Any]:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/shard/routing")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to get shard routing: {e}")

    async def get_version(self) -> str:
        try:
            resp = await self._t.get(self._t.base_url + "/v1/version")
            _raise_for_status(resp)
            return resp.text.strip()
        except Exception as e:
            raise ConnectionError(f"Failed to get version: {e}")


class _AsyncMetaMixin:
    _t: _AsyncTransport
    ui_url: str

    async def get_metadata(self, record_id: int) -> Optional[Dict[str, Any]]:
        import json as _json
        url = f"{self._t.base_url}/v1/memory/meta/get?key=rec:{record_id}"
        try:
            resp = await self._t.get(url)
            if resp.status_code == 404:
                return None
            _raise_for_status(resp)
            val = resp.json().get("value")
            if val is None:
                return None
            return _json.loads(val) if isinstance(val, str) else val
        except Exception as e:
            raise ConnectionError(f"Failed to retrieve metadata: {e}")

    async def set_metadata(self, record_id: int, metadata: Dict[str, Any]) -> None:
        import json as _json
        await self._t.post_rpc("/v1/memory/meta/set", {
            "key": f"rec:{record_id}",
            "value": _json.dumps(metadata, separators=(",", ":")),
        })

    async def meta_get(self, key: str) -> Optional[str]:
        try:
            resp = await self._t.get(f"{self._t.base_url}/v1/memory/meta/get?key={key}")
            if resp.status_code == 404:
                return None
            _raise_for_status(resp)
            return resp.json().get("value")
        except Exception as e:
            raise ConnectionError(f"Failed to retrieve meta key '{key}': {e}")

    async def meta_set(self, key: str, value: str) -> None:
        await self._t.post_rpc("/v1/memory/meta/set", {"key": key, "value": value})

    async def record_count(self) -> int:
        resp = await self._t.get(f"{self._t.base_url}/health")
        _raise_for_status(resp)
        return resp.json().get("records", {}).get("live", 0)

    async def timeline(
        self,
        from_ts: Optional[str] = None,
        to_ts: Optional[str] = None,
        collection: Optional[str] = None,
    ) -> Dict[str, Any]:
        params: Dict[str, str] = {}
        if from_ts:
            params["from"] = from_ts
        if to_ts:
            params["to"] = to_ts
        if collection:
            params["collection"] = collection
        try:
            resp = await self._t.get(f"{self._t.base_url}/v1/timeline", params=params)
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to fetch timeline: {e}")

    async def get_timeline(self) -> List[str]:
        url = self._t.base_url + "/v1/timeline"
        try:
            resp = await self._t.get(url)
            if resp.status_code == 404:
                raise NotFoundError("Timeline endpoint not found on remote node.")
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to fetch timeline from {url}: {e}")

    async def list_contradictions(self, collection: str = "default", status: str = "pending") -> Dict[str, Any]:
        warnings.warn(
            "list_contradictions() is deprecated; use contradict() instead.",
            DeprecationWarning, stacklevel=2,
        )
        url = self.ui_url + f"/api/contradictions?collection={collection}&status={status}"
        try:
            resp = await self._t.get(url)
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"list_contradictions failed: {e}")

    async def resolve_contradiction(self, contradiction_id: str, action: str) -> Dict[str, Any]:
        warnings.warn(
            "resolve_contradiction() is deprecated; use consolidate() or contradict() instead.",
            DeprecationWarning, stacklevel=2,
        )
        url = self.ui_url + "/api/contradictions"
        try:
            resp = await self._t.post(url, json={"id": contradiction_id, "action": action})
            _raise_for_status(resp)
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"resolve_contradiction failed: {e}")


# ── Public API ───────────────────────────────────────────────────────────────


class SyncRemoteClient(
    _SyncRecordsMixin,
    _SyncSearchMixin,
    _SyncGraphMixin,
    _SyncProofMixin,
    _SyncSnapshotMixin,
    _SyncCollectionsMixin,
    _SyncMemoryMixin,
    _SyncTreeMixin,
    _SyncCommunityMixin,
    _SyncIngestMixin,
    _SyncCryptoMixin,
    _SyncKeysMixin,
    _SyncClusterMixin,
    _SyncIndexMixin,
    _SyncMetaMixin,
    ValoriClient,
):
    """Synchronous REST client for a Valoricore node — standalone or clustered.

    Against a Raft cluster, point ``base_url`` at *any* node. Reads are served
    locally; writes are transparently redirected to the current leader (HTTP 307)
    and the leader URL is cached for subsequent requests.

    ``ui_url`` is the optional Next.js UI server URL (default: base_url with
    port replaced by 3001). Required only for deprecated ``list_contradictions``
    and ``resolve_contradiction``.
    """

    def __init__(
        self,
        base_url: str,
        max_retries: int = 3,
        retry_backoff: float = 0.5,
        ui_url: Optional[str] = None,
        timeout: int = 10,
        token: Optional[str] = None,
    ):
        auth = _BearerAuth(token) if token else None
        self._t = _SyncTransport(
            base_url=base_url.rstrip("/"),
            auth=auth,
            timeout=timeout,
            max_retries=max_retries,
            retry_backoff=retry_backoff,
        )
        self.base_url = self._t.base_url
        self.ui_url = ui_url.rstrip("/") if ui_url else re.sub(r":\d+$", ":3001", self.base_url)
        self._auto_snapshot_interval: Optional[int] = None
        self._insert_count: int = 0
        import os as _os
        self._snapshot_dir: str = str(_os.path.join(_os.path.expanduser("~"), ".valori", "snapshots"))

    # ── Backward-compat shims ─────────────────────────────────────────────────

    @property
    def session(self) -> requests.Session:
        """Expose the underlying Session for callers that read it directly."""
        return self._t._session

    @property
    def _leader_url(self) -> Optional[str]:
        return self._t._leader_url

    @_leader_url.setter
    def _leader_url(self, v: Optional[str]) -> None:
        self._t._leader_url = v

    # ── Context manager ───────────────────────────────────────────────────────

    def close(self) -> None:
        self._t.close()

    def __enter__(self) -> "SyncRemoteClient":
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()


class AsyncRemoteClient(
    _AsyncRecordsMixin,
    _AsyncSearchMixin,
    _AsyncGraphMixin,
    _AsyncProofMixin,
    _AsyncSnapshotMixin,
    _AsyncCollectionsMixin,
    _AsyncMemoryMixin,
    _AsyncTreeMixin,
    _AsyncCommunityMixin,
    _AsyncIngestMixin,
    _AsyncCryptoMixin,
    _AsyncKeysMixin,
    _AsyncClusterMixin,
    _AsyncIndexMixin,
    _AsyncMetaMixin,
    ValoriClient,
):
    """Asynchronous REST client for a Valoricore node (httpx-backed).

    ``ui_url`` is the optional Next.js UI server URL (default: base_url with
    port replaced by 3001). Required only for deprecated ``list_contradictions``
    and ``resolve_contradiction``.
    """

    def __init__(
        self,
        base_url: str,
        max_retries: int = 3,
        retry_backoff: float = 0.5,
        ui_url: Optional[str] = None,
        token: Optional[str] = None,
        timeout: float = 10.0,
    ):
        self._t = _AsyncTransport(
            base_url=base_url.rstrip("/"),
            token=token,
            timeout=timeout,
            max_retries=max_retries,
            retry_backoff=retry_backoff,
        )
        self.base_url = self._t.base_url
        self.ui_url = ui_url.rstrip("/") if ui_url else re.sub(r":\d+$", ":3001", self.base_url)
        self._auto_snapshot_interval: Optional[int] = None
        self._insert_count: int = 0
        self._snapshot_dir: str = "./valoricore_snapshots"

    @property
    def _leader_url(self) -> Optional[str]:
        return self._t._leader_url

    @_leader_url.setter
    def _leader_url(self, v: Optional[str]) -> None:
        self._t._leader_url = v

    # ValoriClient abstract methods (sync stubs — use the async variants)
    def insert(self, *a, **kw):  # type: ignore[override]
        raise RuntimeError("Use `await client.insert(...)` — this client is async")

    def insert_batch(self, *a, **kw):  # type: ignore[override]
        raise RuntimeError("Use `await client.insert_batch(...)` — this client is async")

    def delete(self, *a, **kw):  # type: ignore[override]
        raise RuntimeError("Use `await client.delete(...)` — this client is async")

    def search(self, *a, **kw):  # type: ignore[override]
        raise RuntimeError("Use `await client.search(...)` — this client is async")

    def record_count(self) -> int:  # type: ignore[override]
        raise RuntimeError("Use `await client.record_count()` — this client is async")

    def get_metadata(self, *a, **kw):  # type: ignore[override]
        raise RuntimeError("Use `await client.get_metadata(...)` — this client is async")

    def set_metadata(self, *a, **kw):  # type: ignore[override]
        raise RuntimeError("Use `await client.set_metadata(...)` — this client is async")

    def get_state_hash(self) -> str:  # type: ignore[override]
        raise RuntimeError("Use `await client.get_state_hash()` — this client is async")

    async def close(self) -> None:
        await self._t.close()

    async def __aenter__(self) -> "AsyncRemoteClient":
        return self

    async def __aexit__(self, *_: Any) -> None:
        await self.close()


class ClusterClient:
    """Multi-node cluster client — routes writes to the leader, round-robins reads.

    Point it at all the nodes in your cluster; the client discovers the leader
    automatically via the first 307 redirect and caches it. Local reads are
    spread across all nodes; linearizable reads go to the leader.

    Usage::

        c = ClusterClient([
            "http://node1:3000",
            "http://node2:3000",
            "http://node3:3000",
        ], token="your-auth-token")

        rid = c.insert([0.1, 0.2, 0.3, 0.4])
        hits = c.search([0.1, 0.2, 0.3, 0.4], k=5, consistency="local")
    """

    def __init__(
        self,
        nodes: List[str],
        max_retries: int = 3,
        retry_backoff: float = 0.5,
        ui_url: Optional[str] = None,
        token: Optional[str] = None,
    ):
        if not nodes:
            raise ValueError("ClusterClient requires at least one node URL")
        self._clients = [
            SyncRemoteClient(url, max_retries=max_retries, retry_backoff=retry_backoff,
                             ui_url=ui_url, token=token)
            for url in nodes
        ]
        self._rr_idx = 0

    def leader_url(self) -> Optional[str]:
        for c in self._clients:
            if c._leader_url is not None:
                return c._leader_url
        return None

    def _write_client(self) -> SyncRemoteClient:
        for c in self._clients:
            if c._leader_url is not None:
                return c
        return self._clients[0]

    def _read_client(self, consistency: str = "local") -> SyncRemoteClient:
        if consistency == "linearizable":
            return self._write_client()
        c = self._clients[self._rr_idx % len(self._clients)]
        self._rr_idx += 1
        return c

    def insert(self, vector: Vector, tag: int = 0, collection: str = "default",
               idempotency_key: Optional[bytes] = None) -> RecordId:
        return self._write_client().insert(vector, tag=tag, collection=collection,
                                           idempotency_key=idempotency_key)

    def insert_batch(self, batch: List[Vector], collection: str = "default",
                     metadata: Optional[List[Optional[Dict[str, Any]]]] = None,
                     request_ids: Optional[List[Optional[str]]] = None,
                     texts: Optional[List[Optional[str]]] = None) -> List[RecordId]:
        return self._write_client().insert_batch(batch, collection=collection,
                                                  metadata=metadata, request_ids=request_ids,
                                                  texts=texts)

    def delete(self, record_id: int, collection: str = "default",
               idempotency_key: Optional[bytes] = None) -> None:
        self._write_client().delete(record_id, collection=collection, idempotency_key=idempotency_key)

    def soft_delete(self, record_id: int, collection: str = "default",
                    idempotency_key: Optional[bytes] = None) -> None:
        self._write_client().soft_delete(record_id, collection=collection, idempotency_key=idempotency_key)

    def create_collection(self, name: str) -> Dict[str, Any]:
        return self._write_client().create_collection(name)

    def drop_collection(self, name: str) -> None:
        self._write_client().drop_collection(name)

    def restore(self, data: bytes) -> None:
        self._write_client().restore(data)

    def search(self, query: Vector, k: int, filter_tag: Optional[int] = None,
               consistency: str = "local", collection: str = "default",
               **kwargs: Any) -> Any:
        return self._read_client(consistency).search(
            query, k, filter_tag=filter_tag, consistency=consistency,
            collection=collection, **kwargs)

    def graphrag(self, query_vector: Vector, k: int = 5, depth: int = 2,
                 collection: str = "default", consistency: str = "local") -> Dict[str, Any]:
        return self._read_client(consistency).graphrag(
            query_vector, k=k, depth=depth, collection=collection, consistency=consistency)

    def consolidate(self, old_record_id: int, new_vector: Vector,
                    collection: str = "default",
                    metadata: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        return self._write_client().consolidate(old_record_id, new_vector,
                                                 collection=collection, metadata=metadata)

    def contradict(self, record_a: int, record_b: int,
                   threshold: Optional[float] = None,
                   collection: str = "default") -> Dict[str, Any]:
        return self._write_client().contradict(record_a, record_b,
                                                threshold=threshold, collection=collection)

    def list_collections(self) -> List[Dict[str, Any]]:
        return self._read_client().list_collections()

    def get_state_hash(self) -> str:
        return self._read_client().get_state_hash()

    def event_log_proof(self) -> Dict[str, Any]:
        return self._read_client().event_log_proof()

    def get_receipt(self) -> Dict[str, Any]:
        return self._read_client().get_receipt()

    def get_receipt_by_id(self, receipt_id: str) -> Dict[str, Any]:
        return self._read_client().get_receipt_by_id(receipt_id)

    def timeline(self, from_ts: Optional[str] = None, to_ts: Optional[str] = None,
                 collection: Optional[str] = None) -> Dict[str, Any]:
        return self._read_client().timeline(from_ts=from_ts, to_ts=to_ts, collection=collection)

    def snapshot(self) -> bytes:
        return self._read_client().snapshot()

    def cluster_status(self) -> Dict[str, Any]:
        return self._read_client().cluster_status()

    def cluster_health(self) -> bool:
        return any(c.cluster_health() for c in self._clients)

    def get_cluster_role(self) -> str:
        return self._write_client().get_cluster_role()

    def create_key(self, scope: str = "read_write", collection: Optional[str] = None,
                   description: Optional[str] = None) -> Dict[str, Any]:
        return self._write_client().create_key(scope=scope, collection=collection,
                                                description=description)

    def list_keys(self) -> List[Dict[str, Any]]:
        return self._write_client().list_keys()

    def revoke_key(self, key_id: str) -> None:
        self._write_client().revoke_key(key_id)

    def close(self) -> None:
        for c in self._clients:
            c.close()

    def __enter__(self) -> "ClusterClient":
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()


class AsyncClusterClient:
    """Async multi-node cluster client. Mirrors :class:`ClusterClient`."""

    def __init__(
        self,
        nodes: List[str],
        max_retries: int = 3,
        retry_backoff: float = 0.5,
        ui_url: Optional[str] = None,
        token: Optional[str] = None,
    ):
        if not nodes:
            raise ValueError("AsyncClusterClient requires at least one node URL")
        self._clients = [
            AsyncRemoteClient(url, max_retries=max_retries, retry_backoff=retry_backoff,
                              ui_url=ui_url, token=token)
            for url in nodes
        ]
        self._rr_idx = 0

    def leader_url(self) -> Optional[str]:
        for c in self._clients:
            if c._leader_url is not None:
                return c._leader_url
        return None

    def _write_client(self) -> AsyncRemoteClient:
        for c in self._clients:
            if c._leader_url is not None:
                return c
        return self._clients[0]

    def _read_client(self, consistency: str = "local") -> AsyncRemoteClient:
        if consistency == "linearizable":
            return self._write_client()
        c = self._clients[self._rr_idx % len(self._clients)]
        self._rr_idx += 1
        return c

    async def insert(self, vector: Vector, tag: int = 0, collection: str = "default",
                     text: Optional[str] = None) -> RecordId:
        return await self._write_client().insert(vector, tag=tag, collection=collection, text=text)

    async def insert_batch(self, batch: List[Vector], collection: str = "default",
                           metadata: Optional[List[Optional[str]]] = None,
                           request_ids: Optional[List[Optional[str]]] = None,
                           texts: Optional[List[Optional[str]]] = None) -> List[RecordId]:
        return await self._write_client().insert_batch(
            batch, collection=collection, metadata=metadata,
            request_ids=request_ids, texts=texts)

    async def delete(self, record_id: int, collection: str = "default") -> None:
        await self._write_client().delete(record_id, collection=collection)

    async def soft_delete(self, record_id: int, collection: str = "default") -> None:
        await self._write_client().soft_delete(record_id, collection=collection)

    async def create_collection(self, name: str) -> Dict[str, Any]:
        return await self._write_client().create_collection(name)

    async def drop_collection(self, name: str) -> None:
        await self._write_client().drop_collection(name)

    async def search(self, query: Vector, k: int, filter_tag: Optional[int] = None,
                     consistency: str = "local", collection: str = "default",
                     **kwargs: Any) -> Any:
        return await self._read_client(consistency).search(
            query, k, filter_tag=filter_tag, consistency=consistency,
            collection=collection, **kwargs)

    async def graphrag(self, query_vector: Vector, k: int = 5, depth: int = 2,
                       collection: str = "default", consistency: str = "local") -> Dict[str, Any]:
        return await self._read_client(consistency).graphrag(
            query_vector, k=k, depth=depth, collection=collection, consistency=consistency)

    async def consolidate(self, old_record_id: int, new_vector: Vector,
                          collection: str = "default",
                          metadata: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        return await self._write_client().consolidate(old_record_id, new_vector,
                                                       collection=collection, metadata=metadata)

    async def contradict(self, record_a: int, record_b: int,
                         threshold: Optional[float] = None,
                         collection: str = "default") -> Dict[str, Any]:
        return await self._write_client().contradict(record_a, record_b,
                                                      threshold=threshold, collection=collection)

    async def list_collections(self) -> List[Dict[str, Any]]:
        return await self._read_client().list_collections()

    async def get_state_hash(self) -> str:
        return await self._read_client().get_state_hash()

    async def event_log_proof(self) -> Dict[str, Any]:
        return await self._read_client().event_log_proof()

    async def get_receipt(self) -> Dict[str, Any]:
        return await self._read_client().get_receipt()

    async def get_receipt_by_id(self, receipt_id: str) -> Dict[str, Any]:
        return await self._read_client().get_receipt_by_id(receipt_id)

    async def timeline(self, from_ts: Optional[str] = None, to_ts: Optional[str] = None,
                       collection: Optional[str] = None) -> Dict[str, Any]:
        return await self._read_client().timeline(from_ts=from_ts, to_ts=to_ts, collection=collection)

    async def cluster_status(self) -> Dict[str, Any]:
        return await self._read_client().cluster_status()

    async def cluster_health(self) -> bool:
        import asyncio
        results = await asyncio.gather(*[c.cluster_health() for c in self._clients],
                                        return_exceptions=True)
        return any(r is True for r in results)

    async def get_cluster_role(self) -> str:
        return await self._write_client().get_cluster_role()

    async def close(self) -> None:
        import asyncio
        await asyncio.gather(*[c.close() for c in self._clients])

    async def __aenter__(self) -> "AsyncClusterClient":
        return self

    async def __aexit__(self, *_: Any) -> None:
        await self.close()


class RemoteClient(SyncRemoteClient):
    """Deprecated: Use SyncRemoteClient instead."""
    def __init__(self, *args, **kwargs):
        warnings.warn(
            "RemoteClient is deprecated. Use SyncRemoteClient instead.",
            DeprecationWarning, stacklevel=2,
        )
        super().__init__(*args, **kwargs)
