# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
Integration-layer tests for ValoricoreLangChain and ValoricoreLlamaIndex.

These tests use DummyEmbedder / HashEmbedder so they run fully offline
with zero API keys and zero external dependencies beyond the SDK itself.
They are deliberately lightweight — they test the adapter wiring, not the
kernel internals (those are covered by Rust tests in node/tests/).
"""

import json
import tempfile
import pytest

from valoricore.integrations import ValoricoreLangChain, ValoricoreRetriever, ValoricoreLlamaIndex
from valoricore.embeddings import DummyEmbedder, HashEmbedder


# ── Helpers ───────────────────────────────────────────────────────────────────

DIM = 16   # must match DummyEmbedder output dim


class _LangChainEmbeddingAdapter:
    """
    Thin shim so valoricore's DummyEmbedder satisfies LangChain's Embeddings interface
    (embed_documents / embed_query) without requiring langchain-core in CI.
    """
    def __init__(self, embedder):
        self._e = embedder

    def embed_documents(self, texts):
        return [self._e.embed(t) for t in texts]

    def embed_query(self, text):
        return self._e.embed(text)


def _make_langchain_store(tmp_path: str) -> ValoricoreLangChain:
    embedding = _LangChainEmbeddingAdapter(DummyEmbedder(dim=DIM))
    return ValoricoreLangChain(path=tmp_path, embedding=embedding)


def _make_llamaindex_store(tmp_path: str) -> ValoricoreLlamaIndex:
    return ValoricoreLlamaIndex(path=tmp_path)


# ── Import check ──────────────────────────────────────────────────────────────

def test_integrations_importable():
    """The public import surface must always work, even without LangChain/LlamaIndex."""
    from valoricore.integrations import (
        ValoricoreLangChain,
        ValoricoreRetriever,
        ValoricoreLlamaIndex,
    )
    from valoricore import ValoricoreLangChain, ValoricoreLlamaIndex, ValoricoreRetriever


# ── ValoricoreLangChain ───────────────────────────────────────────────────────

class TestValoricoreLangChain:

    def test_add_texts_returns_string_ids(self, tmp_path):
        store = _make_langchain_store(str(tmp_path))
        ids = store.add_texts(["hello world", "fixed point"])
        assert len(ids) == 2
        assert all(isinstance(i, str) for i in ids)

    def test_add_texts_with_metadata(self, tmp_path):
        store = _make_langchain_store(str(tmp_path))
        ids = store.add_texts(
            ["doc with meta"],
            metadatas=[{"source": "unit-test", "page": 1}],
        )
        assert len(ids) == 1

    def test_similarity_search_returns_documents(self, tmp_path):
        store = _make_langchain_store(str(tmp_path))
        store.add_texts(["alpha beta", "gamma delta", "epsilon zeta"])
        docs = store.similarity_search("alpha", k=2)
        assert len(docs) == 2
        assert all(hasattr(d, "page_content") for d in docs)

    def test_similarity_search_metadata_roundtrip(self, tmp_path):
        store = _make_langchain_store(str(tmp_path))
        store.add_texts(["roundtrip doc"], metadatas=[{"key": "value42"}])
        docs = store.similarity_search("roundtrip", k=1)
        assert docs[0].metadata.get("key") == "value42"
        assert docs[0].page_content == "roundtrip doc"

    def test_similarity_search_with_score_shape(self, tmp_path):
        store = _make_langchain_store(str(tmp_path))
        store.add_texts(["scored doc"])
        pairs = store.similarity_search_with_score("scored", k=1)
        assert len(pairs) == 1
        doc, score = pairs[0]
        assert hasattr(doc, "page_content")
        assert isinstance(score, float)
        assert score >= 0.0, "L2 distance must be non-negative"

    def test_similarity_search_by_vector(self, tmp_path):
        store = _make_langchain_store(str(tmp_path))
        store.add_texts(["vector doc"])
        vec  = DummyEmbedder(dim=DIM).embed("anything")
        docs = store.similarity_search_by_vector(vec, k=1)
        assert len(docs) == 1

    def test_add_documents(self, tmp_path):
        try:
            from langchain_core.documents import Document
        except ImportError:
            pytest.skip("langchain-core not installed")

        store = _make_langchain_store(str(tmp_path))
        docs  = [Document(page_content="langchain doc", metadata={"src": "test"})]
        ids   = store.add_documents(docs)
        assert len(ids) == 1

    def test_from_texts_classmethod(self, tmp_path):
        embedding = _LangChainEmbeddingAdapter(DummyEmbedder(dim=DIM))
        store = ValoricoreLangChain.from_texts(
            texts     = ["fromtexts doc"],
            embedding = embedding,
            path      = str(tmp_path),
        )
        assert isinstance(store, ValoricoreLangChain)
        docs = store.similarity_search("fromtexts", k=1)
        assert len(docs) == 1

    def test_as_retriever_returns_retriever(self, tmp_path):
        store     = _make_langchain_store(str(tmp_path))
        retriever = store.as_retriever(k=3)
        assert isinstance(retriever, ValoricoreRetriever)

    def test_retriever_get_relevant_documents(self, tmp_path):
        store = _make_langchain_store(str(tmp_path))
        store.add_texts(["retriever test doc"])
        retriever = store.as_retriever(k=1)
        docs = retriever.get_relevant_documents("retriever")
        assert len(docs) == 1

    def test_get_state_hash_64_chars(self, tmp_path):
        store = _make_langchain_store(str(tmp_path))
        store.add_texts(["state hash test"])
        h = store.get_state_hash()
        assert isinstance(h, str)
        assert len(h) == 64

    def test_snapshot_restore_preserves_hash(self, tmp_path):
        store = _make_langchain_store(str(tmp_path / "original"))
        store.add_texts(["snap doc 1", "snap doc 2"])
        hash_before = store.get_state_hash()
        snap        = store.snapshot()

        store2 = _make_langchain_store(str(tmp_path / "restored"))
        store2.restore(snap)
        assert store2.get_state_hash() == hash_before

    def test_state_hash_changes_on_insert(self, tmp_path):
        store  = _make_langchain_store(str(tmp_path))
        h0     = store.get_state_hash()
        store.add_texts(["new record"])
        h1     = store.get_state_hash()
        assert h0 != h1

    def test_empty_texts_no_crash(self, tmp_path):
        store = _make_langchain_store(str(tmp_path))
        ids   = store.add_texts([])
        assert ids == []


# ── ValoricoreLlamaIndex ──────────────────────────────────────────────────────

class TestValoricoreLlamaIndex:

    def _make_text_node(self, text: str, embedding: list, node_id: str = None, meta: dict = None):
        """Create a minimal TextNode-like object for testing without llama_index."""
        try:
            from llama_index.core.schema import TextNode
            return TextNode(
                text      = text,
                id_       = node_id or "node-0",
                embedding = embedding,
                metadata  = meta or {},
            )
        except ImportError:
            pytest.skip("llama-index-core not installed")

    def test_stores_text_flag(self, tmp_path):
        store = _make_llamaindex_store(str(tmp_path))
        assert store.stores_text is True

    def test_is_embedding_query_flag(self, tmp_path):
        store = _make_llamaindex_store(str(tmp_path))
        assert store.is_embedding_query is True

    def test_client_property(self, tmp_path):
        store = _make_llamaindex_store(str(tmp_path))
        assert store.client is store._client

    def test_add_returns_node_ids(self, tmp_path):
        store    = _make_llamaindex_store(str(tmp_path))
        embedder = DummyEmbedder(dim=DIM)
        node     = self._make_text_node(
            text      = "llama doc",
            embedding = embedder.embed("llama doc"),
            node_id   = "node-abc",
        )
        ids = store.add([node])
        assert ids == ["node-abc"]

    def test_add_skips_nodes_without_embedding(self, tmp_path):
        store = _make_llamaindex_store(str(tmp_path))
        node  = self._make_text_node(
            text      = "no embedding",
            embedding = None,
            node_id   = "node-no-emb",
        )
        node.embedding = None   # force-clear
        ids = store.add([node])
        assert ids == []

    def test_query_returns_result(self, tmp_path):
        try:
            from llama_index.core.vector_stores.types import VectorStoreQuery
        except ImportError:
            pytest.skip("llama-index-core not installed")

        store    = _make_llamaindex_store(str(tmp_path))
        embedder = DummyEmbedder(dim=DIM)

        node = self._make_text_node(
            text      = "llamaindex query doc",
            embedding = embedder.embed("llamaindex query doc"),
            node_id   = "node-q1",
        )
        store.add([node])

        q      = VectorStoreQuery(query_embedding=embedder.embed("llamaindex"), similarity_top_k=1)
        result = store.query(q)

        assert len(result.nodes) == 1
        assert len(result.similarities) == 1
        assert len(result.ids) == 1
        assert result.nodes[0].text == "llamaindex query doc"

    def test_query_similarity_in_range(self, tmp_path):
        try:
            from llama_index.core.vector_stores.types import VectorStoreQuery
        except ImportError:
            pytest.skip("llama-index-core not installed")

        store    = _make_llamaindex_store(str(tmp_path))
        embedder = DummyEmbedder(dim=DIM)
        node     = self._make_text_node("sim range doc", embedder.embed("sim range doc"), "n1")
        store.add([node])

        q      = VectorStoreQuery(query_embedding=embedder.embed("sim"), similarity_top_k=1)
        result = store.query(q)

        sim = result.similarities[0]
        assert 0.0 < sim <= 1.0, f"Similarity must be in (0, 1], got {sim}"

    def test_query_empty_embedding_returns_empty(self, tmp_path):
        try:
            from llama_index.core.vector_stores.types import VectorStoreQuery
        except ImportError:
            pytest.skip("llama-index-core not installed")

        store  = _make_llamaindex_store(str(tmp_path))
        q      = VectorStoreQuery(query_embedding=None, similarity_top_k=4)
        result = store.query(q)
        assert result.nodes == []

    def test_get_state_hash(self, tmp_path):
        store    = _make_llamaindex_store(str(tmp_path))
        embedder = DummyEmbedder(dim=DIM)
        node     = self._make_text_node("hash node", embedder.embed("hash node"), "n-hash")
        store.add([node])
        h = store.get_state_hash()
        assert len(h) == 64

    def test_snapshot_restore(self, tmp_path):
        store    = _make_llamaindex_store(str(tmp_path / "orig"))
        embedder = DummyEmbedder(dim=DIM)
        node     = self._make_text_node("snap node", embedder.embed("snap node"), "n-snap")
        store.add([node])

        hash_before = store.get_state_hash()
        snap        = store.snapshot()

        store2 = _make_llamaindex_store(str(tmp_path / "restored"))
        store2.restore(snap)
        assert store2.get_state_hash() == hash_before
