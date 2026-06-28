# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
valoricore.adapters
===================

Low-level HTTP adapter utilities.

  ValoricoreAdapter       — retry-capable HTTP wrapper for a Valoricore node.
                            Use when you need direct REST access with retry logic.
  UpsertItem              — dataclass for batch upsert payloads.
  SentenceTransformerAdapter — sentence-transformers embed function adapter.

For the full LangChain / LlamaIndex integrations use::

    from valoricore.integrations import ValoricoreLangChain, ValoricoreLlamaIndex
"""

from .base import ValoricoreAdapter, UpsertItem
from .sentence_transformers_adapter import SentenceTransformerAdapter

__all__ = [
    "ValoricoreAdapter",
    "UpsertItem",
    "SentenceTransformerAdapter",
]
