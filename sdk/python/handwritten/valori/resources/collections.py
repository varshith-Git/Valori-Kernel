# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Collections, and the ``Collection`` handle everything else hangs off.

Covers ``create_collection``, ``list_collections``, ``delete_collection``,
``search``, ``search_multi`` and ``graphrag``. Per-collection records, index,
graph and memory operations live in their own modules and are reached through
:class:`Collection`.

Phase API-4A §10: a ``Collection`` is a *handle*, not a fetched document. The
contract has no ``GET /v1/namespaces/{name}``, so ``client.collections["docs"]``
is a zero-round-trip handle. :meth:`Collections.get` is the checked form — it
lists and raises :class:`~valori.errors.CollectionNotFoundError` if the name is
absent. No endpoint was invented to make the ergonomics nicer.
"""

from __future__ import annotations

from typing import Any, Mapping, Optional, Sequence

from valori_generated.api.collections import (
    create_collection as _create_collection,
    delete_collection as _delete_collection,
    list_collections as _list_collections,
)
from valori_generated.api.graph import graphrag as _graphrag
from valori_generated.api.search import search as _search, search_multi as _search_multi
from valori_generated.models.create_collection_request import CreateCollectionRequest
from valori_generated.models.graph_rag_request import GraphRagRequest
from valori_generated.models.multi_search_request import MultiSearchRequest
from valori_generated.models.search_request import SearchRequest

from .._models import build
from .._wire import encode_metadata_filter
from ..errors import CollectionAlreadyExistsError, CollectionNotFoundError, ConflictError
from ._base import Resource
from .graph import Graph
from .index import CollectionIndex
from .memory import Memory
from .records import Records

__all__ = ["Collections", "Collection"]


class Collection:
    """A handle to one collection. Everything collection-scoped lives here."""

    def __init__(self, transport, name: str) -> None:
        self._t = transport
        self._name = name
        self.records = Records(transport, name)
        self.index = CollectionIndex(transport, name)
        self.graph = Graph(transport, name)
        self.memory = Memory(transport, name)

    @property
    def name(self) -> str:
        return self._name

    def __repr__(self) -> str:  # pragma: no cover - formatting only
        return f"Collection(name={self._name!r})"

    # ── search ───────────────────────────────────────────────────────────────

    def search(
        self,
        query: Sequence[float],
        k: int,
        *,
        query_text: Optional[str] = None,
        rerank: Optional[bool] = None,
        decay_half_life_secs: Optional[int] = None,
        metadata_filter: Optional[Mapping[str, Any]] = None,
        graph_rerank: Optional[Mapping[str, Any]] = None,
        as_of: Optional[str] = None,
        as_of_log_index: Optional[int] = None,
    ) -> Any:
        """K-nearest-neighbour search. ``POST /v1/search``."""
        body = build(
            SearchRequest,
            query=list(query),
            k=k,
            collection=self._name,
            query_text=query_text,
            rerank=rerank,
            decay_half_life_secs=decay_half_life_secs,
            metadata_filter=encode_metadata_filter(metadata_filter),
            graph_rerank=graph_rerank,
            as_of=as_of,
            as_of_log_index=as_of_log_index,
        )
        return self._t.call(_search, body=body)

    def graphrag(
        self,
        query_vector: Sequence[float],
        *,
        k: Optional[int] = None,
        depth: Optional[int] = None,
        retrieval_k: Optional[int] = None,
        final_k: Optional[int] = None,
        graph_weight: Optional[float] = None,
        max_nodes: Optional[int] = None,
        max_edges: Optional[int] = None,
        max_graph_candidates: Optional[int] = None,
    ) -> Any:
        """Vector hits plus the connected subgraph. ``POST /v1/graphrag``."""
        body = build(
            GraphRagRequest,
            query_vector=list(query_vector),
            collection=self._name,
            k=k,
            depth=depth,
            retrieval_k=retrieval_k,
            final_k=final_k,
            graph_weight=graph_weight,
            max_nodes=max_nodes,
            max_edges=max_edges,
            max_graph_candidates=max_graph_candidates,
        )
        return self._t.call(_graphrag, body=body)


class Collections(Resource):
    """``client.collections`` — create, list, look up and drop collections."""

    def create(
        self,
        name: str,
        *,
        dimension: int,
        metric: str,
        index: Optional[str] = None,
    ) -> Collection:
        """Create a collection and return a handle to it. ``POST /v1/namespaces``.

        ``dimension`` and ``metric`` are required by the contract — a fresh
        project has no implicit "default" collection to inherit them from.
        """
        body = build(
            CreateCollectionRequest,
            name=name,
            dimension=dimension,
            metric=metric,
            index=index,
        )
        try:
            self._t.call(_create_collection, body=body)
        except ConflictError as exc:
            # See CollectionAlreadyExistsError's docstring: the node reports this
            # as a plain `conflict`, and this is the one call site where that
            # code can only mean one thing.
            raise CollectionAlreadyExistsError(
                exc.message,
                status_code=exc.status_code,
                code=exc.code,
                request_id=exc.request_id,
                body=exc.body,
                headers=exc.headers,
            ) from exc
        return Collection(self._t, name)

    def list(self) -> Any:
        """List collections. ``GET /v1/namespaces``."""
        return self._t.call(_list_collections)

    def names(self) -> list:
        """Just the collection names, as a list of strings."""
        listed = self.list()
        entries = getattr(listed, "collections", None)
        if entries is None and isinstance(listed, list):
            entries = listed
        out = []
        for entry in entries or []:
            name = getattr(entry, "name", None)
            if name is None and isinstance(entry, Mapping):
                name = entry.get("name")
            if name is None and isinstance(entry, str):
                name = entry
            if name is not None:
                out.append(name)
        return out

    def get(self, name: str) -> Collection:
        """Return a handle, verifying the collection exists.

        Costs one ``GET /v1/namespaces`` because the contract has no
        single-collection read. Use ``client.collections[name]`` to skip the
        check when you already know the collection is there.
        """
        if name not in self.names():
            raise CollectionNotFoundError(
                f"collection {name!r} does not exist on {self._t.base_url}",
                status_code=404,
                code="collection_not_found",
            )
        return Collection(self._t, name)

    def __getitem__(self, name: str) -> Collection:
        """Unchecked handle — no round trip."""
        return Collection(self._t, name)

    def __contains__(self, name: object) -> bool:
        return isinstance(name, str) and name in self.names()

    def delete(self, name: str) -> Any:
        """Drop a collection. ``DELETE /v1/namespaces/{name}``."""
        return self._t.call(_delete_collection, name=name)

    def search_multi(
        self,
        query: Sequence[float],
        k: int,
        collections: Sequence[str],
        *,
        decay_half_life_secs: Optional[int] = None,
        metadata_filter: Optional[Mapping[str, Any]] = None,
    ) -> Any:
        """Fan a query across several collections. ``POST /v1/search/multi``.

        Lives on ``client.collections`` rather than on a ``Collection`` because
        it is deliberately not scoped to one.
        """
        body = build(
            MultiSearchRequest,
            query=list(query),
            k=k,
            collections=list(collections),
            decay_half_life_secs=decay_half_life_secs,
            metadata_filter=encode_metadata_filter(metadata_filter),
        )
        return self._t.call(_search_multi, body=body)
