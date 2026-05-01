# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from .base import ValoricoreAdapter, UpsertItem
from .sentence_transformers_adapter import SentenceTransformerAdapter

try:
    from .llamaindex import ValoricoreVectorStore as LlamaIndexVectorStore
except ImportError:
    pass

try:
    from .langchain import ValoricoreRetriever as LangChainRetriever
    from .langchain_vectorstore import ValoricoreVectorStore as LangChainVectorStore
except ImportError:
    pass

__all__ = [
    "ValoricoreAdapter",
    "UpsertItem",
    "SentenceTransformerAdapter",
    "LlamaIndexVectorStore",
    "LangChainRetriever",
    "LangChainVectorStore",
]
