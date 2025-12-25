"""
Complete LlamaIndex + Valori Example
=====================================

This example shows how to use Valori as a vector store in LlamaIndex applications.

Requirements:
    pip install llama-index llama-index-embeddings-openai llama-index-llms-openai valori

Usage:
    python examples/llamaindex_example.py
"""

import os
from llama_index.core import VectorStoreIndex, StorageContext, Document
from llama_index.core.node_parser import SentenceSplitter
from llama_index.embeddings.openai import OpenAIEmbedding
from llama_index.llms.openai import OpenAI

# Import Valori adapters
import sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'python'))

from valori.adapters import ValoriAdapter, LlamaIndexVectorStore


def main():
    print("=" * 60)
    print("LlamaIndex + Valori Chat Engine Example")
    print("=" * 60)
    
    # Step 1: Setup Valori adapter
    print("\n1. Setting up Valori connection...")
    adapter = ValoriAdapter(
        base_url="http://localhost:3000",
        api_key=os.getenv("VALORI_API_KEY"),
        max_retries=3,
    )
    print("✓ Connected to Valori node")
    
    # Step 2: Create Valori vector store
    print("\n2. Creating Valori vector store...")
    vector_store = LlamaIndexVectorStore(adapter=adapter)
    
    storage_context = StorageContext.from_defaults(
        vector_store=vector_store
    )
    print("✓ Vector store ready")
    
    # Step 3: Prepare documents
    print("\n3. Preparing knowledge base...")
    
    documents = [
        Document(
            text="""
            Valori is a deterministic vector database designed for reproducible AI systems.
            It uses Q16.16 fixed-point arithmetic instead of floating-point to ensure
            bit-identical results across different hardware architectures.
            """,
            metadata={"source": "intro", "category": "overview"}
        ),
        Document(
            text="""
            The database includes Write-Ahead Logging (WAL) for crash recovery and durability.
            Every operation is logged before being applied to memory, ensuring that the system
            can recover to an exact state after a crash.
            """,
            metadata={"source": "features", "category": "durability"}
        ),
        Document(
            text="""
            Valori is perfect for robotics, embedded AI, and safety-critical applications
            where reproducibility is essential. It works on x86, ARM, and WASM with
            identical state hashes, making it ideal for distributed robot fleets.
            """,
            metadata={"source": "use-cases", "category": "applications"}
        ),
        Document(
            text="""
            The kernel is written in Rust with a no_std core, allowing it to run on
            microcontrollers without an operating system. It provides cryptographic
            proofs of memory state for verification.
            """,
            metadata={"source": "technical", "category": "architecture"}
        ),
    ]
    
    print(f"✓ Prepared {len(documents)} documents")
    
    # Step 4: Create index with embeddings
    print("\n4. Building index with OpenAI embeddings...")
    
    embed_model = OpenAIEmbedding(
        model="text-embedding-3-small",
        api_key=os.getenv("OPENAI_API_KEY")
    )
    
    index = VectorStoreIndex.from_documents(
        documents,
        storage_context=storage_context,
        embed_model=embed_model,
        show_progress=True,
    )
    
    print("✓ Index built successfully")
    
    # Step 5: Query Engine
    print("\n" + "=" * 60)
    print("5. Testing Query Engine")
    print("=" * 60)
    
    llm = OpenAI(
        model="gpt-4",
        temperature=0,
        api_key=os.getenv("OPENAI_API_KEY")
    )
    
    query_engine = index.as_query_engine(
        llm=llm,
        similarity_top_k=3,
    )
    
    queries = [
        "What is Valori?",
        "How does Valori ensure deterministic behavior?",
        "What are the main use cases?",
    ]
    
    for query in queries:
        print(f"\n🔍 Query: {query}")
        response = query_engine.query(query)
        print(f"💡 Response: {response}")
        print(f"📚 Sources: {len(response.source_nodes)} nodes")
    
    # Step 6: Chat Engine
    print("\n" + "=" * 60)
    print("6. Interactive Chat Engine")
    print("=" * 60)
    
    chat_engine = index.as_chat_engine(
        llm=llm,
        chat_mode="condense_question",
        verbose=False,
    )
    
    print("\nStarting conversation...")
    
    # Conversation 1
    print("\n👤 User: Tell me about Valori")
    response = chat_engine.chat("Tell me about Valori")
    print(f"🤖 Assistant: {response}")
    
    # Conversation 2 (with context)
    print("\n👤 User: What makes it different from other databases?")
    response = chat_engine.chat("What makes it different from other databases?")
    print(f"🤖 Assistant: {response}")
    
    # Conversation 3 (follow-up)
    print("\n👤 User: Can you give me a specific example?")
    response = chat_engine.chat("Can you give me a specific example?")
    print(f"🤖 Assistant: {response}")
    
    # Step 7: Demonstrate streaming
    print("\n" + "=" * 60)
    print("7. Streaming Response")
    print("=" * 60)
    
    streaming_engine = index.as_query_engine(
        llm=llm,
        streaming=True,
    )
    
    print("\n🔍 Query: Explain WAL in Valori")
    print("🤖 Streaming Response: ", end="", flush=True)
    
    response = streaming_engine.query("Explain the WAL feature in detail")
    for token in response.response_gen:
        print(token, end="", flush=True)
    print()  # New line
    
    # Step 8: Retrieval with metadata filtering
    print("\n" + "=" * 60)
    print("8. Advanced: Metadata Filtering")
    print("=" * 60)
    
    retriever = index.as_retriever(
        similarity_top_k=2,
    )
    
    nodes = retriever.retrieve("deterministic systems")
    
    print(f"\n📄 Retrieved {len(nodes)} nodes:")
    for i, node in enumerate(nodes, 1):
        print(f"\n{i}. Score: {node.score:.4f}")
        print(f"   Category: {node.metadata.get('category')}")
        print(f"   Source: {node.metadata.get('source')}")
        print(f"   Text: {node.text[:100]}...")


if __name__ == "__main__":
    # Check environment
    if not os.getenv("OPENAI_API_KEY"):
        print("⚠️  Warning: OPENAI_API_KEY not set")
        print("   Set it with: export OPENAI_API_KEY=your-key")
        print("\n   Continuing anyway (some features may not work)...\n")
    
    try:
        main()
        print("\n" + "=" * 60)
        print("✅ Example completed successfully!")
        print("=" * 60)
        print("\n🎯 Key Takeaways:")
        print("  • Valori works seamlessly with LlamaIndex")
        print("  • Query, chat, and streaming all supported")
        print("  • Metadata filtering works out of the box")
        print("  • But with determinism guarantees!")
    except Exception as e:
        print(f"\n❌ Error: {e}")
        print("\nMake sure:")
        print("  1. Valori node is running (cargo run --release -p valori-node)")
        print("  2. OPENAI_API_KEY is set")
        print("  3. Dependencies installed:")
        print("     pip install llama-index llama-index-embeddings-openai llama-index-llms-openai")
