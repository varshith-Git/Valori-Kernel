import requests
from typing import List, Dict, Optional, Any

class Client:
    def __init__(self, base_url: str = "http://127.0.0.1:3000"):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()

    def _post(self, path: str, json_data: Dict[str, Any]) -> Dict[str, Any]:
        url = self.base_url + path
        resp = self.session.post(url, json=json_data)
        resp.raise_for_status()
        return resp.json()

    def insert_record(self, values: List[float]) -> int:
        """Insert a vector record. Returns the new Record ID."""
        data = {"values": values}
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
