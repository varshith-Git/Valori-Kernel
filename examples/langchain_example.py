"""
LangChain + Valori Example
===========================

Uses Valori as a LangChain-compatible vector store via SyncRemoteClient.

Requirements:
    pip install langchain langchain-openai openai valoricore

Start a Valori node first:
    VALORI_DIM=1536 cargo run -p valori-node

Usage:
    OPENAI_API_KEY=sk-... python examples/langchain_example.py
"""

import os
import sys
from typing import List, Optional

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

from valoricore.remote import SyncRemoteClient

# ── Inline LangChain adapter ──────────────────────────────────────────────────

from langchain_core.documents import Document
from langchain_core.vectorstores import VectorStore


class ValoriVectorStore(VectorStore):
    """Minimal LangChain VectorStore backed by a Valori node."""

    def __init__(self, client: SyncRemoteClient, embedding, collection: str = "default"):
        self._client = client
        self._embedding = embedding
        self._collection = collection

    def add_texts(self, texts: List[str], metadatas: Optional[List[dict]] = None, **kwargs) -> List[str]:
        vectors = self._embedding.embed_documents(texts)
        ids = []
        for i, (text, vec) in enumerate(zip(texts, vectors)):
            meta = str(metadatas[i]) if metadatas else text
            rid = self._client.insert(vec, text=meta, collection=self._collection)
            ids.append(str(rid))
        return ids

    def similarity_search(self, query: str, k: int = 4, **kwargs) -> List[Document]:
        vec = self._embedding.embed_query(query)
        hits = self._client.search(vec, k=k, collection=self._collection)
        return [
            Document(
                page_content=h.get("metadata", ""),
                metadata={"id": h["id"], "score": h["score"]},
            )
            for h in hits
        ]

    @classmethod
    def from_texts(cls, texts, embedding, metadatas=None, **kwargs):
        client = SyncRemoteClient(kwargs.get("url", "http://localhost:3000"))
        store = cls(client, embedding, kwargs.get("collection", "default"))
        store.add_texts(texts, metadatas)
        return store


# ── Demo ──────────────────────────────────────────────────────────────────────

def main():
    print("=" * 60)
    print("LangChain + Valori RAG Example")
    print("=" * 60)

    from langchain_openai import OpenAIEmbeddings, ChatOpenAI
    from langchain.chains import RetrievalQA

    print("\n1. Connecting to Valori node at http://localhost:3000...")
    client = SyncRemoteClient("http://localhost:3000")
    print(f"   health: {client.health()}")

    print("\n2. Initializing OpenAI embeddings (text-embedding-3-small, dim=1536)...")
    embeddings = OpenAIEmbeddings(model="text-embedding-3-small")

    print("\n3. Loading documents into Valori...")
    documents = [
        "Valori is a deterministic vector database built in Rust.",
        "It uses Q16.16 fixed-point arithmetic for bit-identical results.",
        "Valori works on x86, ARM, and WASM with identical state hashes.",
        "The database includes WAL for crash recovery and durability.",
        "Perfect for robotics, embedded AI, and safety-critical applications.",
        "Valori provides cryptographic proofs of memory state.",
        "It is designed for reproducible AI systems.",
    ]
    metadatas = [{"source": "docs", "index": i} for i in range(len(documents))]

    vectorstore = ValoriVectorStore(client, embeddings)
    ids = vectorstore.add_texts(documents, metadatas)
    print(f"   inserted {len(ids)} records: {ids}")

    print("\n4. Similarity search...")
    query = "What makes Valori deterministic?"
    results = vectorstore.similarity_search(query, k=3)
    print(f"\n   Query: '{query}'")
    for i, doc in enumerate(results, 1):
        print(f"   {i}. {doc.page_content[:80]}")
        print(f"      id={doc.metadata['id']}  score={doc.metadata['score']:.4f}")

    print("\n5. RAG chain (GPT-4o-mini)...")
    llm = ChatOpenAI(model="gpt-4o-mini", temperature=0)
    qa = RetrievalQA.from_chain_type(
        llm=llm,
        retriever=vectorstore.as_retriever(search_kwargs={"k": 3}),
    )
    for q in ["What is Valori?", "How does Valori ensure reproducibility?"]:
        print(f"\n   Q: {q}")
        print(f"   A: {qa.invoke(q)['result']}")

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
