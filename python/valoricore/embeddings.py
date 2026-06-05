# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
valoricore.embeddings
~~~~~~~~~~~~~~~~~~~~~

Production-ready embedding provider adapters for Valoricore.

Each provider exposes a **synchronous** `embed(text: str) -> List[float]`
callable (and an `embed_batch` for throughput) that is directly compatible
with every Valoricore API that accepts an `EmbedFn`.

Quick-start
-----------
Local / offline (no API key needed)::

    from valoricore.embeddings import SentenceTransformerEmbedder
    embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
    vec = embedder.embed("Hello world")          # -> List[float], dim=384

OpenAI (cloud)::

    from valoricore.embeddings import OpenAIEmbedder
    embedder = OpenAIEmbedder(api_key="sk-...")
    vec = embedder.embed("Hello world")          # -> List[float], dim=1536

Cohere (cloud)::

    from valoricore.embeddings import CohereEmbedder
    embedder = CohereEmbedder(api_key="...")

HuggingFace Inference API (cloud)::

    from valoricore.embeddings import HuggingFaceEmbedder
    embedder = HuggingFaceEmbedder(api_key="hf_...", model="sentence-transformers/all-MiniLM-L6-v2")

Ollama (local server)::

    from valoricore.embeddings import OllamaEmbedder
    embedder = OllamaEmbedder(model="nomic-embed-text")

Testing / CI (deterministic zeros, no model required)::

    from valoricore.embeddings import DummyEmbedder
    embedder = DummyEmbedder(dim=384)
"""

from __future__ import annotations

import os
import hashlib
import logging
from typing import Callable, List, Optional, Any

# Every provider uses this logger — set level with logging.getLogger("valoricore.embeddings")
logger = logging.getLogger(__name__)

# ─────────────────────────────────────────────────────────────────────────────
# EmbedFn is the one-and-only type that Valoricore APIs accept.
# Any callable that takes a str and returns List[float] qualifies —
# including a lambda, a plain function, or an instance of BaseEmbedder.
# ─────────────────────────────────────────────────────────────────────────────
EmbedFn = Callable[[str], List[float]]


# ─────────────────────────────────────────────────────────────────────────────
# Base class
# ─────────────────────────────────────────────────────────────────────────────
class BaseEmbedder:
    """
    Abstract base for all embedding providers.

    Subclass this to implement a custom provider – override ``embed`` and
    optionally ``embed_batch`` for efficient batching.
    """

    #: Embedding dimension.  Set by subclasses or detected at first call.
    dim: Optional[int] = None

    def embed(self, text: str) -> List[float]:
        """Embed a single string. Must return ``List[float]``."""
        raise NotImplementedError

    def embed_batch(self, texts: List[str]) -> List[List[float]]:
        """
        Embed multiple strings efficiently.

        The default implementation just loops over embed() — which is fine
        for quick prototyping.  Cloud providers (OpenAI, Cohere) override this
        to batch the whole list in ONE API call, which is dramatically faster
        and much cheaper.
        """
        return [self.embed(t) for t in texts]

    def __call__(self, text: str) -> List[float]:
        # This is the magic that makes `embedder` passable directly as an EmbedFn.
        # Every Valoricore API that accepts `embed=...` will just call the object
        # like a function — e.g. client.add_document(text=..., embed=embedder)
        return self.embed(text)

    def __repr__(self) -> str:
        return f"{self.__class__.__name__}(dim={self.dim})"


# ─────────────────────────────────────────────────────────────────────────────
# 1. Dummy / Testing
# ─────────────────────────────────────────────────────────────────────────────
class DummyEmbedder(BaseEmbedder):
    """
    Deterministic zero-vector embedder for CI and unit tests.

    No network calls, no model weights – returns a fixed-length list of
    zeros.  Every text maps to an identical vector so search results are
    meaningless, but all API plumbing can be tested end-to-end.

    Args:
        dim: Output dimension. Must match the dimension your kernel was
             initialised with (default: 384).

    Example::

        from valoricore.embeddings import DummyEmbedder
        embed = DummyEmbedder(dim=384)
        assert len(embed("hello")) == 384
    """

    def __init__(self, dim: int = 384) -> None:
        # dim=384 is the default because all-MiniLM-L6-v2 uses 384 dimensions.
        # Change this to match whatever model your kernel was compiled with.
        self.dim = dim

    def embed(self, text: str) -> List[float]:
        # Always return zeros — the text is intentionally ignored.
        # This makes every document "equally close" to every query,
        # so search results are meaningless but all the wiring still works.
        return [0.0] * self.dim

    def embed_batch(self, texts: List[str]) -> List[List[float]]:
        return [[0.0] * self.dim for _ in texts]


class HashEmbedder(BaseEmbedder):
    """
    Deterministic hash-based embedder for testing with *distinct* vectors.

    Each unique text produces a unique, reproducible embedding by hashing
    the text and expanding the digest into a float list.  Useful for testing
    search correctness (distinct texts → distinct vectors) without any model.

    Args:
        dim: Output dimension (default: 384).

    Example::

        from valoricore.embeddings import HashEmbedder
        embed = HashEmbedder(dim=384)
        assert embed("foo") != embed("bar")   # distinct vectors
        assert embed("foo") == embed("foo")   # deterministic
    """

    def __init__(self, dim: int = 384) -> None:
        self.dim = dim

    def embed(self, text: str) -> List[float]:
        # Strategy: hash the text with SHA-256, then keep hashing sub-blocks
        # until we've produced enough bytes.  Map each byte [0,255] → [-1.0, 1.0].
        # This gives us a deterministic, unique-per-text vector with no model.
        seed = hashlib.sha256(text.encode("utf-8")).digest()
        floats: List[float] = []
        i = 0
        while len(floats) < self.dim:
            # Use the block index as extra entropy so consecutive blocks differ
            chunk = hashlib.sha256(seed + i.to_bytes(4, "little")).digest()
            for b in chunk:
                floats.append((b / 127.5) - 1.0)  # [0,255] → [-1, 1]
                if len(floats) == self.dim:
                    break
            i += 1
        return floats


# ─────────────────────────────────────────────────────────────────────────────
# 2. Sentence Transformers  (local / offline)
# ─────────────────────────────────────────────────────────────────────────────
class SentenceTransformerEmbedder(BaseEmbedder):
    """
    Local offline embedder backed by the ``sentence-transformers`` library.

    No API key, no network call after the initial model download.
    Recommended for **production** self-hosted deployments.

    Install extra::

        pip install sentence-transformers

    Args:
        model_name: Any model name from HuggingFace Hub supported by
                    ``sentence-transformers`` (default: ``"all-MiniLM-L6-v2"``
                    which produces 384-dimensional vectors).
        device:     Torch device string (``"cpu"``, ``"cuda"``, ``"mps"``).
                    ``None`` → auto-detect.
        normalize:  If ``True``, L2-normalise the output vectors.  Normalised
                    cosine similarity equals dot product.

    Popular models
    ~~~~~~~~~~~~~~
    ============================================= ===== ==============
    Model                                         Dim   Notes
    ============================================= ===== ==============
    ``all-MiniLM-L6-v2``                          384   Fast, quality
    ``all-mpnet-base-v2``                         768   High quality
    ``paraphrase-multilingual-MiniLM-L12-v2``     384   Multi-lingual
    ``BAAI/bge-small-en-v1.5``                    384   State-of-art
    ``BAAI/bge-large-en-v1.5``                    1024  Best quality
    ============================================= ===== ==============

    Example::

        from valoricore.embeddings import SentenceTransformerEmbedder
        from valoricore import MemoryClient

        embedder = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
        client   = MemoryClient()
        result   = client.add_document(
            text  = "Valoricore is deterministic.",
            embed = embedder,          # callable – works directly
        )
    """

    def __init__(
        self,
        model_name: str = "all-MiniLM-L6-v2",
        device: Optional[str] = None,
        normalize: bool = False,
    ) -> None:
        try:
            from sentence_transformers import SentenceTransformer
        except ImportError as exc:
            raise ImportError(
                "sentence-transformers is not installed. "
                "Install it with: pip install sentence-transformers"
            ) from exc

        self.model_name = model_name
        self._normalize = normalize

        # SentenceTransformer will auto-select CPU/CUDA/MPS when device=None.
        # Pass device="mps" on Apple Silicon for a big speed boost.
        self._model = SentenceTransformer(model_name, device=device)

        # We send a tiny probe string to discover the actual output dimension.
        # This is cheaper than parsing the model's config and always correct.
        probe = self._model.encode("probe", convert_to_numpy=True)
        self.dim = probe.shape[-1]
        logger.info(
            "SentenceTransformerEmbedder ready: model=%s dim=%d device=%s",
            model_name, self.dim, device or "auto",
        )

    def embed(self, text: str) -> List[float]:
        vec = self._model.encode(
            text,
            convert_to_numpy=True,
            normalize_embeddings=self._normalize,
        )
        # .tolist() converts numpy float32 → Python float — required for JSON
        return vec.tolist()

    def embed_batch(self, texts: List[str]) -> List[List[float]]:
        """Uses the model's internal batching — much faster than looping embed()."""
        vecs = self._model.encode(
            texts,
            convert_to_numpy=True,
            normalize_embeddings=self._normalize,
            show_progress_bar=False,  # keep logs clean in production
        )
        return vecs.tolist()


# ─────────────────────────────────────────────────────────────────────────────
# 3. OpenAI  (cloud)
# ─────────────────────────────────────────────────────────────────────────────
class OpenAIEmbedder(BaseEmbedder):
    """
    Cloud embedder backed by the OpenAI Embeddings API.

    Install extra::

        pip install openai

    Args:
        api_key:    OpenAI API key. Falls back to the ``OPENAI_API_KEY``
                    environment variable.
        model:      Model identifier (default: ``"text-embedding-3-small"``,
                    1536 dimensions).
        dimensions: Optional output truncation. Supported by
                    ``text-embedding-3-*`` models only.
        max_retries: Number of automatic retries on transient API errors.

    Available models
    ~~~~~~~~~~~~~~~~
    ============================= ====== =========
    Model                         Dim    Quality
    ============================= ====== =========
    ``text-embedding-3-small``    1536   Fastest / cheapest
    ``text-embedding-3-large``    3072   Highest quality
    ``text-embedding-ada-002``    1536   Legacy
    ============================= ====== =========

    Example::

        from valoricore.embeddings import OpenAIEmbedder
        from valoricore import MemoryClient

        embedder = OpenAIEmbedder()          # reads OPENAI_API_KEY from env
        client   = MemoryClient()
        result   = client.add_document(
            text  = "Determinism in AI systems.",
            embed = embedder,
        )
    """

    def __init__(
        self,
        api_key: Optional[str] = None,
        model: str = "text-embedding-3-small",
        dimensions: Optional[int] = None,
        max_retries: int = 3,
    ) -> None:
        try:
            from openai import OpenAI
        except ImportError as exc:
            raise ImportError(
                "openai is not installed. Install it with: pip install openai"
            ) from exc

        # Prefer an explicit key; fall back to the environment variable.
        # This matches the pattern most OpenAI users already have configured.
        resolved_key = api_key or os.environ.get("OPENAI_API_KEY")
        if not resolved_key:
            raise ValueError(
                "No OpenAI API key provided. Pass api_key= or set the "
                "OPENAI_API_KEY environment variable."
            )

        # max_retries is built into the OpenAI client — handles rate limits
        # and transient 5xx errors automatically with exponential back-off.
        self._client = OpenAI(api_key=resolved_key, max_retries=max_retries)
        self._model = model
        self._dimensions = dimensions  # only supported by text-embedding-3-* models

        # Make one tiny API call to discover the output dimension.
        # If dimensions= was specified we trust that value to avoid a round-trip.
        self.dim = dimensions or self._probe_dim()
        logger.info("OpenAIEmbedder ready: model=%s dim=%d", model, self.dim)

    def _probe_dim(self) -> int:
        # Embed the word "probe" — shortest possible call just to learn the vector size.
        kwargs: dict = {"input": "probe", "model": self._model}
        resp = self._client.embeddings.create(**kwargs)
        return len(resp.data[0].embedding)

    def embed(self, text: str) -> List[float]:
        kwargs: dict = {"input": text, "model": self._model}
        if self._dimensions:
            # Matryoshka truncation — keeps only the first N dimensions.
            # Quality drops slightly but storage and search are much cheaper.
            kwargs["dimensions"] = self._dimensions
        resp = self._client.embeddings.create(**kwargs)
        return resp.data[0].embedding

    def embed_batch(self, texts: List[str]) -> List[List[float]]:
        """Send ALL texts in one API call — dramatically cheaper than looping embed()."""
        kwargs: dict = {"input": texts, "model": self._model}
        if self._dimensions:
            kwargs["dimensions"] = self._dimensions
        resp = self._client.embeddings.create(**kwargs)
        # OpenAI guarantees order matches input, but we sort by index to be safe.
        return [item.embedding for item in sorted(resp.data, key=lambda x: x.index)]


# ─────────────────────────────────────────────────────────────────────────────
# 4. Cohere  (cloud)
# ─────────────────────────────────────────────────────────────────────────────
class CohereEmbedder(BaseEmbedder):
    """
    Cloud embedder backed by the Cohere Embed API.

    Install extra::

        pip install cohere

    Args:
        api_key:    Cohere API key. Falls back to ``COHERE_API_KEY`` env var.
        model:      Model identifier (default: ``"embed-english-v3.0"``).
        input_type: Cohere input type hint.  Use ``"search_document"`` when
                    indexing and ``"search_query"`` when querying (improves
                    retrieval quality).

    Example::

        from valoricore.embeddings import CohereEmbedder

        doc_embed   = CohereEmbedder(input_type="search_document")
        query_embed = CohereEmbedder(input_type="search_query")

        result = client.add_document(text="...", embed=doc_embed)
        hits   = client.semantic_search("...", embed=query_embed, k=5)
    """

    def __init__(
        self,
        api_key: Optional[str] = None,
        model: str = "embed-english-v3.0",
        input_type: str = "search_document",
    ) -> None:
        try:
            import cohere
        except ImportError as exc:
            raise ImportError(
                "cohere is not installed. Install it with: pip install cohere"
            ) from exc

        resolved_key = api_key or os.environ.get("COHERE_API_KEY")
        if not resolved_key:
            raise ValueError(
                "No Cohere API key provided. Pass api_key= or set the "
                "COHERE_API_KEY environment variable."
            )

        self._co = cohere.Client(resolved_key)
        self._model = model
        self._input_type = input_type

        # Cohere v3 embed-english-v3.0 → 1024 dims
        probe = self._co.embed(
            texts=["probe"], model=model, input_type=input_type
        ).embeddings
        self.dim = len(probe[0])
        logger.info("CohereEmbedder ready: model=%s dim=%d", model, self.dim)

    def embed(self, text: str) -> List[float]:
        resp = self._co.embed(
            texts=[text], model=self._model, input_type=self._input_type
        )
        return list(resp.embeddings[0])

    def embed_batch(self, texts: List[str]) -> List[List[float]]:
        resp = self._co.embed(
            texts=texts, model=self._model, input_type=self._input_type
        )
        return [list(e) for e in resp.embeddings]


# ─────────────────────────────────────────────────────────────────────────────
# 5. HuggingFace Inference API  (cloud)
# ─────────────────────────────────────────────────────────────────────────────
class HuggingFaceEmbedder(BaseEmbedder):
    """
    Cloud embedder backed by the HuggingFace Inference API.

    No local GPU required – runs inference on HuggingFace servers.

    Install extra::

        pip install requests   # already a core dependency

    Args:
        api_key: HuggingFace Hub token. Falls back to ``HF_TOKEN`` env var.
        model:   Any sentence-embedding model on HuggingFace Hub.
        timeout: HTTP timeout in seconds.

    Example::

        from valoricore.embeddings import HuggingFaceEmbedder

        embedder = HuggingFaceEmbedder(
            model="sentence-transformers/all-MiniLM-L6-v2"
        )
    """

    _HF_API = "https://api-inference.huggingface.co/pipeline/feature-extraction"

    def __init__(
        self,
        api_key: Optional[str] = None,
        model: str = "sentence-transformers/all-MiniLM-L6-v2",
        timeout: int = 30,
    ) -> None:
        import requests

        resolved_key = api_key or os.environ.get("HF_TOKEN") or os.environ.get("HUGGINGFACEHUB_API_TOKEN")
        if not resolved_key:
            raise ValueError(
                "No HuggingFace token provided. Pass api_key= or set the "
                "HF_TOKEN environment variable."
            )

        self._session = requests.Session()
        self._session.headers.update({"Authorization": f"Bearer {resolved_key}"})
        self._model = model
        self._timeout = timeout
        self._url = f"{self._HF_API}/{model}"

        # Probe for dim (also warms up the model endpoint)
        probe = self._call(["probe"])
        self.dim = len(probe[0]) if probe else 384
        logger.info("HuggingFaceEmbedder ready: model=%s dim=%d", model, self.dim)

    def _call(self, texts: List[str]) -> List[List[float]]:
        resp = self._session.post(
            self._url,
            json={"inputs": texts, "options": {"wait_for_model": True}},
            timeout=self._timeout,
        )
        resp.raise_for_status()
        result = resp.json()
        # API can return [[float,...]] or [[[float,...]]] – flatten one level if needed
        if result and isinstance(result[0][0], list):
            result = [item[0] for item in result]
        return result

    def embed(self, text: str) -> List[float]:
        return self._call([text])[0]

    def embed_batch(self, texts: List[str]) -> List[List[float]]:
        return self._call(texts)


# ─────────────────────────────────────────────────────────────────────────────
# 6. Ollama  (local server)
# ─────────────────────────────────────────────────────────────────────────────
class OllamaEmbedder(BaseEmbedder):
    """
    Local server embedder backed by `Ollama <https://ollama.com>`_.

    Requires Ollama running locally (``ollama serve``).  No API key needed.
    Ideal for fully air-gapped / on-premise production deployments.

    Install Ollama and pull a model::

        brew install ollama
        ollama pull nomic-embed-text   # 768 dims
        ollama pull mxbai-embed-large  # 1024 dims

    Args:
        model:    Ollama model name (default: ``"nomic-embed-text"``).
        base_url: Ollama server URL (default: ``http://localhost:11434``).
        timeout:  HTTP timeout in seconds.

    Recommended models
    ~~~~~~~~~~~~~~~~~~
    ========================= ====
    Model                     Dim
    ========================= ====
    ``nomic-embed-text``      768
    ``mxbai-embed-large``     1024
    ``all-minilm``            384
    ========================= ====

    Example::

        from valoricore.embeddings import OllamaEmbedder
        from valoricore import MemoryClient

        embedder = OllamaEmbedder(model="nomic-embed-text")
        client   = MemoryClient()
        result   = client.add_document(text="Embedded locally.", embed=embedder)
    """

    def __init__(
        self,
        model: str = "nomic-embed-text",
        base_url: str = "http://localhost:11434",
        timeout: int = 30,
    ) -> None:
        import requests

        self._model = model
        # Ollama's embedding endpoint differs from its chat endpoint
        self._url = f"{base_url.rstrip('/')}/api/embeddings"
        self._timeout = timeout
        # Reuse a session for connection pooling — avoids TCP handshake on every call
        self._session = requests.Session()

        # If this fails, Ollama is probably not running or the model isn't pulled.
        # Run `ollama pull nomic-embed-text` if you see a connection error here.
        probe = self._embed_single("probe")
        self.dim = len(probe)
        logger.info("OllamaEmbedder ready: model=%s dim=%d url=%s", model, self.dim, base_url)

    def _embed_single(self, text: str) -> List[float]:
        # Note: Ollama uses "prompt" (not "input") for the embedding endpoint.
        # This is different from the /api/chat endpoint — easy to mix up.
        resp = self._session.post(
            self._url,
            json={"model": self._model, "prompt": text},
            timeout=self._timeout,
        )
        resp.raise_for_status()
        return resp.json()["embedding"]

    def embed(self, text: str) -> List[float]:
        return self._embed_single(text)

    def embed_batch(self, texts: List[str]) -> List[List[float]]:
        # Ollama's embedding API is single-item only as of v0.1.x.
        # We serialise calls here — not ideal for large batches but correct.
        return [self._embed_single(t) for t in texts]


# ─────────────────────────────────────────────────────────────────────────────
# 7. Cached embedder wrapper  (production utility)
# ─────────────────────────────────────────────────────────────────────────────
class CachedEmbedder(BaseEmbedder):
    """
    LRU-cache wrapper around any :class:`BaseEmbedder`.

    Identical inputs will not trigger a second model/API call.
    Useful when the same chunk text is embedded multiple times during
    document processing or when handling repeated queries.

    Args:
        embedder:  Any ``BaseEmbedder`` instance to wrap.
        max_size:  Maximum number of cached embeddings (default: 1000).

    Example::

        from valoricore.embeddings import SentenceTransformerEmbedder, CachedEmbedder

        raw     = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
        embedder = CachedEmbedder(raw, max_size=5000)
    """

    def __init__(self, embedder: BaseEmbedder, max_size: int = 1000) -> None:
        from functools import lru_cache

        self._embedder = embedder
        self.dim = embedder.dim

        # lru_cache requires hashable keys — strings are hashable, so perfect.
        # We store results as tuples (hashable) and convert back to list on retrieval.
        # max_size controls how many unique texts fit in the cache before LRU eviction.
        @lru_cache(maxsize=max_size)
        def _cached_embed(text: str) -> tuple:
            return tuple(embedder.embed(text))

        self._cached_embed = _cached_embed

    def embed(self, text: str) -> List[float]:
        # Convert tuple back to list — the rest of the SDK always expects list
        return list(self._cached_embed(text))

    def embed_batch(self, texts: List[str]) -> List[List[float]]:
        # Each item benefits from the cache independently
        return [self.embed(t) for t in texts]

    @property
    def cache_info(self):
        """Returns lru_cache hit/miss stats — useful for monitoring in production."""
        return self._cached_embed.cache_info()


# ─────────────────────────────────────────────────────────────────────────────
# 8. Async wrapper  (FastAPI / asyncio compatible)
# ─────────────────────────────────────────────────────────────────────────────
class AsyncEmbedder:
    """
    Async wrapper that runs any synchronous :class:`BaseEmbedder` in a
    thread-pool executor, making it safe to ``await`` from an asyncio
    event loop without blocking it.

    Args:
        embedder: Any ``BaseEmbedder`` instance.

    Example::

        import asyncio
        from valoricore.embeddings import SentenceTransformerEmbedder, AsyncEmbedder

        sync_embedder  = SentenceTransformerEmbedder("all-MiniLM-L6-v2")
        async_embedder = AsyncEmbedder(sync_embedder)

        async def main():
            vec = await async_embedder.embed("Hello from async!")
            print(len(vec))   # 384

        asyncio.run(main())
    """

    def __init__(self, embedder: BaseEmbedder) -> None:
        self._embedder = embedder
        self.dim = embedder.dim

    async def embed(self, text: str) -> List[float]:
        import asyncio
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(None, self._embedder.embed, text)

    async def embed_batch(self, texts: List[str]) -> List[List[float]]:
        import asyncio
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(None, self._embedder.embed_batch, texts)


# ─────────────────────────────────────────────────────────────────────────────
# Convenience factory
# ─────────────────────────────────────────────────────────────────────────────
def get_embedder(provider: str, **kwargs: Any) -> BaseEmbedder:
    """
    Convenience factory that returns a configured :class:`BaseEmbedder` by name.

    Args:
        provider: One of ``"openai"``, ``"cohere"``, ``"huggingface"``,
                  ``"sentence_transformers"`` / ``"local"``,
                  ``"ollama"``, ``"dummy"``, ``"hash"``.
        **kwargs: Forwarded to the selected embedder's ``__init__``.

    Example::

        from valoricore.embeddings import get_embedder

        # Local inference
        embed = get_embedder("local", model_name="all-MiniLM-L6-v2")

        # OpenAI
        embed = get_embedder("openai", api_key="sk-...")

        # Ollama
        embed = get_embedder("ollama", model="nomic-embed-text")
    """
    _map = {
        "openai": OpenAIEmbedder,
        "cohere": CohereEmbedder,
        "huggingface": HuggingFaceEmbedder,
        "hf": HuggingFaceEmbedder,
        "sentence_transformers": SentenceTransformerEmbedder,
        "local": SentenceTransformerEmbedder,
        "ollama": OllamaEmbedder,
        "dummy": DummyEmbedder,
        "hash": HashEmbedder,
    }
    cls = _map.get(provider.lower())
    if cls is None:
        raise ValueError(
            f"Unknown embedding provider '{provider}'. "
            f"Valid options: {sorted(_map.keys())}"
        )
    return cls(**kwargs)


__all__ = [
    "BaseEmbedder",
    "DummyEmbedder",
    "HashEmbedder",
    "SentenceTransformerEmbedder",
    "OpenAIEmbedder",
    "CohereEmbedder",
    "HuggingFaceEmbedder",
    "OllamaEmbedder",
    "CachedEmbedder",
    "AsyncEmbedder",
    "get_embedder",
    "EmbedFn",
]
