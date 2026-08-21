# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Agent-memory primitives, scoped to a collection.

Covers ``memory_upsert``, ``memory_upsert_vector``, ``memory_search``,
``memory_search_vector``, ``memory_consolidate``, ``memory_contradict``,
``get_metadata_sidecar`` and ``set_metadata_sidecar``.

``memory_upsert``/``memory_upsert_vector`` and
``memory_search``/``memory_search_vector`` are distinct operationIds on distinct
paths that share a request schema. Both are wrapped, and neither is quietly
aliased onto the other — an SDK that hides a path difference makes the audit
trail harder to read, not easier.
"""

from __future__ import annotations

from typing import Any, Mapping, Optional, Sequence

from valori_generated.api.memory import (
    get_metadata_sidecar as _get_metadata_sidecar,
    memory_consolidate as _memory_consolidate,
    memory_contradict as _memory_contradict,
    memory_search as _memory_search,
    memory_search_vector as _memory_search_vector,
    memory_upsert as _memory_upsert,
    memory_upsert_vector as _memory_upsert_vector,
    set_metadata_sidecar as _set_metadata_sidecar,
)
from valori_generated.models.memory_consolidate_request import MemoryConsolidateRequest
from valori_generated.models.memory_contradict_request import MemoryContradictRequest
from valori_generated.models.memory_search_vector_request import MemorySearchVectorRequest
from valori_generated.models.memory_upsert_vector_request import MemoryUpsertVectorRequest
from valori_generated.models.metadata_set_request import MetadataSetRequest

from .._models import build
from .._wire import encode_metadata_filter, encode_metadata_object
from ..transport import unset_if_none
from ._base import CollectionScoped

__all__ = ["Memory"]


class Memory(CollectionScoped):
    """``collection.memory`` — self-maintaining agent memory."""

    # ── writes ───────────────────────────────────────────────────────────────

    def upsert(
        self,
        vector: Sequence[float],
        *,
        metadata: Optional[Mapping[str, Any]] = None,
        tags: Optional[Sequence[str]] = None,
        attach_to_document_node: Optional[int] = None,
    ) -> Any:
        """``POST /v1/memory/upsert``."""
        return self._t.call(_memory_upsert, body=self._upsert_body(
            vector, metadata, tags, attach_to_document_node))

    def upsert_vector(
        self,
        vector: Sequence[float],
        *,
        metadata: Optional[Mapping[str, Any]] = None,
        tags: Optional[Sequence[str]] = None,
        attach_to_document_node: Optional[int] = None,
    ) -> Any:
        """``POST /v1/memory/upsert_vector``."""
        return self._t.call(_memory_upsert_vector, body=self._upsert_body(
            vector, metadata, tags, attach_to_document_node))

    def _upsert_body(self, vector, metadata, tags, attach_to_document_node):
        return build(
            MemoryUpsertVectorRequest,
            vector=list(vector),
            collection=self._collection,
            metadata=encode_metadata_object(metadata),
            tags=list(tags) if tags is not None else None,
            attach_to_document_node=attach_to_document_node,
        )

    # ── reads ────────────────────────────────────────────────────────────────

    def search(
        self,
        query_vector: Sequence[float],
        k: int,
        *,
        explain: Optional[bool] = None,
        query_text: Optional[str] = None,
        rerank: Optional[bool] = None,
        decay_half_life_secs: Optional[int] = None,
        metadata_filter: Optional[Mapping[str, Any]] = None,
        consistency: Optional[str] = None,
    ) -> Any:
        """``POST /v1/memory/search``."""
        return self._t.call(
            _memory_search,
            body=self._search_body(
                query_vector, k, query_text, rerank, decay_half_life_secs,
                metadata_filter, consistency),
            explain=unset_if_none(explain),
        )

    def search_vector(
        self,
        query_vector: Sequence[float],
        k: int,
        *,
        explain: Optional[bool] = None,
        query_text: Optional[str] = None,
        rerank: Optional[bool] = None,
        decay_half_life_secs: Optional[int] = None,
        metadata_filter: Optional[Mapping[str, Any]] = None,
        consistency: Optional[str] = None,
    ) -> Any:
        """``POST /v1/memory/search_vector``."""
        return self._t.call(
            _memory_search_vector,
            body=self._search_body(
                query_vector, k, query_text, rerank, decay_half_life_secs,
                metadata_filter, consistency),
            explain=unset_if_none(explain),
        )

    def _search_body(self, query_vector, k, query_text, rerank,
                     decay_half_life_secs, metadata_filter, consistency):
        return build(
            MemorySearchVectorRequest,
            query_vector=list(query_vector),
            k=k,
            collection=self._collection,
            query_text=query_text,
            rerank=rerank,
            decay_half_life_secs=decay_half_life_secs,
            metadata_filter=encode_metadata_filter(metadata_filter),
            consistency=consistency,
        )

    # ── maintenance ──────────────────────────────────────────────────────────

    def consolidate(
        self,
        old_record_id: int,
        new_vector: Sequence[float],
        *,
        metadata: Optional[Mapping[str, Any]] = None,
    ) -> Any:
        """Supersede a memory with a newer one. ``POST /v1/memory/consolidate``."""
        body = build(
            MemoryConsolidateRequest,
            old_record_id=old_record_id,
            new_vector=list(new_vector),
            collection=self._collection,
            metadata=encode_metadata_object(metadata),
        )
        return self._t.call(_memory_consolidate, body=body)

    def contradict(
        self, record_a: int, record_b: int, *, threshold: Optional[float] = None
    ) -> Any:
        """Record a contradiction between two memories. ``POST /v1/memory/contradict``."""
        body = build(
            MemoryContradictRequest,
            record_a=record_a,
            record_b=record_b,
            threshold=threshold,
            collection=self._collection,
        )
        return self._t.call(_memory_contradict, body=body)

    # ── metadata sidecar ─────────────────────────────────────────────────────

    def get_metadata(self, target_id: str) -> Any:
        """``GET /v1/memory/meta/get``."""
        return self._t.call(_get_metadata_sidecar, target_id=target_id)

    def set_metadata(self, target_id: str, metadata: Mapping[str, Any]) -> Any:
        """``POST /v1/memory/meta/set``."""
        body = build(
            MetadataSetRequest,
            target_id=target_id,
            metadata=encode_metadata_object(metadata),
        )
        return self._t.call(_set_metadata_sidecar, body=body)
