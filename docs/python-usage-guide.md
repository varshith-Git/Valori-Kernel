# Using Valori in Your Python Project

Complete guide to integrating Valori with LangChain and LlamaIndex.

---

## 🚀 Quick Start

### 1. Install Dependencies

```bash
# For LangChain
pip install langchain langchain-openai openai

# For LlamaIndex
pip install llama-index llama-index-embeddings-openai llama-index-llms-openai

# Install Valori (when published)
pip install valori
```

### 2. Start Valori Node

```bash
cd Valori-Kernel
cargo run --release -p valori-node
```

Node will start on `http://localhost:3000`

---

## 📚 LangChain Integration

### Basic Usage

```python
from langchain_openai import OpenAIEmbeddings
from valori.adapters import ValoriAdapter, LangChainVectorStore

# Setup
adapter = ValoriAdapter(base_url="http://localhost:3000")
vectorstore = LangChainVectorStore(
    adapter=adapter,
    embedding=OpenAIEmbeddings()
)

# Add documents
vectorstore.add_texts([
    "Valori is deterministic",
    "It uses fixed-point math",
])

# Search
docs = vectorstore.similarity_search("deterministic", k=2)
```

### RAG (Question Answering)

```python
from langchain_openai import ChatOpenAI
from langchain.chains import RetrievalQA

qa_chain = RetrievalQA.from_chain_type(
    llm=ChatOpenAI(model="gpt-4"),
    retriever=vectorstore.as_retriever(),
)

result = qa_chain({"query": "What is Valori?"})
print(result["result"])
```

### Full Example

See: [`examples/langchain_example.py`](../examples/langchain_example.py)

Run with:
```bash
export OPENAI_API_KEY=your-key
python examples/langchain_example.py
```

---

## 🦙 LlamaIndex Integration

### Basic Usage

```python
from llama_index.core import VectorStoreIndex, StorageContext, Document
from llama_index.embeddings.openai import OpenAIEmbedding
from valori.adapters import ValoriAdapter, LlamaIndexVectorStore

# Setup
adapter = ValoriAdapter(base_url="http://localhost:3000")
vector_store = LlamaIndexVectorStore(adapter=adapter)

storage_context = StorageContext.from_defaults(vector_store=vector_store)

# Create index
documents = [Document(text="Valori is amazing")]
index = VectorStoreIndex.from_documents(
    documents,
    storage_context=storage_context,
    embed_model=OpenAIEmbedding()
)

# Query
query_engine = index.as_query_engine()
response = query_engine.query("Tell me about Valori")
```

### Chat Engine

```python
from llama_index.llms.openai import OpenAI

chat_engine = index.as_chat_engine(llm=OpenAI(model="gpt-4"))

# Interactive conversation
response1 = chat_engine.chat("What is Valori?")
response2 = chat_engine.chat("Tell me more")  # Keeps context!
```

### Full Example

See: [`examples/llamaindex_example.py`](../examples/llamaindex_example.py)

Run with:
```bash
export OPENAI_API_KEY=your-key
python examples/llamaindex_example.py
```

---

## 🎯 Use Cases

### 1. **Deterministic RAG for Robotics**

```python
# Robot A (ARM Cortex-M)
vectorstore.add_texts(mission_logs)
state_hash_a = adapter.get_state_hash()

# Cloud (x86)
vectorstore_cloud.restore_from_hash(state_hash_a)
# Identical retrieval results!

# Robot B (ARM Cortex-M7)
# Can verify and share same memory
```

### 2. **Reproducible Research**

```python
# Experiment on Monday
vectorstore.add_texts(research_papers)
results_monday = vectorstore.similarity_search("quantum", k=5)

# Exact same results on Friday
results_friday = vectorstore.similarity_search("quantum", k=5)

assert results_monday == results_friday  # ✅ Always true!
```

### 3. **Safety-Critical AI**

```python
# Medical AI - results must be reproducible
vectorstore.add_texts(medical_knowledge)

# Query for diagnosis
results = vectorstore.similarity_search(symptoms, k=10)

# Generate cryptographic proof
proof_hash = adapter.get_state_hash()
# Auditors can verify EXACT same results
```

---

## 🔄 Migration Guide

### From FAISS

```python
# Before
from langchain_community.vectorstores import FAISS
vectorstore = FAISS.from_texts(texts, embeddings)

# After (just 2 line changes!)
from valori.adapters import ValoriAdapter, LangChainVectorStore
adapter = ValoriAdapter(base_url="http://localhost:3000")
vectorstore = LangChainVectorStore.from_texts(
    texts, embeddings, adapter=adapter
)
```

### From Chroma

```python
# Before
from langchain_community.vectorstores import Chroma
vectorstore = Chroma.from_texts(texts, embeddings)

# After
from valori.adapters import ValoriAdapter, LangChainVectorStore
adapter = ValoriAdapter(base_url="http://localhost:3000")
vectorstore = LangChainVectorStore.from_texts(
    texts, embeddings, adapter=adapter
)
```

### From Pinecone

```python
# Before
from langchain_community.vectorstores import Pinecone
vectorstore = Pinecone.from_texts(texts, embeddings, index_name="my-index")

# After (no index name needed!)
from valori.adapters import ValoriAdapter, LangChainVectorStore
adapter = ValoriAdapter(base_url="http://localhost:3000")
vectorstore = LangChainVectorStore.from_texts(
    texts, embeddings, adapter=adapter
)
```

---

## ⚙️ Configuration

### Remote Mode (HTTP Server)

```python
adapter = ValoriAdapter(
    base_url="http://production-server:3000",
    api_key="your-secret-key",
    max_retries=5,
    timeout=30,
)
```

### Embedded Mode (Direct, no server)

```python
from valori import EmbeddedKernel

# Coming soon - direct kernel access!
kernel = EmbeddedKernel(max_records=10000, dim=1536)
```

---

## 🎁 Why Use Valori?

| Feature | FAISS | Chroma | Pinecone | **Valori** |
|---------|-------|--------|----------|------------|
| **LangChain Support** | ✅ | ✅ | ✅ | ✅ |
| **LlamaIndex Support** | ✅ | ✅ | ✅ | ✅ |
| **Deterministic** | ❌ | ❌ | ❌ | **✅** |
| **Crash Recovery** | ❌ | Partial | ✅ | **✅** |
| **Cross-Arch Identical** | ❌ | ❌ | ❌ | **✅** |
| **Embedded Support** | ❌ | ❌ | ❌ | **✅** |
| **Cryptographic Proofs** | ❌ | ❌ | ❌ | **✅** |
| **Self-Hosted** | ✅ | ✅ | ❌ | **✅** |

---

## 📖 API Reference

### ValoriAdapter

```python
adapter = ValoriAdapter(
    base_url: str,              # Valori node URL
    api_key: Optional[str],     # Optional auth token
    embed_fn: Optional[Callable],  # Custom embedding function
    timeout: int = 30,          # Request timeout
    max_retries: int = 5,       # Retry count
)

# Methods
adapter.search_vector(vector, top_k=4)
adapter.upsert_vector(vector, metadata=None)
adapter.upsert_document(text, metadata=None, embedding=None)
```

### LangChainVectorStore

```python
vectorstore = LangChainVectorStore(
    adapter: ValoriAdapter,
    embedding: Embeddings,
)

# Methods (standard LangChain API)
vectorstore.add_texts(texts, metadatas=None)
vectorstore.add_documents(documents)
vectorstore.similarity_search(query, k=4)
vectorstore.similarity_search_with_score(query, k=4)
vectorstore.as_retriever()

# Class methods
LangChainVectorStore.from_texts(texts, embedding, adapter)
LangChainVectorStore.from_documents(documents, embedding, adapter)
```

### LlamaIndexVectorStore

```python
vector_store = LlamaIndexVectorStore(adapter: ValoriAdapter)

# Methods (standard LlamaIndex API)
vector_store.add(nodes)
vector_store.query(query)
vector_store.delete(ref_doc_id)
```

---

## 🐛 Troubleshooting

### "Connection refused"
- Make sure Valori node is running: `cargo run --release -p valori-node`
- Check URL is correct: `http://localhost:3000`

### "No embedding function"
- Pass `embedding` parameter to VectorStore
- Or set `embed_fn` in ValoriAdapter

### "Import Error"
- Install dependencies: `pip install langchain langchain-openai`
- Check Python path includes valori package

---

## 📚 Learn More

- [LangChain Example](../examples/langchain_example.py) - Complete RAG demo
- [LlamaIndex Example](../examples/llamaindex_example.py) - Chat engine demo
- [Valori Architecture](../architecture.md) - How it works
- [WAL Guarantees](../docs/wal-replay-guarantees.md) - Crash recovery

---

**Ready to build reproducible AI? Start with the examples!** 🚀
