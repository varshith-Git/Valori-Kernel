# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Index lifecycle — per-collection builds and the node-wide index config.

Covers ``set_collection_index``, ``get_collection_index``, ``get_index_config``
and ``rebuild_indexes``.
"""

from __future__ import annotations

import time
from typing import Any, Mapping, Optional

from valori_generated.api.index import (
    get_collection_index as _get_collection_index,
    get_index_config as _get_index_config,
    rebuild_indexes as _rebuild_indexes,
    set_collection_index as _set_collection_index,
)
from valori_generated.models.index_build_request import IndexBuildRequest
from valori_generated.models.index_rebuild_request import IndexRebuildRequest

from .._models import build
from ..errors import IndexBuildFailedError, OperationTimeoutError, ValoriConfigError
from ._base import CollectionScoped, Resource

__all__ = ["CollectionIndex", "IndexConfig", "TERMINAL_INDEX_STATES"]

#: Index lifecycle states that mean "the build is over". Sourced from
#: ``IndexStatusResponse::from_state`` in ``valori-engine`` — the node emits
#: exactly ``building`` / ``active`` / ``failed`` / ``none``.
TERMINAL_INDEX_STATES = frozenset({"active", "failed", "none"})


class CollectionIndex(CollectionScoped):
    """``collection.index`` — build an ANN index and watch it land."""

    def build(
        self,
        index_type: Optional[str] = None,
        *,
        parameters: Optional[Mapping[str, Any]] = None,
    ) -> Any:
        """Request an index build. ``POST /v1/namespaces/{name}/index``.

        Returns immediately with the lifecycle state (the node answers 202 while
        the build runs). Use :meth:`wait` to block until it settles.
        """
        if not self._collection:
            raise ValoriConfigError("index.build requires a collection")
        body = build(IndexBuildRequest, type=index_type, parameters=parameters)
        return self._t.call(_set_collection_index, name=self._collection, body=body)

    def status(self) -> Any:
        """Read the current lifecycle state. ``GET /v1/namespaces/{name}/index``."""
        if not self._collection:
            raise ValoriConfigError("index.status requires a collection")
        return self._t.call(_get_collection_index, name=self._collection)

    def wait(
        self,
        *,
        poll_interval: float = 1.0,
        timeout: float = 300.0,
        raise_on_failure: bool = True,
        _now=time.monotonic,
        _sleep=time.sleep,
    ) -> Any:
        """Poll :meth:`status` until the build reaches a terminal state.

        §9: interval, timeout, terminal-state handling and failure conversion
        are all owned here, in the handwritten layer. The generated client keeps
        being a single-shot transport.
        """
        deadline = _now() + timeout
        last: Any = None
        while True:
            last = self.status()
            state = getattr(last, "status", None)
            if state in TERMINAL_INDEX_STATES:
                if state == "failed" and raise_on_failure:
                    raise IndexBuildFailedError(
                        f"index build for collection {self._collection!r} failed",
                        status_code=200,
                        code="index_build_failed",
                        body=last.to_dict() if hasattr(last, "to_dict") else None,
                    )
                return last
            if _now() >= deadline:
                raise OperationTimeoutError(
                    f"index build for collection {self._collection!r} did not settle "
                    f"within {timeout}s",
                    operation_id=str(self._collection),
                    last_status=state,
                )
            _sleep(poll_interval)


class IndexConfig(Resource):
    """``client.index`` — node-wide index configuration and rebuilds."""

    def config(self) -> Any:
        """Read the node's index configuration. ``GET /v1/index/config``."""
        return self._t.call(_get_index_config)

    def rebuild(self, index: Optional[str] = None) -> Any:
        """Rebuild indexes across the node. ``POST /v1/index/rebuild``."""
        body = build(IndexRebuildRequest, index=index)
        return self._t.call(_rebuild_indexes, body=body)
