# Valori AI Framework Adapters - Status & Improvements

## Current Implementation Status

### ✅ What's Working

1. **Base Infrastructure** (`adapters/base.py`)
   - `ValoriAdapter` with retry logic
   - Connection to ProtocolRemoteClient
   - Basic search_vector method

2. **LangChain** (`adapters/langchain.py`)
   - `ValoriRetriever` implements BaseRetriever
   - get_relevant_documents() method
   - Converts Valori search results to LangChain Documents

3. **LlamaIndex** (`adapters/llamaindex.py`)
   - `ValoriVectorStore` implements VectorStore
   - add() and query() methods
   - TextNode support

### ⚠️ Issues Found

#### 1. LlamaIndex Import Error (Line 20)
```python
from .base import AdapterBaseiAdapter, UpsertItem  # Typo!
```
**Fix**: Should be `ValoriAdapter`

#### 2. Missing Methods in Base Adapter
- No `upsert_vector()` wrapper
- No `upsert_document()` helper
- LlamaIndex adapter accesses `.client` directly (line 69)

#### 3. Incomplete LangChain Integration
- Only has Retriever, missing VectorStore
- No add_documents() method
- Can't be used as drop-in replacement for FAISS/Chroma

---

## Recommended Improvements

### 1. Fix Base Adapter

Add missing methods to `adapters/base.py`:

```python
def upsert_vector(
    self,
    vector: List[float],
    metadata: Optional[Dict[str, Any]] = None
) -> str:
    """
    Upsert a vector with metadata.
    
    Returns:
        memory_id assigned by Valori
    """
    validated = validate_float_range(vector)
    return self._retry(lambda: self.client.upsert_vector(
        vector=validated,
        metadata=metadata or {}
    ))

def upsert_document(
    self,
    text: str,
    metadata: Optional[Dict[str, Any]] = None,
    embedding: Optional[List[float]] = None
) -> str:
    """
    Upsert a text document.
    
    If embedding not provided, uses embed_fn from client.
    """
    if not embedding:
        if not self.client.embed_fn:
            raise ValueError("No embedding function configured")
        embedding = self.client.embed_fn(text)
    
    full_metadata = metadata.copy() if metadata else {}
    full_metadata["text"] = text
    
    return self.upsert_vector(embedding, full_metadata)
```

---

### 2. Fix LlamaIndex Adapter

**File**: `adapters/llamaindex.py`

**Line 20 Fix**:
```python
from .base import ValoriAdapter, UpsertItem  # Fixed typo
```

**Line 69-72 Enhancement** (use adapter method instead of direct client access):
```python
try:
    memory_id = self.adapter.upsert_vector(
        vector=vec,
        metadata=meta
    )
    ids.append(node.node_id)
except Exception as e:
    logger.error(f"Failed to upsert node {node.node_id}: {e}")
```

---

### 3. Complete LangChain Adapter

**Current**: Only has Retriever  
**Need**: Full VectorStore implementation

**Create**: `adapters/langchain_vectorstore.py`

```python
from typing import List, Optional, Any, Iterable
from langchain_core.vectorstores import VectorStore
from langchain_core.documents import Document
from langchain_core.embeddings import Embeddings

from .base import ValoriAdapter

class ValoriVectorStore(VectorStore):
    """
    LangChain VectorStore backed by Valori.
    
    Drop-in replacement for FAISS, Chroma, Pinecone, etc.
    with deterministic guarantees.
    """
    
    def __init__(
        self,
        adapter: ValoriAdapter,
        embedding: Embeddings,
    ):
        self._adapter = adapter
        self._embedding = embedding
    
    def add_texts(
        self,
        texts: Iterable[str],
        metadatas: Optional[List[dict]] = None,
        **kwargs: Any,
    ) -> List[str]:
        """Add texts to vector store."""
        texts_list = list(texts)
        metadatas = metadatas or [{} for _ in texts_list]
        
        # Embed all texts
        embeddings = self._embedding.embed_documents(texts_list)
        
        # Upsert to Valori
        ids = []
        for text, embedding, metadata in zip(texts_list, embeddings, metadatas):
            full_meta = {**metadata, "text": text}
            memory_id = self._adapter.upsert_vector(embedding, full_meta)
            ids.append(memory_id)
        
        return ids
    
    def similarity_search(
        self,
        query: str,
        k: int = 4,
        **kwargs: Any,
    ) -> List[Document]:
        """Search for similar documents."""
        query_embedding = self._embedding.embed_query(query)
        
        results = self._adapter.search_vector(query_embedding, top_k=k)
        
        documents = []
        for result in results.get("results", []):
            metadata = result.get("metadata", {})
            text = metadata.pop("text", "")
            documents.append(Document(
                page_content=text,
                metadata=metadata
            ))
        
        return documents
    
    @classmethod
    def from_texts(
        cls,
        texts: List[str],
        embedding: Embeddings,
        metadatas: Optional[List[dict]] = None,
        adapter: Optional[ValoriAdapter] = None,
        **kwargs: Any,
    ) -> "ValoriVectorStore":
        """Create from texts."""
        if not adapter:
            raise ValueError("adapter required")
        
        store = cls(adapter=adapter, embedding=embedding)
        store.add_texts(texts, metadatas)
        return store
```

---

## Usage Examples

### LangChain - RAG Application

```python
from langchain_openai import OpenAIEmbeddings, ChatOpenAI
from langchain.chains import RetrievalQA
from valori.adapters.base import ValoriAdapter
from valori.adapters.langchain_vectorstore import ValoriVectorStore

# Setup
adapter = ValoriAdapter(
    base_url="http://localhost:3000",
    embed_fn=None,  # Use LangChain embeddings
)

embeddings = OpenAIEmbeddings()

# Create vector store
vectorstore = ValoriVectorStore(
    adapter=adapter,
    embedding=embeddings
)

# Add documents
texts = [
    "Valori is a deterministic vector database",
    "It uses fixed-point arithmetic for reproducibility",
    "Perfect for robotics and embedded AI",
]

vectorstore.add_texts(texts)

# Create RAG chain
llm = ChatOpenAI(model="gpt-4")
qa_chain = RetrievalQA.from_chain_type(
    llm=llm,
    retriever=vectorstore.as_retriever(),
    return_source_documents=True
)

# Query
result = qa_chain({"query": "What is Valori?"})
print(result["result"])
```

---

### LlamaIndex - Chat Engine

```python
from llama_index.core import VectorStoreIndex, StorageContext
from llama_index.embeddings.openai import OpenAIEmbedding
from llama_index.llms.openai import OpenAI
from valori.adapters.base import ValoriAdapter
from valori.adapters.llamaindex import ValoriVectorStore

# Setup
adapter = ValoriAdapter(base_url="http://localhost:3000")

vector_store = ValoriVectorStore(adapter=adapter)

# Create index
storage_context = StorageContext.from_defaults(
    vector_store=vector_store
)

index = VectorStoreIndex.from_documents(
    documents,  # Your documents
    storage_context=storage_context,
    embed_model=OpenAIEmbedding(),
)

# Chat
chat_engine = index.as_chat_engine(
    llm=OpenAI(model="gpt-4")
)

response = chat_engine.chat("What is Valori?")
print(response)
```

---

## Why This Matters

### Before (Without Adapters):
- Users need to learn Valori-specific APIs
- Can't reuse existing LangChain/LlamaIndex code
- Limited to early adopters

### After (With Adapters):
✅ **Drop-in replacement** for existing vector stores  
✅ **Instant compatibility** with 1000s of LangChain apps  
✅ **LlamaIndex integration** for chat/agents  
✅ **Determinism** as a feature, not a barrier  

---

## Next Steps

1. **Fix Typo** in `llamaindex.py` line 20
2. **Add Methods** to `base.py` (upsert_vector, upsert_document)
3. **Create** `langchain_vectorstore.py` for full LangChain VectorStore
4. **Test** with real LangChain/LlamaIndex apps
5. **Document** in README with "LangChain Compatible" badge

---

## Marketing Value

**Current Pitch**: "Deterministic vector database"  
**New Pitch**: "LangChain-compatible deterministic vector store"

This makes Valori **immediately usable** by the massive LangChain/LlamaIndex community!

Would you like me to implement these fixes?
