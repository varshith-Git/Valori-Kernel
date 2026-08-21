# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Valori Python SDK — the REST client for a Valori node.

This is the *remote* SDK. It talks HTTP to a running node and is generated from
``api/openapi/valori-v1.yaml``. It is a different thing from ``valoricore``,
the embedded in-process SDK that binds the kernel through PyO3; the two are
independent and can be installed side by side.

Layering (Phase API-4A §4)::

    valori            (this package — handwritten, human-owned)
        ↓
    valori_generated  (machine-owned, regenerate; never hand-edit)
        ↓
    httpx
"""

from .client import ValoriClient
from .errors import (
    AuthenticationError,
    AuthorizationError,
    BadRequestError,
    CapacityExceededError,
    CollectionAlreadyExistsError,
    CollectionNotFoundError,
    ConflictError,
    DimensionMismatchError,
    IndexBuildFailedError,
    InvalidIndexError,
    InvalidMetricError,
    NotFoundError,
    NotImplementedAPIError,
    NotLeaderError,
    OperationFailedError,
    OperationTimeoutError,
    RateLimitError,
    RecordNotFoundError,
    ServerError,
    ServiceUnavailableError,
    ValidationError,
    ValoriAPIError,
    ValoriConfigError,
    ValoriConnectionError,
    ValoriError,
    ValoriTimeoutError,
)
from .resources import Collection, Operation
from .retry import RetryPolicy
from .version import (
    API_CONTRACT_VERSION,
    MAX_SUPPORTED_API_CONTRACT,
    MIN_SUPPORTED_API_CONTRACT,
    __version__,
    check_api_compatibility,
)

__all__ = [
    "ValoriClient",
    "Collection",
    "Operation",
    "RetryPolicy",
    "__version__",
    "API_CONTRACT_VERSION",
    "MIN_SUPPORTED_API_CONTRACT",
    "MAX_SUPPORTED_API_CONTRACT",
    "check_api_compatibility",
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
]
