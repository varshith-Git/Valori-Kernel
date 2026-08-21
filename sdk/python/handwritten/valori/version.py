# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Version identity for the Valori Python SDK.

Phase API-4A §14. Two versions, deliberately separate:

``__version__``
    The SDK package version. Patch-level SDK fixes move this and nothing else.

``API_CONTRACT_VERSION``
    The Valori REST API contract this SDK was generated against. It is checked
    against ``api/openapi/valori-v1.yaml`` by
    ``tests/test_contract_version.py``, so the two cannot drift silently.

``MIN_SUPPORTED_API_CONTRACT`` / ``MAX_SUPPORTED_API_CONTRACT``
    The closed range of contract majors this SDK will talk to. A node reporting
    a contract outside this range is an incompatibility, not something to shrug
    at — see :func:`check_api_compatibility`.
"""

from __future__ import annotations

from typing import Optional

__all__ = [
    "__version__",
    "API_CONTRACT_VERSION",
    "MIN_SUPPORTED_API_CONTRACT",
    "MAX_SUPPORTED_API_CONTRACT",
    "check_api_compatibility",
]

__version__ = "0.1.0"

#: major.minor of the contract in api/openapi/valori-v1.yaml (info.version 1.0.0).
API_CONTRACT_VERSION = "1.0"

MIN_SUPPORTED_API_CONTRACT = "1.0"
MAX_SUPPORTED_API_CONTRACT = "1.x"


def _major(version: str) -> Optional[int]:
    try:
        return int(str(version).split(".", 1)[0])
    except (ValueError, AttributeError):
        return None


def check_api_compatibility(node_api_version: str) -> None:
    """Raise if ``node_api_version`` is outside this SDK's supported range.

    Called with the ``api_version`` a node reports from ``GET /v1/version``.
    Silence about an incompatible major is exactly the failure mode §14 forbids.
    """
    from .errors import ValoriConfigError

    node_major = _major(node_api_version)
    want_major = _major(API_CONTRACT_VERSION)
    if node_major is None:
        raise ValoriConfigError(f"node reported an unparseable API version: {node_api_version!r}")
    if node_major != want_major:
        raise ValoriConfigError(
            f"this SDK targets Valori API contract {API_CONTRACT_VERSION} "
            f"(supported: {MIN_SUPPORTED_API_CONTRACT}–{MAX_SUPPORTED_API_CONTRACT}), "
            f"but the node reports {node_api_version}. Install a matching SDK."
        )
