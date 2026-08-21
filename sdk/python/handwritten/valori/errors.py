# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Typed exceptions for the Valori REST API.

Phase API-4A §7. Every exception raised by the handwritten layer preserves the
full raw response: the HTTP status, the API ``code``, the API ``message``, the
request id when the server sent one, and the undecoded response body. Nothing
is thrown away in the process of giving the error a Python class.

The code table below is the closed ``ErrorCode`` enum from
``api/openapi/valori-v1.yaml``. It is not hand-maintained folklore — the test
``tests/test_errors.py::test_every_contract_error_code_has_an_exception`` reads
the contract and fails if the server grows a code this table does not name.

An unrecognised code is **not** an error in the SDK: it maps to
:class:`ValoriAPIError` with every field intact, so a client written against an
older SDK keeps working against a newer node.
"""

from __future__ import annotations

from typing import Any, Mapping, Optional

__all__ = [
    "ValoriError",
    "ValoriConfigError",
    "ValoriConnectionError",
    "ValoriTimeoutError",
    "ValoriAPIError",
    "BadRequestError",
    "ValidationError",
    "AuthenticationError",
    "AuthorizationError",
    "NotFoundError",
    "CollectionNotFoundError",
    "RecordNotFoundError",
    "DimensionMismatchError",
    "InvalidMetricError",
    "InvalidIndexError",
    "IndexBuildFailedError",
    "ConflictError",
    "CollectionAlreadyExistsError",
    "CapacityExceededError",
    "NotLeaderError",
    "ServiceUnavailableError",
    "NotImplementedAPIError",
    "ServerError",
    "RateLimitError",
    "OperationFailedError",
    "OperationTimeoutError",
    "error_for",
]


class ValoriError(Exception):
    """Base class for every error this SDK raises."""


class ValoriConfigError(ValoriError, ValueError):
    """The client was constructed or called with an unusable configuration."""


class ValoriConnectionError(ValoriError):
    """The node could not be reached at all (DNS, TCP, TLS)."""


class ValoriTimeoutError(ValoriError):
    """A request exceeded the configured timeout."""


class ValoriAPIError(ValoriError):
    """The node answered with an error status.

    This is the base class for every status-bearing failure, and also the
    concrete class used when the server's ``code`` is one this SDK does not
    recognise. In that case nothing is lost — ``code``, ``message``,
    ``status_code``, ``request_id`` and ``body`` all carry the raw values.
    """

    def __init__(
        self,
        message: str,
        *,
        status_code: int,
        code: Optional[str] = None,
        request_id: Optional[str] = None,
        body: Any = None,
        headers: Optional[Mapping[str, str]] = None,
    ) -> None:
        super().__init__(message)
        self.message = message
        self.status_code = status_code
        self.code = code
        self.request_id = request_id
        self.body = body
        self.headers = dict(headers) if headers else {}

    def __str__(self) -> str:  # pragma: no cover - formatting only
        parts = [f"HTTP {self.status_code}"]
        if self.code:
            parts.append(self.code)
        parts.append(self.message)
        if self.request_id:
            parts.append(f"(request_id={self.request_id})")
        return " ".join(parts)

    def __repr__(self) -> str:  # pragma: no cover - formatting only
        return (
            f"{type(self).__name__}(status_code={self.status_code!r}, "
            f"code={self.code!r}, message={self.message!r}, "
            f"request_id={self.request_id!r})"
        )


class BadRequestError(ValoriAPIError):
    """400 — the request was malformed."""


class ValidationError(BadRequestError):
    """``validation_error`` — the request body failed validation."""


class AuthenticationError(ValoriAPIError):
    """``unauthorized`` — missing or invalid API key."""


class AuthorizationError(ValoriAPIError):
    """``forbidden`` — authenticated, but not permitted."""


class NotFoundError(ValoriAPIError):
    """``not_found`` — the addressed resource does not exist."""


class CollectionNotFoundError(NotFoundError):
    """``collection_not_found``."""


class RecordNotFoundError(NotFoundError):
    """``record_not_found``."""


class DimensionMismatchError(BadRequestError):
    """``dimension_mismatch`` — vector length disagrees with the collection."""


class InvalidMetricError(BadRequestError):
    """``invalid_metric``."""


class InvalidIndexError(BadRequestError):
    """``invalid_index``."""


class IndexBuildFailedError(ValoriAPIError):
    """``index_build_failed``."""


class ConflictError(ValoriAPIError):
    """``conflict`` — the request collides with existing state."""


class CollectionAlreadyExistsError(ConflictError):
    """A collection create collided with an existing collection.

    Honest note: the node does **not** have a distinct ``collection_already_exists``
    error code today — it reports this as ``conflict``. This subclass is raised
    by :meth:`valori.resources.collections.Collections.create` only, where the
    operation itself makes the meaning unambiguous. Everywhere else a
    ``conflict`` stays a :class:`ConflictError`. If the contract later gains a
    dedicated code, the mapping moves into ``_CODE_MAP`` and this note goes away.
    """


class CapacityExceededError(ValoriAPIError):
    """``capacity_exceeded`` — the node's slab is full (HTTP 507)."""


class NotLeaderError(ValoriAPIError):
    """``not_leader`` — a write reached a follower in cluster mode."""


class ServiceUnavailableError(ValoriAPIError):
    """``unavailable`` — the node is not currently able to serve."""


class NotImplementedAPIError(ValoriAPIError):
    """``not_implemented`` — the feature is not enabled on this node."""


class ServerError(ValoriAPIError):
    """``internal_error`` — an unexpected server-side failure."""


class RateLimitError(ValoriAPIError):
    """HTTP 429. Carries ``retry_after`` in seconds when the server sent it."""

    def __init__(self, *args: Any, retry_after: Optional[float] = None, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self.retry_after = retry_after


class OperationFailedError(ValoriError):
    """A polled long-running operation reached a terminal failure state."""

    def __init__(self, message: str, *, operation_id: str, status: str, detail: Any = None) -> None:
        super().__init__(message)
        self.operation_id = operation_id
        self.status = status
        self.detail = detail


class OperationTimeoutError(ValoriTimeoutError):
    """A polled operation did not reach a terminal state before the deadline."""

    def __init__(self, message: str, *, operation_id: str, last_status: Optional[str] = None) -> None:
        super().__init__(message)
        self.operation_id = operation_id
        self.last_status = last_status


# ── code → exception ─────────────────────────────────────────────────────────
#
# Keys are the closed ErrorCode enum in api/openapi/valori-v1.yaml.
_CODE_MAP: dict[str, type[ValoriAPIError]] = {
    "validation_error": ValidationError,
    "unauthorized": AuthenticationError,
    "forbidden": AuthorizationError,
    "not_found": NotFoundError,
    "collection_not_found": CollectionNotFoundError,
    "record_not_found": RecordNotFoundError,
    "dimension_mismatch": DimensionMismatchError,
    "invalid_metric": InvalidMetricError,
    "invalid_index": InvalidIndexError,
    "index_build_failed": IndexBuildFailedError,
    "conflict": ConflictError,
    "capacity_exceeded": CapacityExceededError,
    "not_leader": NotLeaderError,
    "unavailable": ServiceUnavailableError,
    "not_implemented": NotImplementedAPIError,
    "internal_error": ServerError,
}

# Fallback when the body carries no usable ``code`` at all (a proxy 502, an
# HTML error page, a truncated response). Status is all we have to go on.
_STATUS_MAP: dict[int, type[ValoriAPIError]] = {
    400: BadRequestError,
    401: AuthenticationError,
    403: AuthorizationError,
    404: NotFoundError,
    409: ConflictError,
    429: RateLimitError,
    500: ServerError,
    501: NotImplementedAPIError,
    503: ServiceUnavailableError,
    507: CapacityExceededError,
}


def _retry_after_seconds(headers: Optional[Mapping[str, str]]) -> Optional[float]:
    if not headers:
        return None
    raw = None
    for key, value in headers.items():
        if key.lower() == "retry-after":
            raw = value
            break
    if raw is None:
        return None
    try:
        return float(raw)
    except (TypeError, ValueError):
        # HTTP-date form. We do not guess a clock skew — a caller that needs
        # the date can read it off ``headers``.
        return None


def _request_id(headers: Optional[Mapping[str, str]], body: Any) -> Optional[str]:
    if headers:
        for key, value in headers.items():
            if key.lower() in ("x-request-id", "x-valori-request-id", "request-id"):
                return value
    if isinstance(body, Mapping):
        for key in ("request_id", "requestId"):
            if isinstance(body.get(key), str):
                return body[key]
    return None


def error_for(
    status_code: int,
    *,
    code: Optional[str] = None,
    message: Optional[str] = None,
    body: Any = None,
    headers: Optional[Mapping[str, str]] = None,
) -> ValoriAPIError:
    """Build the most specific exception for a server error response.

    Resolution order:

    1. HTTP 429 always becomes :class:`RateLimitError` — the contract has no
       ``rate_limited`` code, so status is the only signal.
    2. A recognised ``code`` selects the class.
    3. Otherwise the status code selects the class.
    4. Otherwise :class:`ValoriAPIError`.

    In every branch the raw status, code, message, body, headers and request id
    are attached to the exception.
    """
    text = message or (body if isinstance(body, str) else None) or "request failed"
    request_id = _request_id(headers, body)

    if status_code == 429:
        return RateLimitError(
            text,
            status_code=status_code,
            code=code,
            request_id=request_id,
            body=body,
            headers=headers,
            retry_after=_retry_after_seconds(headers),
        )

    cls: type[ValoriAPIError]
    if code and code in _CODE_MAP:
        cls = _CODE_MAP[code]
    else:
        cls = _STATUS_MAP.get(status_code, ValoriAPIError)

    return cls(
        text,
        status_code=status_code,
        code=code,
        request_id=request_id,
        body=body,
        headers=headers,
    )
