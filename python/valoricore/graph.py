# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
graph.py — High-level fluent graph API for Valori-Kernel
=========================================================

Wraps the low-level integer-ID graph operations behind Python objects so
developers never have to juggle raw IDs manually.

Quick-start::

    from valoricore import MemoryClient, Node
    from valoricore.kinds import NODE_DOCUMENT, NODE_CHUNK, EDGE_PARENT_OF

    db = MemoryClient(dim=384)

    # One call inserts the embedding AND creates the node
    doc   = db.node(NODE_DOCUMENT)
    chunk = db.node(NODE_CHUNK, vector=embedding)

    # Method-chaining for edges
    doc.link_to(chunk, EDGE_PARENT_OF)

    # Or do it all in a context manager:
    with db.build_document() as builder:
        builder.add_chunk(emb1)
        builder.add_chunk(emb2)
    doc_node = builder.document
"""

from __future__ import annotations

from typing import TYPE_CHECKING, List, Optional, Union

from .kinds import EDGE_PARENT_OF, NODE_CHUNK, NODE_DOCUMENT

if TYPE_CHECKING:
    # Avoid circular imports at runtime; only used for type hints.
    from .local import LocalClient


# ── Node ──────────────────────────────────────────────────────────────────────

class Node:
    """
    A Knowledge Graph node — an object you can hold, chain, and traverse.

    Instead of passing raw integer IDs through your code, create Node objects
    with :meth:`~valoricore.LocalClient.node` and work with them directly::

        doc   = db.node(NODE_DOCUMENT)
        chunk = db.node(NODE_CHUNK, vector=my_embedding)
        doc.link_to(chunk, EDGE_PARENT_OF)

    Attributes:
        id         Integer node ID (low-level handle, always accessible).
        kind       Node kind integer (matches the ``NODE_*`` constants).
        record_id  Linked vector record ID, or ``None`` if no vector is attached.
    """

    def __init__(
        self,
        node_id: int,
        kind: int,
        record_id: Optional[int],
        _db: "LocalClient",
    ) -> None:
        self.id        = node_id
        self.kind      = kind
        self.record_id = record_id
        self._db       = _db

    # ── Edge creation ─────────────────────────────────────────────────────────

    def link_to(
        self,
        other: Union["Node", List["Node"], int],
        edge_kind: int,
    ) -> "Node":
        """
        Create a directed edge from **this node** to *other*.

        Args:
            other:     A :class:`Node`, a plain integer node ID, or a *list*
                       of either — all are linked in one call.
            edge_kind: Integer edge kind (use the ``EDGE_*`` constants).

        Returns:
            ``self`` so you can chain calls::

                doc.link_to(c1, EDGE_PARENT_OF).link_to(c2, EDGE_PARENT_OF)
        """
        targets = other if isinstance(other, (list, tuple)) else [other]
        for t in targets:
            to_id = t.id if isinstance(t, Node) else int(t)
            self._db.create_edge(from_id=self.id, to_id=to_id, kind=edge_kind)
        return self

    def link_from(self, other: Union["Node", int], edge_kind: int) -> "Node":
        """
        Create a directed edge **into** this node from *other*.

        Returns ``self`` for chaining.
        """
        from_id = other.id if isinstance(other, Node) else int(other)
        self._db.create_edge(from_id=from_id, to_id=self.id, kind=edge_kind)
        return self

    # ── Traversal ─────────────────────────────────────────────────────────────

    def children(self, edge_kind: Optional[int] = None) -> List["Node"]:
        """
        Return all nodes reachable via outgoing edges from this node.

        Args:
            edge_kind: If given, only edges of that kind are followed.

        Returns:
            List of :class:`Node` objects in the order the edges were created.
        """
        raw_edges = self._db.get_edges(self.id)
        result: List[Node] = []
        for e in raw_edges:
            if edge_kind is not None and e["kind"] != edge_kind:
                continue
            data = self._db.get_node(e["to_node"])
            if data is not None:
                result.append(
                    Node(e["to_node"], data["kind"], data["record_id"], self._db)
                )
        return result

    def walk(self, max_depth: int = 2) -> List["Node"]:
        """
        BFS traversal starting from this node.

        Returns:
            Ordered list of visited :class:`Node` objects (this node first).
        """
        visited_ids = self._db.walk(self.id, max_depth)
        result: List[Node] = []
        for nid in visited_ids:
            data = self._db.get_node(nid)
            if data is not None:
                result.append(Node(nid, data["kind"], data["record_id"], self._db))
        return result

    def record_ids(self, max_depth: int = 2) -> List[int]:
        """
        Collect all vector record IDs reachable from this node via BFS.

        Equivalent to ``db.expand(node.id, max_depth)`` but returns plain ints.
        """
        return self._db.expand(self.id, max_depth)

    # ── Mutation ──────────────────────────────────────────────────────────────

    def delete(self) -> None:
        """Cascade-delete this node and all its incident edges."""
        self._db.delete_node(self.id)

    # ── Dunder helpers ────────────────────────────────────────────────────────

    def __repr__(self) -> str:
        return f"Node(id={self.id}, kind={self.kind}, record_id={self.record_id})"

    def __eq__(self, other: object) -> bool:
        if isinstance(other, Node):
            return self.id == other.id
        return NotImplemented

    def __hash__(self) -> int:
        return hash(self.id)

    def __int__(self) -> int:
        """Allow ``int(node)`` to retrieve the raw ID."""
        return self.id


# ── DocumentGraph ─────────────────────────────────────────────────────────────

class DocumentGraph:
    """
    Context-manager builder for the **document → chunk** graph pattern.

    The most common knowledge-graph structure in RAG systems is:

    .. code-block:: text

        DocumentNode
        ├── EDGE_PARENT_OF ──▶ ChunkNode(record=vec_0)
        ├── EDGE_PARENT_OF ──▶ ChunkNode(record=vec_1)
        └── EDGE_PARENT_OF ──▶ ChunkNode(record=vec_2)

    ``DocumentGraph`` builds exactly that in a clean ``with`` block::

        with db.build_document(title="My Essay") as builder:
            for embedding in embeddings:
                builder.add_chunk(embedding)

        # After the block:
        doc_node   = builder.document   # the root Node
        chunk_rids = builder.record_ids  # [0, 1, 2, …]

    Attributes:
        document   The root :class:`Node` of kind ``NODE_DOCUMENT``.
        chunks     Ordered list of :class:`Node` objects, one per chunk.
        title      Optional human-readable title (stored as metadata if set).
    """

    def __init__(self, _db: "LocalClient", title: Optional[str] = None) -> None:
        self._db:     "LocalClient"   = _db
        self.title:   Optional[str]   = title
        self.document: Optional[Node] = None
        self.chunks:   List[Node]     = []

    def __enter__(self) -> "DocumentGraph":
        self.document = Node(
            self._db.create_node(kind=NODE_DOCUMENT),
            kind=NODE_DOCUMENT,
            record_id=None,
            _db=self._db,
        )
        return self

    def add_chunk(
        self,
        vector: List[float],
        tag: int = 0,
        metadata: Optional[bytes] = None,
    ) -> Node:
        """
        Insert *vector*, create a ``NODE_CHUNK`` node, wire it to the document,
        and return the new :class:`Node`.

        Args:
            vector:   Embedding vector (must match the configured dimension).
            tag:      Optional integer tag for filtered search.
            metadata: Optional raw bytes to attach to the record (max 64 KB).

        Returns:
            The new chunk :class:`Node`.

        Raises:
            RuntimeError: If called outside the ``with`` block.
        """
        if self.document is None:
            raise RuntimeError(
                "add_chunk() must be called inside the 'with build_document()' block."
            )
        record_id = self._db.insert(vector, tag=tag)
        if metadata is not None:
            self._db.set_metadata(record_id, metadata)
        chunk_id  = self._db.create_node(kind=NODE_CHUNK, record_id=record_id)
        self._db.create_edge(from_id=self.document.id, to_id=chunk_id, kind=EDGE_PARENT_OF)
        chunk = Node(chunk_id, kind=NODE_CHUNK, record_id=record_id, _db=self._db)
        self.chunks.append(chunk)
        return chunk

    def __exit__(self, *_args: object) -> None:
        pass  # nothing to flush; every operation commits immediately

    # ── Convenience accessors ─────────────────────────────────────────────────

    @property
    def record_ids(self) -> List[int]:
        """All vector record IDs attached to chunk nodes, in insertion order."""
        return [c.record_id for c in self.chunks if c.record_id is not None]

    def __repr__(self) -> str:
        doc_id = self.document.id if self.document else "?"
        return (
            f"DocumentGraph(doc_node={doc_id}, "
            f"chunks={len(self.chunks)}, title={self.title!r})"
        )
