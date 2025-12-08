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
         # Not yet exposed in remote client in previous phase, but node has it.
         # Node API: GET /snapshot?
         # Wait, node/src/server.rs impl: "/snapshot" get -> handle_snapshot
         url = self.base_url + "/snapshot"
         resp = self.session.get(url)
         resp.raise_for_status()
         return resp.content

    def restore(self, data: bytes) -> None:
         # Node API: POST /restore body=bytes
         url = self.base_url + "/restore"
         resp = self.session.post(url, data=data)
         resp.raise_for_status()
