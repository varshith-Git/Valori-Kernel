# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Domain → wire encoders for the shapes the contract does not take verbatim.

Phase API-4D §2. There is exactly one place in this SDK where a developer-facing
Python ``dict`` becomes something other than itself on the wire, and this is it.

``metadata`` has three distinct wire shapes in ``api/openapi/valori-v1.yaml``,
and the ergonomic layer hides all three behind one plain mapping:

===============================================  ===========================
endpoint                                          wire shape
===============================================  ===========================
``POST /v1/records``                              ``list[int]`` — UTF-8 bytes
``POST /v1/vectors/batch-insert``                 ``list[str | None]``
memory upsert / metadata sidecar / filters        JSON object, sent verbatim
===============================================  ===========================

The first two are committed *inside* the ``InsertRecord`` event and are
therefore covered by the BLAKE3 audit chain. That is why the node takes bytes
rather than a map: the encoding has to be the caller's, byte for byte, or the
chain would not reproduce. Encoding here preserves that property while letting
callers pass an object.

The serialisation below is deliberately byte-identical to the TypeScript SDK's
``encodeMetadataBytes`` / ``encodeMetadataString`` (``JSON.stringify``):

* ``separators=(",", ":")`` — no whitespace, like ``JSON.stringify``.
* ``ensure_ascii=False``    — emit real UTF-8, like ``JSON.stringify``.
* ``sort_keys=False``       — insertion order, like ``JSON.stringify``.

A record written by the Python SDK and one written by the TypeScript SDK with
the same metadata therefore produce the same event bytes and the same state
hash. Do not "tidy" these arguments.
"""

from __future__ import annotations

import json
from typing import Any, List, Mapping, Optional, Sequence

__all__ = [
    "encode_metadata_bytes",
    "encode_metadata_string",
    "encode_metadata_string_list",
    "encode_metadata_object",
    "encode_metadata_filter",
]


def _canonical_json(value: Mapping[str, Any]) -> str:
    """Serialise a metadata mapping exactly as ``JSON.stringify`` would."""
    if not isinstance(value, Mapping):
        raise TypeError(
            f"metadata must be a mapping of string keys to JSON values, "
            f"got {type(value).__name__}"
        )
    for key in value:
        if not isinstance(key, str):
            raise TypeError(
                f"metadata keys must be strings, got {type(key).__name__}: {key!r}"
            )
    try:
        return json.dumps(
            dict(value),
            separators=(",", ":"),
            ensure_ascii=False,
            sort_keys=False,
            allow_nan=False,
        )
    except (TypeError, ValueError) as exc:  # non-JSON-able value, NaN, Infinity
        raise TypeError(f"metadata is not JSON-serialisable: {exc}") from exc


def encode_metadata_bytes(
    metadata: Optional[Mapping[str, Any]],
) -> Optional[List[int]]:
    """``POST /v1/records`` — opaque UTF-8 JSON bytes as a list of ``u8``.

    ``None`` means "not supplied" and stays ``None`` so :func:`._models.build`
    drops the field rather than sending an explicit null.
    """
    if metadata is None:
        return None
    return list(_canonical_json(metadata).encode("utf-8"))


def encode_metadata_string(
    metadata: Optional[Mapping[str, Any]],
) -> Optional[str]:
    """One entry of ``POST /v1/vectors/batch-insert`` — a UTF-8 JSON string.

    ``None`` is meaningful here: the contract's item type is ``string | null``
    and a null entry means "no metadata for this vector". It is preserved.
    """
    if metadata is None:
        return None
    return _canonical_json(metadata)


def encode_metadata_string_list(
    metadata: Optional[Sequence[Optional[Mapping[str, Any]]]],
) -> Optional[List[Optional[str]]]:
    """The whole ``metadata`` array of ``POST /v1/vectors/batch-insert``."""
    if metadata is None:
        return None
    return [encode_metadata_string(entry) for entry in metadata]


def encode_metadata_object(
    metadata: Optional[Mapping[str, Any]],
) -> Optional[dict]:
    """Endpoints whose ``metadata`` is a real JSON object (memory upsert, sidecar).

    No transformation — but it is still routed through here so the shape is
    *validated* at the boundary rather than silently coerced by the generated
    model's permissive ``from_dict``, and so every metadata-bearing call site
    names which of the three wire shapes it means.
    """
    if metadata is None:
        return None
    # Round-trip through the canonical serialiser: same validation, same
    # rejection of non-JSON-able values, without changing the shape.
    return json.loads(_canonical_json(metadata))


def encode_metadata_filter(
    metadata_filter: Optional[Mapping[str, Any]],
) -> Optional[dict]:
    """``metadata_filter`` on search / memory-search — a JSON object, verbatim.

    Phase I7 predicate syntax: ``{"author": "alice", "year": {"gte": 2020}}``.
    Validated, never rewritten — the SDK must not invent filter semantics.

    NOTE: the server side of this is currently broken; see
    ``docs/api/known-server-issues.md``. That is a server bug and is
    deliberately *not* worked around here.
    """
    return encode_metadata_object(metadata_filter)
