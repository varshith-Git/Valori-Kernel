# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import time
import requests
import warnings
from typing import List, Dict, Optional, Any, Tuple
from uuid import uuid4
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

    ``ui_url`` is the optional Next.js UI server URL (default: base_url with
    port replaced by 3001). Required only for middleware-layer APIs such as
    ``list_contradictions`` and ``resolve_contradiction``, which live in the
    Next.js API layer rather than the Rust kernel.
    """

    def __init__(self, base_url: str, max_retries: int = 3, retry_backoff: float = 0.5,
                 ui_url: Optional[str] = None):
        self.base_url = base_url.rstrip("/")
        # UI layer URL — defaults to same host but port 3001
        if ui_url:
            self.ui_url = ui_url.rstrip("/")
        else:
            import re
            self.ui_url = re.sub(r":\d+$", ":3001", self.base_url)
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

    def _post(
        self,
        path: str,
        json_data: Dict[str, Any],
        idempotency_key: Optional[bytes] = None,
    ) -> Dict[str, Any]:
        """POST with cluster awareness.

        ``requests`` follows the leader's 307 redirect automatically (the POST
        body and method are preserved). We additionally (a) prefer a cached
        leader URL so the common case skips the redirect, (b) learn the leader
        from any redirect that did occur, and (c) retry on transient 503
        / connection errors during an election.

        ``idempotency_key`` — 16 raw bytes (a UUID) injected as ``request_id``
        in the JSON body and kept identical across all retry attempts so the
        server can deduplicate a write that was already applied before the
        connection was lost.
        """
        if idempotency_key is not None:
            json_data = {**json_data, "request_id": list(idempotency_key)}
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

    def insert(
        self,
        vector: Vector,
        tag: int = 0,
        collection: str = "default",
        idempotency_key: Optional[bytes] = None,
    ) -> RecordId:
        """Insert a vector record. Returns the new Record ID.

        ``collection`` routes the record into a named namespace.  Create
        collections first with ``create_collection(name)``; the default
        collection always exists.

        ``idempotency_key`` — 16-byte token (defaults to a fresh UUID4) used
        to deduplicate retried writes on a Raft cluster. Pass the same bytes
        to guarantee exactly-once delivery even when a retry follows a
        connection reset. Ignored by standalone nodes.
        """
        data: Dict[str, Any] = {"values": vector, "tag": tag}
        if collection != "default":
            data["collection"] = collection
        key = idempotency_key if idempotency_key is not None else uuid4().bytes
        resp = self._post("/records", data, idempotency_key=key)
        self._check_auto_snapshot(1)
        return resp["id"]

    def insert_with_proof(self, vector: Vector, tag: int = 0, collection: str = "default") -> Tuple[RecordId, Proof]:
        """Insert a vector and return (id, proof_bytes)."""
        import valoricore
        fixed_vals = valoricore.ingest_embedding(vector)
        proof_hex = valoricore.generate_proof(fixed_vals)
        proof_bytes = bytes.fromhex(proof_hex)
        rid = self.insert(vector, tag=tag, collection=collection)
        return (rid, proof_bytes)

    def insert_batch(
        self,
        batch: List[Vector],
        collection: str = "default",
        metadata: Optional[List[Optional[str]]] = None,
        request_ids: Optional[List[Optional[str]]] = None,
    ) -> List[RecordId]:
        """Insert a batch of vectors with optional per-item idempotency keys.

        Args:
            metadata: Optional per-vector context strings (UTF-8 JSON).
                      When provided, committed into the BLAKE3 audit chain.
                      Length must match ``batch``.
            request_ids: Optional per-vector idempotency keys (32-hex strings).
                         A duplicate key causes that item to be skipped and the
                         previously assigned ID returned. Length must match ``batch``.

        Returns:
            List of record IDs (existing ID for deduped items, new ID otherwise).
        """
        data: Dict[str, Any] = {"batch": batch}
        if collection != "default":
            data["collection"] = collection
        if metadata is not None:
            data["metadata"] = metadata
        if request_ids is not None:
            data["request_ids"] = request_ids
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

    def soft_delete(self, record_id: int, idempotency_key: Optional[bytes] = None) -> None:
        """Mark a record as inactive without physically removing it."""
        key = idempotency_key if idempotency_key is not None else uuid4().bytes
        self._post("/v1/soft-delete", {"id": record_id}, idempotency_key=key)

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
    ) -> List[Dict[str, Any]]:
        """Search for nearest vectors. Returns list of hits [{'id': int, 'score': int}].

        ``collection`` scopes the search to a specific namespace.
        ``consistency`` applies in cluster mode: ``"linearizable"`` (the server
        default) reflects every write committed before the read, via the
        read-index protocol; ``"local"`` serves immediately from the queried
        node and may lag (eventually consistent, but no leader round trip).
        Ignored by a standalone node.

        ``as_of`` — ISO 8601 UTC timestamp, e.g. ``"2026-03-03T00:00:00Z"``.
        Searches the vector state as it existed at that moment. Requires
        ``VALORI_EVENT_LOG_PATH`` to be set on the node. Returns a full
        response dict (not just the results list) that includes
        ``as_of_log_index``, ``as_of_timestamp_iso``, and ``as_of_state_hash``.

        ``as_of_log_index`` — search the state after exactly this many committed
        events. Takes precedence over ``as_of`` if both are given.

        ``decay_half_life_secs`` (Phase C4.1) — recency-aware ranking. When set
        (> 0), older records decay: a record one half-life old has its distance
        doubled, so a fresh near-match can overtake a stale better one. Each hit
        then carries ``decay_factor`` and ``age_secs``. ``score`` stays the true
        (undecayed) distance. Decay is a read-time re-rank — it never changes the
        kernel state hash. Ignored for ``as_of`` queries. (Standalone only in
        v1; accepted-but-neutral on cluster nodes.)
        """
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
        resp = self._post("/search", data)
        # as-of searches return the full response dict (with proof fields).
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
        """GraphRAG: the k nearest vectors AND the connected knowledge subgraph
        around them, retrieved in one call from a single consistent snapshot.

        Returns ``{"hits": [...], "seed_nodes": [...],
        "subgraph": {"nodes": [...], "edges": [...]}}`` where each hit is
        ``{"memory_id", "record_id", "score", "node_id", "metadata"}``.

        ``depth`` is the graph hop limit (clamped to 4 server-side). ``collection``
        scopes the vector search. ``consistency`` applies in cluster mode
        (``"linearizable"`` | ``"local"``); ignored by a standalone node.

        The subgraph is only as rich as the edges that exist — ingest creates a
        document→chunk edge per memory; entity/citation edges add more.
        """
        data: Dict[str, Any] = {"query_vector": query_vector, "k": k, "depth": depth}
        if collection != "default":
            data["collection"] = collection
        if consistency is not None:
            data["consistency"] = consistency
        return self._post("/v1/graphrag", data)

    def consolidate(
        self,
        old_record_id: int,
        new_vector: Vector,
        collection: str = "default",
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        """Consolidate a memory (Phase C4.2): replace ``old_record_id`` with a
        new vector, committing three events to the BLAKE3 audit chain in one
        operation — ``SoftDeleteRecord`` (old) → ``AutoInsertRecord`` (new) →
        ``AutoCreateEdge(Supersedes)`` (new → old).

        The old record is soft-deleted (preserved in the chain, excluded from
        search); the Supersedes edge makes the replacement auditable and lets a
        reader trace why the old memory was retired.

        Returns ``{"old_record_id", "new_record_id", "supersedes_edge_id",
        "state_hash"}``.
        """
        data: Dict[str, Any] = {
            "old_record_id": old_record_id,
            "new_vector": new_vector,
        }
        if collection != "default":
            data["collection"] = collection
        if metadata is not None:
            data["metadata"] = metadata
        return self._post("/v1/memory/consolidate", data)

    def contradict(
        self,
        record_a: int,
        record_b: int,
        threshold: Optional[float] = None,
        collection: str = "default",
    ) -> Dict[str, Any]:
        """Detect contradiction between two memories (Phase C4.3).

        Computes cosine similarity between the two record vectors. If it meets
        ``threshold`` (default 0.85 server-side), a ``Contradicts`` edge
        (``record_a`` → ``record_b``) is committed to the audit chain; otherwise
        nothing is written.

        Returns ``{"record_a", "record_b", "similarity", "contradicts",
        "edge_id"?, "state_hash"}``. ``edge_id`` is present only when
        ``contradicts`` is true.
        """
        data: Dict[str, Any] = {"record_a": record_a, "record_b": record_b}
        if threshold is not None:
            data["threshold"] = threshold
        if collection != "default":
            data["collection"] = collection
        return self._post("/v1/memory/contradict", data)

    # ── Agent-memory primitives (return memory_id + graph nodes + decay) ─────────

    def memory_upsert(
        self,
        vector: Vector,
        collection: str = "default",
        attach_to_document_node: Optional[int] = None,
        metadata: Optional[Dict[str, Any]] = None,
        tags: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        """Insert a memory the agent-memory way: stores the vector **and** wires
        a document→chunk graph (``ParentOf`` edge), returning a stable
        ``memory_id`` plus the created graph node IDs.

        Prefer this over :meth:`insert` when you want the memory addressable by
        ``memory_id`` and linked into the knowledge graph (so GraphRAG and the
        provenance receipts can traverse it). ``attach_to_document_node`` reuses
        an existing document node instead of creating a new one.

        Returns ``{"memory_id", "record_id", "document_node_id",
        "chunk_node_id"}``.
        """
        data: Dict[str, Any] = {"vector": vector}
        if collection != "default":
            data["collection"] = collection
        if attach_to_document_node is not None:
            data["attach_to_document_node"] = attach_to_document_node
        if metadata is not None:
            data["metadata"] = metadata
        if tags is not None:
            data["tags"] = tags
        return self._post("/v1/memory/upsert_vector", data)

    def memory_search(
        self,
        query_vector: Vector,
        k: int = 5,
        collection: str = "default",
        decay_half_life_secs: Optional[int] = None,
    ) -> List[Dict[str, Any]]:
        """Agent-memory search: like :meth:`search` but each hit carries the
        stable ``memory_id`` and any stored ``metadata`` (and, when decay is
        active, ``decay_factor`` + ``age_secs``).

        ``decay_half_life_secs`` (Phase C4.1) — recency-aware ranking; older
        memories fade. ``score`` stays the true (undecayed) distance.

        Returns a list of ``{"memory_id", "record_id", "score", "metadata",
        "decay_factor"?, "age_secs"?}``.
        """
        data: Dict[str, Any] = {"query_vector": query_vector, "k": k}
        if collection != "default":
            data["collection"] = collection
        if decay_half_life_secs is not None:
            data["decay_half_life_secs"] = decay_half_life_secs
        return self._post("/v1/memory/search_vector", data)["results"]

    # ── Proof / provenance ──────────────────────────────────────────────────────

    def event_log_proof(self) -> Dict[str, Any]:
        """Return the event-log proof: the BLAKE3 hash of the committed event
        log plus the final state hash, snapshot hash, event count, and committed
        height. This is the receipt primitive — an external client can replay the
        log and check it reproduces ``final_state_hash`` at ``committed_height``.

        Returns ``{"kernel_version", "event_log_hash", "final_state_hash",
        "snapshot_hash"?, "event_count", "committed_height"}``.
        """
        url = self.base_url + "/v1/proof/event-log"
        try:
            resp = self.session.get(url, timeout=5)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get event-log proof: {e}")

    def get_version(self) -> str:
        """Return the node's software version (``CARGO_PKG_VERSION``)."""
        url = self.base_url + "/version"
        try:
            resp = self.session.get(url, timeout=5)
            resp.raise_for_status()
            return resp.text.strip()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get version: {e}")

    def list_nodes(self, collection: str = "default") -> Dict[str, Any]:
        """List graph nodes in a collection.

        Returns ``{"nodes": [{"node_id", "kind", "record_id", "namespace_id"}],
        "count"}``.
        """
        url = self.base_url + "/graph/nodes"
        params = {} if collection == "default" else {"collection": collection}
        try:
            resp = self.session.get(url, params=params, timeout=5)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list nodes: {e}")

    # ── Snapshot / object-store offload (Phase 3.1) ─────────────────────────────

    def save_snapshot(self, path: Optional[str] = None) -> Dict[str, Any]:
        """Write a snapshot to the node's local filesystem. ``path`` overrides
        the configured ``VALORI_SNAPSHOT_PATH``. Returns ``{"success", "path"}``.
        """
        data: Dict[str, Any] = {}
        if path is not None:
            data["path"] = path
        return self._post("/v1/snapshot/save", data)

    def restore_snapshot(self, path: str) -> Dict[str, Any]:
        """Restore node state from a snapshot file already on the node's local
        filesystem at ``path`` (the counterpart to :meth:`save_snapshot`). To
        restore from raw bytes held client-side, use :meth:`restore` instead.
        Returns ``{"success"}``.
        """
        return self._post("/v1/snapshot/restore", {"path": path})

    def list_remote_snapshots(self) -> Dict[str, Any]:
        """List snapshots in the configured object store (S3/MinIO/R2).
        Requires ``VALORI_OBJECT_STORE_URL`` on the node.
        Returns ``{"snapshots": [...], "count"}``.
        """
        url = self.base_url + "/v1/storage/snapshots"
        try:
            resp = self.session.get(url, timeout=15)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list remote snapshots: {e}")

    def upload_snapshot_to_store(self) -> Dict[str, Any]:
        """Snapshot current state and upload it to the object store, pruning to
        ``VALORI_OBJECT_STORE_KEEP``. Returns the upload result (key, size,
        state hash). Requires an object store configured on the node.
        """
        return self._post("/v1/storage/snapshots/upload", {})

    def restore_from_store(self, key: str) -> Dict[str, Any]:
        """Download a snapshot by object-store ``key`` and restore the node's
        state from it. Returns ``{"key", "state_hash", "size_bytes"}``.
        """
        return self._post("/v1/storage/snapshots/restore", {"key": key})

    def list_remote_wal(self) -> Dict[str, Any]:
        """List archived WAL segments in the object store.
        Returns ``{"segments": [...], "count"}``.
        """
        url = self.base_url + "/v1/storage/wal"
        try:
            resp = self.session.get(url, timeout=15)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list remote WAL: {e}")

    def archive_wal_segment(self, path: str) -> Dict[str, Any]:
        """Archive a sealed WAL segment (absolute local ``path`` on the node) to
        the object store. Returns ``{"key", "size_bytes"}``.
        """
        return self._post("/v1/storage/wal/archive", {"path": path})

    def timeline(
        self,
        from_ts: Optional[str] = None,
        to_ts: Optional[str] = None,
        collection: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Return the committed event timeline.

        ``from_ts`` / ``to_ts`` are ISO 8601 UTC strings that filter the
        window of events returned. Requires ``VALORI_EVENT_LOG_PATH``.

        Returns a dict with ``events`` (list of entries), ``total``,
        ``from_unix``, and ``to_unix``.
        """
        params: Dict[str, str] = {}
        if from_ts:
            params["from"] = from_ts
        if to_ts:
            params["to"] = to_ts
        if collection:
            params["collection"] = collection
        resp = self.session.get(f"{self.base_url}/v1/timeline", params=params)
        resp.raise_for_status()
        return resp.json()

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

    # ── Collection (namespace) management ─────────────────────────────────────

    def create_collection(self, name: str) -> Dict[str, Any]:
        """Create a new collection (namespace).  Idempotent — returns existing
        id if the collection was already created.

        Returns: ``{"name": str, "id": int, "created": bool}``
        """
        return self._post("/v1/namespaces", {"name": name})

    def list_collections(self) -> List[Dict[str, Any]]:
        """List all collections.

        Returns: list of ``{"name": str, "id": int}``
        """
        url = self.base_url + "/v1/namespaces"
        try:
            resp = self.session.get(url, timeout=5)
            resp.raise_for_status()
            return resp.json().get("collections", [])
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list collections: {e}")

    def drop_collection(self, name: str) -> None:
        """Drop a collection and all its records/nodes.

        Raises ``ValueError`` if the collection does not exist or is "default".
        """
        url = self.base_url + f"/v1/namespaces/{name}"
        try:
            resp = self.session.delete(url, timeout=5)
            if resp.status_code == 400:
                raise ValueError(resp.json().get("error", resp.text))
            resp.raise_for_status()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to drop collection '{name}': {e}")

    # ── Phase 3.6: Crypto-shredding ──────────────────────────────────────────

    def insert_encrypted(
        self,
        payload: bytes,
        tag: int = 0,
        collection: str = "default",
        key_id: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Encrypt *payload* with AES-256-GCM and store it.

        The plaintext is base64-encoded before sending.  The server generates a
        fresh DEK unless *key_id* (32 hex chars) is supplied.

        Returns ``{"id": int, "key_id": str}`` — keep *key_id* for later shredding.
        """
        import base64
        body: Dict[str, Any] = {
            "payload": base64.b64encode(payload).decode(),
            "tag": tag,
            "collection": collection,
        }
        if key_id is not None:
            body["key_id"] = key_id
        return self._post("/v1/records/encrypted", body)

    def shred_key(self, key_id: str) -> Dict[str, Any]:
        """Destroy the DEK *key_id* (GDPR Article 17 erasure).

        After this call, all records encrypted under *key_id* become permanently
        unrecoverable.  Returns ``{"key_id": str, "shredded": bool}``.
        """
        url = self.base_url + f"/v1/crypto/shred/{key_id}"
        try:
            resp = self.session.delete(url, timeout=5)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to shred key '{key_id}': {e}")

    def shred_key_status(self, key_id: str) -> Dict[str, Any]:
        """Check whether *key_id* still exists in the vault.

        Returns ``{"key_id": str, "exists": bool}``.
        """
        url = self.base_url + f"/v1/crypto/status/{key_id}"
        try:
            resp = self.session.get(url, timeout=5)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to check key status '{key_id}': {e}")

    def get_index_config(self) -> Dict[str, Any]:
        """Return current index type and HNSW parameters.

        Returns ``{"index_type": str, "hnsw": dict | None}``.
        For HNSW: ``{"m", "m_max0", "ef_construction", "ef_search"}``.
        """
        url = self.base_url + "/v1/index/config"
        try:
            resp = self.session.get(url, timeout=5)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to get index config: {e}")

    # ─────────────────────────────────────────────────────────────────────────

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

    def delete(self, record_id: int, idempotency_key: Optional[bytes] = None) -> None:
        """Permanently remove a record from the remote pool."""
        key = idempotency_key if idempotency_key is not None else uuid4().bytes
        self._post("/v1/delete", {"id": record_id}, idempotency_key=key)

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

    def leader_url(self) -> Optional[str]:
        """Return the cached leader base URL learned from the last 307 redirect.

        Returns ``None`` on a fresh client or after a leader failover resets
        the cache. The value updates automatically on the next successful write.
        """
        return self._leader_url

    def get_cluster_role(self) -> str:
        """Return this node's current Raft role: ``"leader"`` or ``"follower"``.

        Raises :class:`ConnectionError` if the node is standalone (endpoint 404s).
        """
        url = self.base_url + "/v1/cluster/role"
        try:
            resp = self.session.get(url, timeout=5)
            if resp.status_code == 404:
                raise ConnectionError("node is not running in cluster mode (/v1/cluster/role not found)")
            resp.raise_for_status()
            return resp.json().get("role", "unknown")
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to fetch cluster role from {url}: {e}")

    # ── API key management (Phase 3.5) ────────────────────────────────────────

    def create_key(
        self,
        scope: str = "read_write",
        collection: Optional[str] = None,
        description: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Create a new API key.  Requires admin credentials.

        ``scope`` — ``"read_only"``, ``"read_write"`` (default), or ``"admin"``.
        ``collection`` — lock the key to a single collection (optional).

        Returns the full key record including the plain-text ``token`` — shown
        only once and not stored server-side in plain text.
        """
        data: Dict[str, Any] = {"scope": scope}
        if collection is not None:
            data["collection"] = collection
        if description is not None:
            data["description"] = description
        return self._post("/v1/keys", data)

    def list_keys(self) -> List[Dict[str, Any]]:
        """List all API keys (masked — raw tokens are never returned).

        Requires admin credentials.  Returns a list of key records with
        ``id``, ``scope``, ``collection``, ``description``, ``created_at``,
        and ``prefix`` (first 8 chars of the token).
        """
        url = self.base_url + "/v1/keys"
        try:
            resp = self.session.get(url, timeout=5)
            resp.raise_for_status()
            return resp.json().get("keys", [])
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to list keys: {e}")

    def revoke_key(self, key_id: str) -> None:
        """Revoke an API key by ID.  Requires admin credentials.

        Raises ``NotFoundError`` if ``key_id`` does not exist.
        """
        url = self.base_url + f"/v1/keys/{key_id}"
        try:
            resp = self.session.delete(url, timeout=5)
            if resp.status_code == 404:
                raise NotFoundError(f"Key not found: {key_id}")
            resp.raise_for_status()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"Failed to revoke key '{key_id}': {e}")

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

    # ── Cortex: Knowledge graph ────────────────────────────────────────────────

    def subgraph(self, root_node: int, depth: int = 2) -> Dict[str, Any]:
        """Bounded BFS from ``root_node`` (depth capped at 4 server-side).

        Returns ``{"nodes": [...], "edges": [...]}`` where each node has
        ``id``, ``kind`` (NodeKind u8), and ``record`` (record_id or None),
        and each edge has ``id``, ``from``, ``to``, ``kind`` (EdgeKind u8).

        NodeKind: 0=Record, 1=Concept, 5=Document, 6=Chunk
        EdgeKind: 4=Mentions, 5=RefersTo, 6=ParentOf
        """
        url = self.base_url + f"/graph/subgraph?root={root_node}&depth={depth}"
        try:
            resp = self.session.get(url, timeout=5)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"subgraph failed: {e}")

    # ── Cortex: Contradiction queue ────────────────────────────────────────────

    def list_contradictions(
        self,
        collection: str = "default",
        status: str = "pending",
    ) -> Dict[str, Any]:
        """**Deprecated (Phase C4.3).** Legacy C3 review-queue read that calls
        the Next.js UI layer (``ui_url``), *not* the Valori node, and returns
        whatever that layer holds (historically ``[]``).

        Contradiction is now node-native and auditable: use :meth:`contradict`
        to commit a ``Contradicts`` edge to the BLAKE3 chain, and traverse those
        edges via :meth:`graphrag` / :meth:`get_edges`. This method will be
        removed in a future release.
        """
        warnings.warn(
            "list_contradictions() is deprecated; it queries the legacy UI layer, "
            "not the node. Use contradict() (node-native, audited) instead.",
            DeprecationWarning,
            stacklevel=2,
        )
        url = self.ui_url + f"/api/contradictions?collection={collection}&status={status}"
        try:
            resp = self.session.get(url, timeout=10)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"list_contradictions failed: {e}")

    def resolve_contradiction(
        self,
        contradiction_id: str,
        action: str,  # "dismiss" | "supersede_b"
    ) -> Dict[str, Any]:
        """**Deprecated (Phase C4.3).** Legacy C3 review-queue write to the
        Next.js UI layer (``ui_url``), *not* the Valori node.

        The node-native, audited replacements are :meth:`consolidate`
        (supersede a memory) and :meth:`contradict` (flag a conflict) — both
        commit events to the BLAKE3 chain. This method will be removed in a
        future release.
        """
        warnings.warn(
            "resolve_contradiction() is deprecated; it writes to the legacy UI layer, "
            "not the node. Use consolidate() or contradict() instead.",
            DeprecationWarning,
            stacklevel=2,
        )
        url = self.ui_url + "/api/contradictions"
        try:
            resp = self.session.post(url, json={"id": contradiction_id, "action": action}, timeout=5)
            resp.raise_for_status()
            return resp.json()
        except requests.exceptions.RequestException as e:
            raise ConnectionError(f"resolve_contradiction failed: {e}")

class AsyncRemoteClient:
    """Asynchronous REST client for a standalone Valoricore node using httpx.

    ``ui_url`` is the optional Next.js UI server URL (default: base_url with
    port replaced by 3001). Required for ``list_contradictions`` and
    ``resolve_contradiction`` which live in the Next.js API layer.
    """

    def __init__(self, base_url: str, max_retries: int = 3, retry_backoff: float = 0.5,
                 ui_url: Optional[str] = None):
        import httpx
        self.base_url = base_url.rstrip("/")
        if ui_url:
            self.ui_url = ui_url.rstrip("/")
        else:
            import re
            self.ui_url = re.sub(r":\d+$", ":3001", self.base_url)
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

    async def insert(self, vector: Vector, tag: int = 0, collection: str = "default") -> RecordId:
        data: Dict[str, Any] = {"values": vector, "tag": tag}
        if collection != "default":
            data["collection"] = collection
        resp = await self._post("/records", data)
        await self._check_auto_snapshot(1)
        return resp["id"]

    async def insert_with_proof(self, vector: Vector, tag: int = 0, collection: str = "default") -> Tuple[RecordId, Proof]:
        import valoricore
        fixed_vals = valoricore.ingest_embedding(vector)
        proof_hex = valoricore.generate_proof(fixed_vals)
        proof_bytes = bytes.fromhex(proof_hex)
        rid = await self.insert(vector, tag=tag, collection=collection)
        return (rid, proof_bytes)

    async def insert_batch(
        self,
        batch: List[Vector],
        collection: str = "default",
        metadata: Optional[List[Optional[str]]] = None,
        request_ids: Optional[List[Optional[str]]] = None,
    ) -> List[RecordId]:
        """Insert a batch of vectors with optional per-item idempotency keys."""
        data: Dict[str, Any] = {"batch": batch}
        if collection != "default":
            data["collection"] = collection
        if metadata is not None:
            data["metadata"] = metadata
        if request_ids is not None:
            data["request_ids"] = request_ids
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
    ) -> List[Dict[str, Any]]:
        """See SyncRemoteClient.search. ``consistency`` is "linearizable" | "local".
        ``as_of`` and ``as_of_log_index`` enable point-in-time search (see sync docs).
        ``decay_half_life_secs`` (Phase C4.1) enables recency-aware re-ranking."""
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
        resp = await self._post("/search", data)
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
        """Async version of SyncRemoteClient.graphrag — k nearest vectors plus the
        connected subgraph in one call. Returns ``{"hits", "seed_nodes", "subgraph"}``."""
        data: Dict[str, Any] = {"query_vector": query_vector, "k": k, "depth": depth}
        if collection != "default":
            data["collection"] = collection
        if consistency is not None:
            data["consistency"] = consistency
        return await self._post("/v1/graphrag", data)

    async def consolidate(
        self,
        old_record_id: int,
        new_vector: Vector,
        collection: str = "default",
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        """Async version of SyncRemoteClient.consolidate (Phase C4.2). Replaces
        ``old_record_id`` with ``new_vector`` and commits a Supersedes edge.
        Returns ``{"old_record_id", "new_record_id", "supersedes_edge_id",
        "state_hash"}``."""
        data: Dict[str, Any] = {
            "old_record_id": old_record_id,
            "new_vector": new_vector,
        }
        if collection != "default":
            data["collection"] = collection
        if metadata is not None:
            data["metadata"] = metadata
        return await self._post("/v1/memory/consolidate", data)

    async def contradict(
        self,
        record_a: int,
        record_b: int,
        threshold: Optional[float] = None,
        collection: str = "default",
    ) -> Dict[str, Any]:
        """Async version of SyncRemoteClient.contradict (Phase C4.3). Commits a
        Contradicts edge when cosine similarity ≥ threshold. Returns
        ``{"record_a", "record_b", "similarity", "contradicts", "edge_id"?,
        "state_hash"}``."""
        data: Dict[str, Any] = {"record_a": record_a, "record_b": record_b}
        if threshold is not None:
            data["threshold"] = threshold
        if collection != "default":
            data["collection"] = collection
        return await self._post("/v1/memory/contradict", data)

    # ── Agent-memory primitives ─────────────────────────────────────────────────

    async def memory_upsert(
        self,
        vector: Vector,
        collection: str = "default",
        attach_to_document_node: Optional[int] = None,
        metadata: Optional[Dict[str, Any]] = None,
        tags: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        """Async version of SyncRemoteClient.memory_upsert. Returns
        ``{"memory_id", "record_id", "document_node_id", "chunk_node_id"}``."""
        data: Dict[str, Any] = {"vector": vector}
        if collection != "default":
            data["collection"] = collection
        if attach_to_document_node is not None:
            data["attach_to_document_node"] = attach_to_document_node
        if metadata is not None:
            data["metadata"] = metadata
        if tags is not None:
            data["tags"] = tags
        return await self._post("/v1/memory/upsert_vector", data)

    async def memory_search(
        self,
        query_vector: Vector,
        k: int = 5,
        collection: str = "default",
        decay_half_life_secs: Optional[int] = None,
    ) -> List[Dict[str, Any]]:
        """Async version of SyncRemoteClient.memory_search. Returns a list of
        ``{"memory_id", "record_id", "score", "metadata", "decay_factor"?,
        "age_secs"?}``."""
        data: Dict[str, Any] = {"query_vector": query_vector, "k": k}
        if collection != "default":
            data["collection"] = collection
        if decay_half_life_secs is not None:
            data["decay_half_life_secs"] = decay_half_life_secs
        resp = await self._post("/v1/memory/search_vector", data)
        return resp["results"]

    # ── Proof / provenance ──────────────────────────────────────────────────────

    async def event_log_proof(self) -> Dict[str, Any]:
        """Async version of SyncRemoteClient.event_log_proof. Returns
        ``{"kernel_version", "event_log_hash", "final_state_hash",
        "snapshot_hash"?, "event_count", "committed_height"}``."""
        url = self.base_url + "/v1/proof/event-log"
        async with self.session.get(url) as resp:
            resp.raise_for_status()
            return await resp.json()

    async def get_version(self) -> str:
        """Return the node's software version."""
        url = self.base_url + "/version"
        async with self.session.get(url) as resp:
            resp.raise_for_status()
            return (await resp.text()).strip()

    async def list_nodes(self, collection: str = "default") -> Dict[str, Any]:
        """List graph nodes in a collection. Returns ``{"nodes": [...], "count"}``."""
        url = self.base_url + "/graph/nodes"
        params = {} if collection == "default" else {"collection": collection}
        async with self.session.get(url, params=params) as resp:
            resp.raise_for_status()
            return await resp.json()

    # ── Snapshot / object-store offload ─────────────────────────────────────────

    async def save_snapshot(self, path: Optional[str] = None) -> Dict[str, Any]:
        """Write a snapshot to the node's local filesystem. Returns
        ``{"success", "path"}``."""
        data: Dict[str, Any] = {}
        if path is not None:
            data["path"] = path
        return await self._post("/v1/snapshot/save", data)

    async def restore_snapshot(self, path: str) -> Dict[str, Any]:
        """Restore node state from a snapshot file on the node's local
        filesystem at ``path``. Returns ``{"success"}``."""
        return await self._post("/v1/snapshot/restore", {"path": path})

    async def list_remote_snapshots(self) -> Dict[str, Any]:
        """List snapshots in the object store. Returns ``{"snapshots", "count"}``."""
        url = self.base_url + "/v1/storage/snapshots"
        async with self.session.get(url) as resp:
            resp.raise_for_status()
            return await resp.json()

    async def upload_snapshot_to_store(self) -> Dict[str, Any]:
        """Snapshot + upload to the object store. Returns the upload result."""
        return await self._post("/v1/storage/snapshots/upload", {})

    async def restore_from_store(self, key: str) -> Dict[str, Any]:
        """Restore state from an object-store snapshot ``key``. Returns
        ``{"key", "state_hash", "size_bytes"}``."""
        return await self._post("/v1/storage/snapshots/restore", {"key": key})

    async def list_remote_wal(self) -> Dict[str, Any]:
        """List archived WAL segments in the object store. Returns
        ``{"segments", "count"}``."""
        url = self.base_url + "/v1/storage/wal"
        async with self.session.get(url) as resp:
            resp.raise_for_status()
            return await resp.json()

    async def archive_wal_segment(self, path: str) -> Dict[str, Any]:
        """Archive a sealed WAL segment (local ``path``) to the object store.
        Returns ``{"key", "size_bytes"}``."""
        return await self._post("/v1/storage/wal/archive", {"path": path})

    async def timeline(
        self,
        from_ts: Optional[str] = None,
        to_ts: Optional[str] = None,
        collection: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Async version of SyncRemoteClient.timeline."""
        params: Dict[str, str] = {}
        if from_ts:
            params["from"] = from_ts
        if to_ts:
            params["to"] = to_ts
        if collection:
            params["collection"] = collection
        try:
            resp = await self.client.get(f"{self.base_url}/v1/timeline", params=params)
            resp.raise_for_status()
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"Failed to fetch timeline: {e}")

    async def create_node(self, kind: int, record_id: Optional[int] = None) -> NodeId:
        data = {"kind": kind, "record_id": record_id}
        resp = await self._post("/graph/node", data)
        return resp["node_id"]

    async def create_edge(self, from_id: int, to_id: int, kind: int) -> int:
        data = {"from": from_id, "to": to_id, "kind": kind}
        resp = await self._post("/graph/edge", data)
        return resp["edge_id"]

    async def create_collection(self, name: str) -> Dict[str, Any]:
        """Create a new collection (namespace). Idempotent."""
        return await self._post("/v1/namespaces", {"name": name})

    async def list_collections(self) -> List[Dict[str, Any]]:
        """List all collections."""
        url = self.base_url + "/v1/namespaces"
        try:
            resp = await self.client.get(url)
            resp.raise_for_status()
            return resp.json().get("collections", [])
        except Exception as e:
            raise ConnectionError(f"Failed to list collections: {e}")

    async def drop_collection(self, name: str) -> None:
        """Drop a collection and all its records/nodes."""
        url = self.base_url + f"/v1/namespaces/{name}"
        try:
            resp = await self.client.delete(url)
            if resp.status_code == 400:
                raise ValueError(resp.json().get("error", resp.text))
            resp.raise_for_status()
        except Exception as e:
            raise ConnectionError(f"Failed to drop collection '{name}': {e}")

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

    # ── Phase 3.6: Crypto-shredding ──────────────────────────────────────────

    async def insert_encrypted(
        self,
        payload: bytes,
        tag: int = 0,
        collection: str = "default",
        key_id: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Async version of :meth:`SyncRemoteClient.insert_encrypted`."""
        import base64
        body: Dict[str, Any] = {
            "payload": base64.b64encode(payload).decode(),
            "tag": tag,
            "collection": collection,
        }
        if key_id is not None:
            body["key_id"] = key_id
        return await self._post("/v1/records/encrypted", body)

    async def shred_key(self, key_id: str) -> Dict[str, Any]:
        """Async version of :meth:`SyncRemoteClient.shred_key`."""
        url = self.base_url + f"/v1/crypto/shred/{key_id}"
        async with self.session.delete(url) as resp:
            resp.raise_for_status()
            return await resp.json()

    async def shred_key_status(self, key_id: str) -> Dict[str, Any]:
        """Async version of :meth:`SyncRemoteClient.shred_key_status`."""
        url = self.base_url + f"/v1/crypto/status/{key_id}"
        async with self.session.get(url) as resp:
            resp.raise_for_status()
            return await resp.json()

    async def get_index_config(self) -> Dict[str, Any]:
        """Async version of :meth:`SyncRemoteClient.get_index_config`."""
        url = self.base_url + "/v1/index/config"
        async with self.session.get(url) as resp:
            resp.raise_for_status()
            return await resp.json()

    # ─────────────────────────────────────────────────────────────────────────

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

    # ── API key management (Phase 3.5) ────────────────────────────────────────

    async def create_key(
        self,
        scope: str = "read_write",
        collection: Optional[str] = None,
        description: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Create a new API key.  Requires admin credentials."""
        data: Dict[str, Any] = {"scope": scope}
        if collection is not None:
            data["collection"] = collection
        if description is not None:
            data["description"] = description
        return await self._post("/v1/keys", data)

    async def list_keys(self) -> List[Dict[str, Any]]:
        """List all API keys (masked).  Requires admin credentials."""
        try:
            resp = await self.client.get(f"{self.base_url}/v1/keys")
            resp.raise_for_status()
            return resp.json().get("keys", [])
        except Exception as e:
            raise ConnectionError(f"Failed to list keys: {e}")

    async def revoke_key(self, key_id: str) -> None:
        """Revoke an API key by ID.  Requires admin credentials."""
        try:
            resp = await self.client.delete(f"{self.base_url}/v1/keys/{key_id}")
            if resp.status_code == 404:
                raise NotFoundError(f"Key not found: {key_id}")
            resp.raise_for_status()
        except Exception as e:
            raise ConnectionError(f"Failed to revoke key '{key_id}': {e}")

    async def cluster_health(self) -> bool:
        """True when the node sees an elected leader (HTTP 200 on /v1/cluster/health)."""
        import httpx
        url = self.base_url + "/v1/cluster/health"
        try:
            resp = await self.client.get(url)
            return resp.status_code == 200
        except httpx.HTTPError:
            return False

    async def leader_url(self) -> Optional[str]:
        """Cached leader base URL learned from a 307 redirect, or None."""
        return self._leader_url

    async def get_cluster_role(self) -> str:
        """Return this node's current Raft role: ``"leader"`` or ``"follower"``."""
        url = self.base_url + "/v1/cluster/role"
        try:
            resp = await self.client.get(url)
            if resp.status_code == 404:
                raise ConnectionError("node is not running in cluster mode (/v1/cluster/role not found)")
            resp.raise_for_status()
            return resp.json().get("role", "unknown")
        except Exception as e:
            raise ConnectionError(f"Failed to fetch cluster role from {url}: {e}")

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

    # ── Cortex: Knowledge graph ────────────────────────────────────────────────

    async def subgraph(self, root_node: int, depth: int = 2) -> Dict[str, Any]:
        """Bounded BFS from ``root_node``. Returns ``{"nodes": [...], "edges": [...]}``.

        NodeKind: 0=Record, 1=Concept, 5=Document, 6=Chunk
        EdgeKind: 4=Mentions, 5=RefersTo, 6=ParentOf
        """
        url = self.base_url + f"/graph/subgraph?root={root_node}&depth={depth}"
        try:
            resp = await self.client.get(url)
            resp.raise_for_status()
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"subgraph failed: {e}")

    # ── Cortex: Contradiction queue ────────────────────────────────────────────

    async def list_contradictions(
        self,
        collection: str = "default",
        status: str = "pending",
    ) -> Dict[str, Any]:
        """**Deprecated (Phase C4.3).** Legacy C3 UI-layer read (``ui_url``), not
        the node. Use :meth:`contradict` (node-native, audited) instead."""
        warnings.warn(
            "list_contradictions() is deprecated; it queries the legacy UI layer, "
            "not the node. Use contradict() instead.",
            DeprecationWarning,
            stacklevel=2,
        )
        url = self.ui_url + f"/api/contradictions?collection={collection}&status={status}"
        try:
            resp = await self.client.get(url)
            resp.raise_for_status()
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"list_contradictions failed: {e}")

    async def resolve_contradiction(
        self,
        contradiction_id: str,
        action: str,
    ) -> Dict[str, Any]:
        """**Deprecated (Phase C4.3).** Legacy C3 UI-layer write (``ui_url``), not
        the node. Use :meth:`consolidate` or :meth:`contradict` instead."""
        warnings.warn(
            "resolve_contradiction() is deprecated; it writes to the legacy UI layer, "
            "not the node. Use consolidate() or contradict() instead.",
            DeprecationWarning,
            stacklevel=2,
        )
        url = self.ui_url + "/api/contradictions"
        try:
            resp = await self.client.post(url, json={"id": contradiction_id, "action": action})
            resp.raise_for_status()
            return resp.json()
        except Exception as e:
            raise ConnectionError(f"resolve_contradiction failed: {e}")

    async def close(self):
        """Close the underlying httpx client."""
        await self.client.aclose()

class ClusterClient:
    """Multi-node cluster client — routes writes to the leader, round-robins reads.

    Point it at all the nodes in your cluster; the client discovers the
    leader automatically via the first 307 redirect and caches it.  Local
    reads are spread across all nodes; linearizable reads go to the leader.

    Usage::

        from valoricore.remote import ClusterClient

        c = ClusterClient([
            "http://node1:3000",
            "http://node2:3000",
            "http://node3:3000",
        ])

        rid = c.insert([0.1, 0.2, 0.3, 0.4])
        hits = c.search([0.1, 0.2, 0.3, 0.4], k=5, consistency="local")
        print(c.leader_url())   # → 'http://node2:3000' (whichever is the leader)
    """

    def __init__(
        self,
        nodes: List[str],
        max_retries: int = 3,
        retry_backoff: float = 0.5,
        ui_url: Optional[str] = None,
    ):
        if not nodes:
            raise ValueError("ClusterClient requires at least one node URL")
        self._clients = [
            SyncRemoteClient(url, max_retries=max_retries, retry_backoff=retry_backoff, ui_url=ui_url)
            for url in nodes
        ]
        self._rr_idx = 0

    def leader_url(self) -> Optional[str]:
        """Last known leader base URL, or ``None`` until the first write discovers it."""
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

    # ── Writes ────────────────────────────────────────────────────────────────

    def insert(
        self,
        vector: Vector,
        tag: int = 0,
        collection: str = "default",
        idempotency_key: Optional[bytes] = None,
    ) -> RecordId:
        return self._write_client().insert(
            vector, tag=tag, collection=collection, idempotency_key=idempotency_key
        )

    def insert_batch(
        self,
        batch: List[Vector],
        collection: str = "default",
        metadata: Optional[List[Optional[str]]] = None,
        request_ids: Optional[List[Optional[str]]] = None,
    ) -> List[RecordId]:
        return self._write_client().insert_batch(
            batch, collection=collection, metadata=metadata, request_ids=request_ids
        )

    def delete(self, record_id: int, idempotency_key: Optional[bytes] = None) -> None:
        self._write_client().delete(record_id, idempotency_key=idempotency_key)

    def soft_delete(self, record_id: int, idempotency_key: Optional[bytes] = None) -> None:
        self._write_client().soft_delete(record_id, idempotency_key=idempotency_key)

    def create_collection(self, name: str) -> Dict[str, Any]:
        return self._write_client().create_collection(name)

    def drop_collection(self, name: str) -> None:
        self._write_client().drop_collection(name)

    def restore(self, data: bytes) -> None:
        self._write_client().restore(data)

    # ── Reads ─────────────────────────────────────────────────────────────────

    def search(
        self,
        query: Vector,
        k: int,
        filter_tag: Optional[int] = None,
        consistency: str = "local",
        collection: str = "default",
        **kwargs: Any,
    ) -> Any:
        return self._read_client(consistency).search(
            query, k, filter_tag=filter_tag,
            consistency=consistency, collection=collection, **kwargs,
        )

    def graphrag(
        self,
        query_vector: Vector,
        k: int = 5,
        depth: int = 2,
        collection: str = "default",
        consistency: str = "local",
    ) -> Dict[str, Any]:
        """GraphRAG routed to a read replica (see SyncRemoteClient.graphrag).
        ``consistency`` defaults to "local"; pass "linearizable" for a
        read-index round trip that reflects every committed write."""
        return self._read_client(consistency).graphrag(
            query_vector, k=k, depth=depth, collection=collection, consistency=consistency,
        )

    def consolidate(
        self,
        old_record_id: int,
        new_vector: Vector,
        collection: str = "default",
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        """Consolidate a memory (Phase C4.2) — routed to the leader. See
        SyncRemoteClient.consolidate. Cluster IDs are assigned by the Raft state
        machine; the response carries the allocated record/edge IDs."""
        return self._write_client().consolidate(
            old_record_id, new_vector, collection=collection, metadata=metadata,
        )

    def contradict(
        self,
        record_a: int,
        record_b: int,
        threshold: Optional[float] = None,
        collection: str = "default",
    ) -> Dict[str, Any]:
        """Detect contradiction (Phase C4.3) — routed to the leader. See
        SyncRemoteClient.contradict."""
        return self._write_client().contradict(
            record_a, record_b, threshold=threshold, collection=collection,
        )

    def list_collections(self) -> List[Dict[str, Any]]:
        return self._read_client().list_collections()

    def get_state_hash(self) -> str:
        return self._read_client().get_state_hash()

    def event_log_proof(self) -> Dict[str, Any]:
        """Event-log proof from a replica (see SyncRemoteClient.event_log_proof)."""
        return self._read_client().event_log_proof()

    def timeline(
        self,
        from_ts: Optional[str] = None,
        to_ts: Optional[str] = None,
        collection: Optional[str] = None,
    ) -> Dict[str, Any]:
        return self._read_client().timeline(from_ts=from_ts, to_ts=to_ts, collection=collection)

    def snapshot(self) -> bytes:
        return self._read_client().snapshot()

    # ── Cluster meta ──────────────────────────────────────────────────────────

    def cluster_status(self) -> Dict[str, Any]:
        return self._read_client().cluster_status()

    def cluster_health(self) -> bool:
        return any(c.cluster_health() for c in self._clients)

    def get_cluster_role(self) -> str:
        return self._write_client().get_cluster_role()

    def create_key(self, scope: str = "read_write", collection: Optional[str] = None, description: Optional[str] = None) -> Dict[str, Any]:
        return self._write_client().create_key(scope=scope, collection=collection, description=description)

    def list_keys(self) -> List[Dict[str, Any]]:
        return self._write_client().list_keys()

    def revoke_key(self, key_id: str) -> None:
        self._write_client().revoke_key(key_id)


class AsyncClusterClient:
    """Async multi-node cluster client. Mirrors :class:`ClusterClient`."""

    def __init__(
        self,
        nodes: List[str],
        max_retries: int = 3,
        retry_backoff: float = 0.5,
        ui_url: Optional[str] = None,
    ):
        if not nodes:
            raise ValueError("AsyncClusterClient requires at least one node URL")
        self._clients = [
            AsyncRemoteClient(url, max_retries=max_retries, retry_backoff=retry_backoff, ui_url=ui_url)
            for url in nodes
        ]
        self._rr_idx = 0

    def leader_url(self) -> Optional[str]:
        for c in self._clients:
            if c._leader_url is not None:
                return c._leader_url
        return None

    def _write_client(self) -> "AsyncRemoteClient":
        for c in self._clients:
            if c._leader_url is not None:
                return c
        return self._clients[0]

    def _read_client(self, consistency: str = "local") -> "AsyncRemoteClient":
        if consistency == "linearizable":
            return self._write_client()
        c = self._clients[self._rr_idx % len(self._clients)]
        self._rr_idx += 1
        return c

    async def insert(
        self,
        vector: Vector,
        tag: int = 0,
        collection: str = "default",
    ) -> RecordId:
        return await self._write_client().insert(vector, tag=tag, collection=collection)

    async def insert_batch(
        self,
        batch: List[Vector],
        collection: str = "default",
        metadata: Optional[List[Optional[str]]] = None,
        request_ids: Optional[List[Optional[str]]] = None,
    ) -> List[RecordId]:
        return await self._write_client().insert_batch(
            batch, collection=collection, metadata=metadata, request_ids=request_ids
        )

    async def delete(self, record_id: int) -> None:
        await self._write_client().delete(record_id)

    async def create_collection(self, name: str) -> Dict[str, Any]:
        return await self._write_client().create_collection(name)

    async def drop_collection(self, name: str) -> None:
        await self._write_client().drop_collection(name)

    async def search(
        self,
        query: Vector,
        k: int,
        filter_tag: Optional[int] = None,
        consistency: str = "local",
        collection: str = "default",
        **kwargs: Any,
    ) -> Any:
        return await self._read_client(consistency).search(
            query, k, filter_tag=filter_tag,
            consistency=consistency, collection=collection, **kwargs,
        )

    async def graphrag(
        self,
        query_vector: Vector,
        k: int = 5,
        depth: int = 2,
        collection: str = "default",
        consistency: str = "local",
    ) -> Dict[str, Any]:
        """Async version of ClusterClient.graphrag — routed to a read replica."""
        return await self._read_client(consistency).graphrag(
            query_vector, k=k, depth=depth, collection=collection, consistency=consistency,
        )

    async def consolidate(
        self,
        old_record_id: int,
        new_vector: Vector,
        collection: str = "default",
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        """Async version of ClusterClient.consolidate (Phase C4.2) — routed to the leader."""
        return await self._write_client().consolidate(
            old_record_id, new_vector, collection=collection, metadata=metadata,
        )

    async def contradict(
        self,
        record_a: int,
        record_b: int,
        threshold: Optional[float] = None,
        collection: str = "default",
    ) -> Dict[str, Any]:
        """Async version of ClusterClient.contradict (Phase C4.3) — routed to the leader."""
        return await self._write_client().contradict(
            record_a, record_b, threshold=threshold, collection=collection,
        )

    async def list_collections(self) -> List[Dict[str, Any]]:
        return await self._read_client().list_collections()

    async def get_state_hash(self) -> str:
        return await self._read_client().get_state_hash()

    async def event_log_proof(self) -> Dict[str, Any]:
        """Event-log proof from a replica (see SyncRemoteClient.event_log_proof)."""
        return await self._read_client().event_log_proof()

    async def timeline(
        self,
        from_ts: Optional[str] = None,
        to_ts: Optional[str] = None,
        collection: Optional[str] = None,
    ) -> Dict[str, Any]:
        return await self._read_client().timeline(from_ts=from_ts, to_ts=to_ts, collection=collection)

    async def cluster_status(self) -> Dict[str, Any]:
        return await self._read_client().cluster_status()

    async def cluster_health(self) -> bool:
        import asyncio
        results = await asyncio.gather(
            *[c.cluster_health() for c in self._clients], return_exceptions=True
        )
        return any(r is True for r in results)

    async def get_cluster_role(self) -> str:
        return await self._write_client().get_cluster_role()

    async def close(self) -> None:
        import asyncio
        await asyncio.gather(*[c.close() for c in self._clients])


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
