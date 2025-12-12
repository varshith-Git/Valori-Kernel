# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
Demo for Valori Adapters (LangChain & LlamaIndex)
Prerequisites:
    pip install numpy valori langchain llama-index-core
"""
import numpy as np

# Mock Embedder
def simple_embed(text: str) -> list[float]:
    np.random.seed(len(text))
    return np.random.uniform(-1, 1, 16).tolist()

def demo_langchain():
    print("\n--- LangChain Retriever Demo ---")
    from valori.adapters.base import ValoriAdapter
    from valori.adapters.langchain import ValoriRetriever
    
    # 1. Init Adapter
    # Note: Requires running Valori server!
    adapter = ValoriAdapter(base_url="http://localhost:3000", api_key="dev-key", embed_fn=simple_embed)
    
    # 2. Init Retriever
    retriever = ValoriRetriever(adapter, simple_embed, k=2)
    
    # 3. Use (This will fail locally if server not running, but shows API)
    print("Retriever initialized. Calling get_relevant_documents('hello')...")
    try:
        docs = retriever.get_relevant_documents("hello world")
        print(f"Got {len(docs)} docs")
    except Exception as e:
        print(f"Skipping actual call (Server offline?): {e}")

def demo_llamaindex():
    print("\n--- LlamaIndex VectorStore Demo ---")
    from valori.adapters.base import ValoriAdapter
    from valori.adapters.llamaindex import ValoriVectorStore
    try:
        from llama_index.core.schema import TextNode
    except ImportError:
        print("llama_index.core not installed, skipping.")
        return

    adapter = ValoriAdapter(base_url="http://localhost:3000", api_key="dev-key", embed_fn=simple_embed)
    store = ValoriVectorStore(adapter)
    
    node = TextNode(text="Valori is deterministic.", metadata={"category": "tech"})
    # Mock embedding because node.get_embedding needs setting
    node.embedding = simple_embed("Valori is deterministic.")
    
    print("VectorStore initialized. Adding node...")
    try:
        ids = store.add([node])
        print(f"Added nodes: {ids}")
    except Exception as e:
        print(f"Skipping actual call: {e}")

if __name__ == "__main__":
    demo_langchain()
    demo_llamaindex()
