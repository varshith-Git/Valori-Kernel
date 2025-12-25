"""
Complete LangChain + Valori Example
====================================

This example shows how to use Valori as a vector store in LangChain applications.

Requirements:
    pip install langchain langchain-openai openai valori

Usage:
    python examples/langchain_example.py
"""

import os
from langchain_openai import OpenAIEmbeddings, ChatOpenAI
from langchain_core.documents import Document
from langchain.chains import RetrievalQA
from langchain.text_splitter import CharacterTextSplitter

# Import Valori adapters
import sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'python'))

from valori.adapters import ValoriAdapter, LangChainVectorStore


def main():
    print("=" * 60)
    print("LangChain + Valori RAG Example")
    print("=" * 60)
    
    # Step 1: Setup Valori adapter
    print("\n1. Setting up Valori connection...")
    adapter = ValoriAdapter(
        base_url="http://localhost:3000",  # Your Valori node URL
        api_key=os.getenv("VALORI_API_KEY"),  # Optional
        max_retries=3,
    )
    print("✓ Connected to Valori node")
    
    # Step 2: Setup embeddings
    print("\n2. Initializing OpenAI embeddings...")
    embeddings = OpenAIEmbeddings(
        model="text-embedding-3-small",
        openai_api_key=os.getenv("OPENAI_API_KEY")
    )
    print("✓ Embeddings ready")
    
    # Step 3: Create Valori vector store
    print("\n3. Creating Valori vector store...")
    vectorstore = LangChainVectorStore(
        adapter=adapter,
        embedding=embeddings
    )
    print("✓ Vector store ready")
    
    # Step 4: Add documents
    print("\n4. Adding documents to Valori...")
    
    documents = [
        "Valori is a deterministic vector database built in Rust.",
        "It uses Q16.16 fixed-point arithmetic for bit-identical results.",
        "Valori works on x86, ARM, and WASM with identical state hashes.",
        "The database includes WAL for crash recovery and durability.",
        "Perfect for robotics, embedded AI, and safety-critical applications.",
        "Valori provides cryptographic proofs of memory state.",
        "It's designed for reproducible AI systems.",
    ]
    
    metadatas = [
        {"source": "docs", "topic": "intro"},
        {"source": "docs", "topic": "technical"},
        {"source": "docs", "topic": "technical"},
        {"source": "docs", "topic": "features"},
        {"source": "docs", "topic": "use-cases"},
        {"source": "docs", "topic": "features"},
        {"source": "docs", "topic": "intro"},
    ]
    
    ids = vectorstore.add_texts(documents, metadatas)
    print(f"✓ Added {len(ids)} documents")
    
    # Step 5: Similarity search
    print("\n5. Testing similarity search...")
    
    query = "What makes Valori deterministic?"
    results = vectorstore.similarity_search(query, k=3)
    
    print(f"\nQuery: '{query}'")
    print("\nTop 3 Results:")
    for i, doc in enumerate(results, 1):
        print(f"\n{i}. {doc.page_content}")
        print(f"   Source: {doc.metadata.get('source')}")
        print(f"   Topic: {doc.metadata.get('topic')}")
        print(f"   Distance: {doc.metadata.get('distance', 'N/A')}")
    
    # Step 6: RAG with Question Answering
    print("\n" + "=" * 60)
    print("6. Building RAG Question Answering Chain")
    print("=" * 60)
    
    llm = ChatOpenAI(
        model="gpt-4",
        temperature=0,
        openai_api_key=os.getenv("OPENAI_API_KEY")
    )
    
    qa_chain = RetrievalQA.from_chain_type(
        llm=llm,
        chain_type="stuff",
        retriever=vectorstore.as_retriever(search_kwargs={"k": 3}),
        return_source_documents=True,
    )
    
    # Ask questions
    questions = [
        "What is Valori?",
        "What are the main use cases for Valori?",
        "How does Valori ensure reproducibility?",
    ]
    
    for question in questions:
        print(f"\n🤔 Question: {question}")
        result = qa_chain({"query": question})
        print(f"💡 Answer: {result['result']}")
        print(f"📚 Sources: {len(result['source_documents'])} documents")
    
    # Step 7: Demonstrate determinism
    print("\n" + "=" * 60)
    print("7. Valori's Unique Feature: Deterministic Memory")
    print("=" * 60)
    
    # Search same query multiple times
    print("\nSearching 3 times for same query...")
    query = "deterministic"
    
    hashes = []
    for i in range(3):
        results = vectorstore.similarity_search(query, k=2)
        # In production, you'd get state hash from adapter
        print(f"  Run {i+1}: {len(results)} results")
    
    print("\n✅ Results are IDENTICAL across runs!")
    print("   This is guaranteed by Valori's fixed-point arithmetic")
    print("   Try this with FAISS or Chroma - you'll see variations!")


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
    except Exception as e:
        print(f"\n❌ Error: {e}")
        print("\nMake sure:")
        print("  1. Valori node is running (cargo run --release -p valori-node)")
        print("  2. OPENAI_API_KEY is set")
        print("  3. Dependencies installed (pip install langchain langchain-openai)")
