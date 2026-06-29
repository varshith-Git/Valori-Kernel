# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
Abstract base class for all Valoricore client implementations.

Both LocalClient (FFI) and SyncRemoteClient (HTTP) implement this interface,
making them swappable through the Valoricore() factory without call-site changes.

Contract notes:
- `collection` — LocalClient is single-tenant; the parameter is accepted and
  silently ignored. Remote honours it as a namespace.
- `text` / `texts` — LocalClient has no built-in text index; accepted and ignored.
  Pass text at insert time only when you know the client is remote and the node
  has VALORI_EMBED_PROVIDER configured.
- `idempotency_key` — LocalClient performs no dedup; the key is ignored.
- `consistency` / `as_of` — cluster-only; LocalClient accepts and ignores them.
- Extra `**kwargs` on any method — silently forwarded to remote; ignored locally.
  This keeps code portable as new optional parameters are added.
"""

from abc import ABC, abstractmethod
from typing import Any, Dict, List, Optional

from .types import Proof, RecordId, StateHash, Vector


class ValoriClient(ABC):
    """Shared interface for LocalClient and SyncRemoteClient."""

    # ── Write ─────────────────────────────────────────────────────────────────

    @abstractmethod
    def insert(
        self,
        vector: Vector,
        tag: int = 0,
        *,
        collection: Optional[str] = None,
        text: Optional[str] = None,
        idempotency_key: Optional[str] = None,
        **kwargs: Any,
    ) -> RecordId:
        """Insert a single vector. Returns the assigned record ID."""

    @abstractmethod
    def insert_batch(
        self,
        vectors: List[Vector],
        *,
        collection: Optional[str] = None,
        metadata: Optional[List[Optional[Dict[str, Any]]]] = None,
        texts: Optional[List[str]] = None,
        tags: Optional[List[int]] = None,
        **kwargs: Any,
    ) -> List[RecordId]:
        """Insert multiple vectors. Returns list of assigned record IDs.

        ``metadata``: one dict per vector (or None to skip). The SDK serialises
        each dict to a JSON string before sending; callers always work with dicts.
        """

    @abstractmethod
    def delete(
        self,
        record_id: RecordId,
        *,
        idempotency_key: Optional[str] = None,
        **kwargs: Any,
    ) -> None:
        """Permanently delete a record by ID."""

    # ── Read ──────────────────────────────────────────────────────────────────

    @abstractmethod
    def search(
        self,
        query: Vector,
        k: int,
        filter_tag: Optional[int] = None,
        *,
        collection: Optional[str] = None,
        consistency: Optional[str] = None,
        decay_half_life_secs: Optional[float] = None,
        rerank: bool = False,
        query_text: Optional[str] = None,
        metadata_filter: Optional[Dict[str, Any]] = None,
        **kwargs: Any,
    ) -> List[Dict[str, Any]]:
        """
        Nearest-neighbour search. Returns list of dicts with at minimum
        ``{"id": int, "score": int}``.
        """

    @abstractmethod
    def record_count(self) -> int:
        """Return the number of live (non-deleted) records."""

    # ── Metadata ──────────────────────────────────────────────────────────────

    @abstractmethod
    def get_metadata(self, record_id: RecordId) -> Optional[Dict[str, Any]]:
        """Return metadata dict for a record, or None if not set."""

    @abstractmethod
    def set_metadata(self, record_id: RecordId, metadata: Dict[str, Any]) -> None:
        """Attach a metadata dict to a record."""

    # ── Proof / audit ─────────────────────────────────────────────────────────

    @abstractmethod
    def get_state_hash(self) -> StateHash:
        """Return the hex-encoded BLAKE3 root hash of the current state."""
