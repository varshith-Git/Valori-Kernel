# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Bridge helpers between Python keyword arguments and generated request models.

The generated models are attrs classes with wire-accurate field names (``from``
becomes ``from_``, ``async`` becomes ``async_``). Rather than remembering which
Python identifier each wire key mangles into, wrappers hand :func:`build` the
*wire* keys and let the generated ``from_dict`` do the mapping. That keeps the
handwritten layer honest: it speaks the contract's vocabulary, not the
generator's.
"""

from __future__ import annotations

from typing import Any, Mapping, TypeVar

T = TypeVar("T")

__all__ = ["build"]


def build(model_cls: type, **fields: Any) -> Any:
    """Construct a generated request model from wire-named keyword arguments.

    ``None`` means "not supplied" and is dropped, so an omitted optional never
    reaches the wire as an explicit null. A caller who genuinely needs to send
    ``null`` should build the generated model directly.
    """
    payload: dict[str, Any] = {}
    for key, value in fields.items():
        if value is None:
            continue
        payload[key] = _plain(value)
    return model_cls.from_dict(payload)


def _plain(value: Any) -> Any:
    """Normalise nested SDK/generated objects into plain JSON-able data."""
    if hasattr(value, "to_dict") and not isinstance(value, type):
        return value.to_dict()
    if isinstance(value, Mapping):
        return {k: _plain(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_plain(v) for v in value]
    return value
