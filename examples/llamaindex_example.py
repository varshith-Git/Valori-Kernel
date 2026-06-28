"""
LlamaIndex + Valori Example
=============================

Uses Valori as a LlamaIndex-compatible vector store via SyncRemoteClient.

Requirements:
    pip install llama-index llama-index-embeddings-openai llama-index-llms-openai valoricore

Start a Valori node first:
    VALORI_DIM=1536 cargo run -p valori-node

Usage:
    OPENAI_API_KEY=sk-... python examples/llamaindex_example.py
"""

import os
import sys
from typing import Any, Dict, List, Optional, cast

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

from valoricore.remote import SyncRemoteClient

# ── Inline LlamaIndex adapter ─────────────────────────────────────────────────

from llama_index.core.schema import TextNode, NodeWithScore, QueryBundle
from llama_index.core.vector_stores.types import (
    BasePydanticVectorStore,
    VectorStoreQuery,
    VectorStoreQueryResult,
)


class ValoriVectorStore(BasePydanticVectorStore):
    """Minimal LlamaIndex VectorStore backed by a Valori node."""

    stores_text: bool = True
    flat_metadata: bool = True

    _client: Any
    _collection: str

    def __init__(self, client: SyncRemoteClient, collection: str = "default"):
        super().__init__()
        self._client = client
        self._collection = collection

    def add(self, nodes: List[TextNode], **kwargs) -> List[str]:
        ids = []
        for node in nodes:
            vec = node.embedding
            if vec is None:
                raise ValueError(f"Node {node.node_id} has no embedding — embed first.")
            rid = self._client.insert(vec, text=node.get_content(), collection=self._collection)
            ids.append(str(rid))
        return ids

    def delete(self, ref_doc_id: str, **kwargs) -> None:
        pass  # deletion not yet wired in this demo

    def query(self, query: VectorStoreQuery, **kwargs) -> VectorStoreQueryResult:
        vec = query.query_embedding
        k = query.similarity_top_k or 4
        hits = self._client.search(vec, k=k, collection=self._collection)
        nodes = []
        scores = []
        ids = []
        for h in hits:
            node = TextNode(text=h.get("metadata", ""), id_=str(h["id"]))
            nodes.append(node)
            scores.append(float(h["score"]))
            ids.append(str(h["id"]))
        return VectorStoreQueryResult(nodes=nodes, similarities=scores, ids=ids)

    @property
    def client(self) -> Any:
        return self._client


# ── Demo ──────────────────────────────────────────────────────────────────────

def main():
    print("=" * 60)
    print("LlamaIndex + Valori Chat Engine Example")
    print("=" * 60)

    from llama_index.core import VectorStoreIndex, StorageContext, Document
    from llama_index.core.node_parser import SentenceSplitter
    from llama_index.embeddings.openai import OpenAIEmbedding
    from llama_index.llms.openai import OpenAI

    print("\n1. Connecting to Valori node at http://localhost:3000...")
    client = SyncRemoteClient("http://localhost:3000")
    print(f"   health: {client.health()}")

    print("\n2. Setting up embeddings and LLM...")
    embed_model = OpenAIEmbedding(model="text-embedding-3-small")
    llm = OpenAI(model="gpt-4o-mini", temperature=0)

    print("\n3. Loading documents...")
    documents = [
        Document(text="Valori is a deterministic vector database built in Rust. "
                      "It uses Q16.16 fixed-point arithmetic for bit-identical results across "
                      "x86, ARM, and WASM. The database includes WAL for crash recovery."),
        Document(text="Valori provides cryptographic proofs of memory state via BLAKE3 audit chains. "
                      "Every mutation is hashed into an event log, and the final state hash is "
                      "reproducible by any node that replays the same events."),
        Document(text="Use cases for Valori include robotics with deterministic sensor fusion, "
                      "embedded AI that must produce identical results on each inference pass, "
                      "and safety-critical applications requiring audit trails."),
    ]
    print(f"   {len(documents)} documents ready")

    print("\n4. Building index (chunk → embed → insert into Valori)...")
    splitter = SentenceSplitter(chunk_size=256)
    valori_store = ValoriVectorStore(client)
    storage_ctx = StorageContext.from_defaults(vector_store=valori_store)
    index = VectorStoreIndex.from_documents(
        documents,
        storage_context=storage_ctx,
        embed_model=embed_model,
        transformations=[splitter],
    )
    print("   index built")

    print("\n5. Chat engine — ask questions...")
    engine = index.as_chat_engine(llm=llm, verbose=False)
    questions = [
        "What is Valori and what makes it unique?",
        "What are the main use cases for Valori?",
        "How does Valori prove its memory state?",
    ]
    for q in questions:
        print(f"\n   Q: {q}")
        print(f"   A: {engine.chat(q)}")

    print("\n6. Verifiable state hash (unique to Valori)...")
    h = client.get_state_hash()
    print(f"   {h}")
    print("   Any node replaying the same events produces the exact same hash.")


if __name__ == "__main__":
    if not os.getenv("OPENAI_API_KEY"):
        print("Set OPENAI_API_KEY before running this example.")
        sys.exit(1)
    main()
    print("\nDone.")
