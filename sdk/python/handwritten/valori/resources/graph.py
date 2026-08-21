# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Knowledge-graph operations, scoped to a collection.

Covers ``create_graph_node``, ``create_graph_edge``, ``get_graph_node``,
``delete_graph_node``, ``list_graph_nodes``, ``list_node_edges``,
``get_subgraph`` and ``graph_query``. ``graphrag`` is exposed one level up as
``collection.graphrag`` because it is a retrieval call, not a graph mutation.
"""

from __future__ import annotations

from typing import Any, Optional

from valori_generated.api.graph import (
    create_graph_edge as _create_graph_edge,
    create_graph_node as _create_graph_node,
    delete_graph_node as _delete_graph_node,
    get_graph_node as _get_graph_node,
    get_subgraph as _get_subgraph,
    graph_query as _graph_query,
    list_graph_nodes as _list_graph_nodes,
    list_node_edges as _list_node_edges,
)
from valori_generated.models.create_edge_request import CreateEdgeRequest
from valori_generated.models.create_node_request import CreateNodeRequest

from .._models import build
from ..transport import unset_if_none
from ._base import CollectionScoped

__all__ = ["Graph"]


class Graph(CollectionScoped):
    """``collection.graph`` — nodes, edges and traversals."""

    def create_node(self, kind: int, *, record_id: Optional[int] = None) -> Any:
        """Create a graph node. ``POST /v1/graph/node``."""
        body = build(
            CreateNodeRequest, kind=kind, record_id=record_id, collection=self._collection
        )
        return self._t.call(_create_graph_node, body=body)

    def create_edge(self, from_node: int, to_node: int, kind: int) -> Any:
        """Create a directed edge. ``POST /v1/graph/edge``.

        The wire field is ``from``, a Python keyword, so the argument is named
        ``from_node`` here and translated on the way out.
        """
        body = build(
            CreateEdgeRequest,
            **{"from": from_node, "to": to_node, "kind": kind, "collection": self._collection},
        )
        return self._t.call(_create_graph_edge, body=body)

    def get_node(self, node_id: int) -> Any:
        """Read one node. ``GET /v1/graph/node/{id}``."""
        return self._t.call(
            _get_graph_node, id=node_id, collection=unset_if_none(self._collection)
        )

    def delete_node(self, node_id: int) -> Any:
        """Delete a node and its incident edges. ``DELETE /v1/graph/node/{id}``."""
        return self._t.call(
            _delete_graph_node, id=node_id, collection=unset_if_none(self._collection)
        )

    def list_nodes(
        self,
        *,
        kind: Optional[int] = None,
        offset: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> Any:
        """Page through nodes. ``GET /v1/graph/nodes``."""
        return self._t.call(
            _list_graph_nodes,
            collection=unset_if_none(self._collection),
            kind=unset_if_none(kind),
            offset=unset_if_none(offset),
            limit=unset_if_none(limit),
        )

    def list_edges(self, node_id: int) -> Any:
        """List a node's edges. ``GET /v1/graph/edges/{id}``."""
        return self._t.call(
            _list_node_edges, id=node_id, collection=unset_if_none(self._collection)
        )

    def subgraph(self, root: int, *, depth: Optional[int] = None) -> Any:
        """Expand a subgraph around ``root``. ``GET /v1/graph/subgraph``."""
        return self._t.call(
            _get_subgraph,
            root=root,
            depth=unset_if_none(depth),
            collection=unset_if_none(self._collection),
        )

    def query(
        self,
        start: int,
        *,
        direction: Optional[str] = None,
        edge_kind: Optional[int] = None,
        node_kind: Optional[int] = None,
        depth: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> Any:
        """Filtered traversal from ``start``. ``GET /v1/graph/query``."""
        return self._t.call(
            _graph_query,
            start=start,
            direction=unset_if_none(direction),
            edge_kind=unset_if_none(edge_kind),
            node_kind=unset_if_none(node_kind),
            depth=unset_if_none(depth),
            limit=unset_if_none(limit),
            collection=unset_if_none(self._collection),
        )
