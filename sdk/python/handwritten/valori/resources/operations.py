# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Operations, and the polling ergonomics on top of them.

Covers ``list_operations``, ``get_operation`` and ``get_operation_execution``.

Phase API-4A §9: the generated client stays a single-shot transport. Poll
interval, deadline, terminal-state recognition and failure conversion are all
decided here.
"""

from __future__ import annotations

import time
from typing import Any, Optional

from valori_generated.api.operations import (
    get_operation as _get_operation,
    get_operation_execution as _get_operation_execution,
    list_operations as _list_operations,
)

from ..errors import OperationFailedError, OperationTimeoutError
from ._base import Resource

__all__ = ["Operations", "Operation", "TERMINAL_STATES", "FAILED_STATES"]

#: Statuses the node uses to mean "this operation is over". Taken from the
#: strings the handlers actually emit (``valori-node/src/ingest.rs``,
#: ``server.rs``, ``cluster_server.rs``): ``processing`` while running,
#: ``completed`` or ``failed`` at the end.
TERMINAL_STATES = frozenset({"completed", "complete", "succeeded", "failed", "error", "cancelled"})
FAILED_STATES = frozenset({"failed", "error", "cancelled"})


class Operation:
    """A handle to one operation, with :meth:`wait`.

    ``client.operations.get(id)`` returns one of these. ``.data`` is the raw
    generated ``OperationDetailResponse`` from the most recent poll.
    """

    def __init__(self, operations: "Operations", operation_id: str, data: Any = None) -> None:
        self._ops = operations
        self._id = operation_id
        self.data = data

    @property
    def id(self) -> str:
        return self._id

    @property
    def status(self) -> Optional[str]:
        return getattr(self.data, "status", None)

    @property
    def done(self) -> bool:
        status = self.status
        return status is not None and status in TERMINAL_STATES

    @property
    def failed(self) -> bool:
        status = self.status
        return status is not None and status in FAILED_STATES

    def refresh(self) -> "Operation":
        """Re-read the operation. One request."""
        self.data = self._ops._fetch(self._id)
        return self

    def execution(self) -> Any:
        """The operation's execution plan/trace. ``GET /v1/operations/{id}/execution``."""
        return self._ops.execution(self._id)

    def wait(
        self,
        *,
        poll_interval: float = 1.0,
        timeout: float = 300.0,
        raise_on_failure: bool = True,
        _now=time.monotonic,
        _sleep=time.sleep,
    ) -> "Operation":
        """Poll until the operation reaches a terminal state.

        Raises :class:`~valori.errors.OperationTimeoutError` if the deadline
        passes first, and :class:`~valori.errors.OperationFailedError` if the
        operation ends in a failure state (unless ``raise_on_failure=False``).
        """
        deadline = _now() + timeout
        while True:
            if self.data is None:
                self.refresh()
            if self.done:
                if self.failed and raise_on_failure:
                    raise OperationFailedError(
                        f"operation {self._id} ended in status {self.status!r}",
                        operation_id=self._id,
                        status=self.status or "unknown",
                        detail=self.data.to_dict() if hasattr(self.data, "to_dict") else None,
                    )
                return self
            if _now() >= deadline:
                raise OperationTimeoutError(
                    f"operation {self._id} did not finish within {timeout}s",
                    operation_id=self._id,
                    last_status=self.status,
                )
            _sleep(poll_interval)
            self.refresh()

    def __repr__(self) -> str:  # pragma: no cover - formatting only
        return f"Operation(id={self._id!r}, status={self.status!r})"


class Operations(Resource):
    """``client.operations`` — list, read and wait on operations."""

    def list(self) -> Any:
        """``GET /v1/operations``."""
        return self._t.call(_list_operations)

    def _fetch(self, operation_id: str) -> Any:
        return self._t.call(_get_operation, id=operation_id)

    def get(self, operation_id: str) -> Operation:
        """``GET /v1/operations/{id}``, wrapped in an :class:`Operation` handle."""
        return Operation(self, operation_id, self._fetch(operation_id))

    def execution(self, operation_id: str) -> Any:
        """``GET /v1/operations/{id}/execution``."""
        return self._t.call(_get_operation_execution, id=operation_id)

    def wait(self, operation_id: str, **kwargs: Any) -> Operation:
        """Convenience: ``client.operations.wait(id)`` == ``get(id).wait()``."""
        return Operation(self, operation_id).wait(**kwargs)
