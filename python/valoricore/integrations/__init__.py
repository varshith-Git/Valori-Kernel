# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
valoricore.integrations
=======================

Native framework adapters for Valori-Kernel.  Import the class you need — the
import itself is always safe and cheap.  The actual framework (LangChain or
LlamaIndex) is loaded lazily; an ``ImportError`` with install instructions is
raised only when you instantiate the class without the dependency present.

Available adapters
------------------

ValoricoreLangChain
    Full LangChain VectorStore + Retriever.  Works as a drop-in replacement
    for FAISS, Chroma, or Pinecone — same ``add_texts / similarity_search /
    from_documents / as_retriever`` interface.

ValoricoreRetriever
    LangChain BaseRetriever returned by ``ValoricoreLangChain.as_retriever()``.
    Plugs directly into RetrievalQA, ConversationalRetrievalChain, agents, etc.

ValoricoreLlamaIndex
    Full LlamaIndex VectorStore (BasePydanticVectorStore ≥ 0.10 or legacy
    VectorStore 0.8–0.9).  Works with ``StorageContext``,
    ``VectorStoreIndex``, and ``as_query_engine()``.

Quick examples
--------------

LangChain — local embedded (no server needed)::

    from valoricore.integrations import ValoricoreLangChain
    from langchain_openai import OpenAIEmbeddings

    store = ValoricoreLangChain(path="./db", embedding=OpenAIEmbeddings())
    store.add_texts(["Valoricore is deterministic.", "Fixed-point is the key."])
    docs = store.similarity_search("What makes it deterministic?", k=3)

LangChain — remote HTTP node::

    store = ValoricoreLangChain(remote="http://my-node:3000", embedding=OpenAIEmbeddings())

LangChain — from documents (standard factory pattern)::

    from langchain.document_loaders import PyPDFLoader

    docs  = PyPDFLoader("report.pdf").load()
    store = ValoricoreLangChain.from_documents(docs, OpenAIEmbeddings(), path="./db")

LangChain — as retriever in a RAG chain::

    from langchain.chains import RetrievalQA
    from langchain_openai import ChatOpenAI

    chain = RetrievalQA.from_chain_type(
        llm       = ChatOpenAI(),
        retriever = store.as_retriever(k=5),
    )
    answer = chain.run("What is deterministic AI memory?")

LlamaIndex — local embedded::

    from llama_index.core import VectorStoreIndex, StorageContext
    from llama_index.embeddings.openai import OpenAIEmbedding
    from valoricore.integrations import ValoricoreLlamaIndex

    vector_store = ValoricoreLlamaIndex(path="./db")
    storage_ctx  = StorageContext.from_defaults(vector_store=vector_store)
    index        = VectorStoreIndex.from_documents(
        documents,
        storage_context = storage_ctx,
        embed_model     = OpenAIEmbedding(),
    )
    response = index.as_query_engine().query("What is deterministic memory?")

Cryptographic audit hash (available on both adapters)::

    print(store.get_state_hash())   # 64-char BLAKE3 hex — deterministic on any machine
"""

from .langchain  import ValoricoreLangChain, ValoricoreRetriever
from .llamaindex import ValoricoreLlamaIndex

__all__ = [
    "ValoricoreLangChain",
    "ValoricoreRetriever",
    "ValoricoreLlamaIndex",
]
