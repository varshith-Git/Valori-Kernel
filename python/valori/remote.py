# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import requests
from typing import List, Dict, Optional, Any

class RemoteClient:
    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()

    def _post(self, path: str, json_data: Dict[str, Any]) -> Dict[str, Any]:
        url = self.base_url + path
        resp = self.session.post(url, json=json_data)
        resp.raise_for_status()
        return resp.json()

    def insert(self, vector: List[float]) -> int:
        """Insert a vector record. Returns the new Record ID."""
        data = {"values": vector}
        resp = self._post("/records", data)
        return resp["id"]

    def insert_batch(self, batch: List[List[float]]) -> List[int]:
        """Insert a batch of vectors. Returns list of new Record IDs."""
        data = {"batch": batch}
        resp = self._post("/v1/vectors/batch_insert", data)
        return resp["ids"]

    def search(self, query: List[float], k: int) -> List[Dict[str, Any]]:
        """Search for nearest vectors. Returns list of hits [{'id': int, 'score': int}]."""
        data = {"query": query, "k": k}
        resp = self._post("/search", data)
        return resp["results"]

    def create_node(self, kind: int, record_id: Optional[int] = None) -> int:
        """Create a graph node. Returns Node ID."""
        data = {"kind": kind, "record_id": record_id}
        resp = self._post("/graph/node", data)
        return resp["node_id"]

    def create_edge(self, from_id: int, to_id: int, kind: int) -> int:
        """Create a graph edge. Returns Edge ID."""
        data = {"from": from_id, "to": to_id, "kind": kind}
        resp = self._post("/graph/edge", data)
        return resp["edge_id"]

    def snapshot(self) -> bytes:
        """Download snapshot from remote."""
        url = self.base_url + "/snapshot"
        resp = self.session.post(url)
        resp.raise_for_status()
        return resp.content

    def restore(self, data: bytes) -> None:
        """Upload snapshot to remote."""
        url = self.base_url + "/restore"
        headers = {"Content-Type": "application/octet-stream"}
        resp = self.session.post(url, data=data, headers=headers)
        resp.raise_for_status()
