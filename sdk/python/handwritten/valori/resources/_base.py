# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Common base for every resource wrapper."""

from __future__ import annotations

from typing import TYPE_CHECKING, Optional

if TYPE_CHECKING:  # pragma: no cover
    from ..transport import Transport

__all__ = ["Resource", "CollectionScoped"]


class Resource:
    """A namespace of ergonomic methods bound to one :class:`Transport`."""

    def __init__(self, transport: "Transport") -> None:
        self._t = transport

    def __repr__(self) -> str:  # pragma: no cover - formatting only
        return f"{type(self).__name__}(base_url={self._t.base_url!r})"


class CollectionScoped(Resource):
    """A resource whose every call carries a collection name."""

    def __init__(self, transport: "Transport", collection: Optional[str]) -> None:
        super().__init__(transport)
        self._collection = collection

    @property
    def collection(self) -> Optional[str]:
        return self._collection

    def __repr__(self) -> str:  # pragma: no cover - formatting only
        return f"{type(self).__name__}(collection={self._collection!r})"
