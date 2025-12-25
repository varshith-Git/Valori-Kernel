# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
Complete LangChain VectorStore adapter for Valori.

Drop-in replacement for FAISS, Chroma, Pinecone with deterministic guarantees.
"""

from typing import Any, Iterable, List, Optional, Tuple
import logging

try:
    from langchain_core.documents import Document
    from langchain_core.embeddings import Embeddings
    from langchain_core.vectorstores import VectorStore
except ImportError:
    # Fallback for dev environ without langchain
    class VectorStore: pass
    class Document:
        def __init__(self, page_content, metadata):
            self.page_content = page_content
            self.metadata = metadata
    class Embeddings: pass

from .base import ValoriAdapter

logger = logging.getLogger(__name__)


class ValoriVectorStore(VectorStore):
    """
    LangChain VectorStore backed by Valori's deterministic kernel.
    
    Features:
    - Bit-identical results across any hardware (x86, ARM, WASM)
    - Cryptographic proof of memory state via state hashes
    - Crash recovery via WAL + snapshots
    - Drop-in replacement for FAISS, Chroma, Pinecone
    
    Example:
        >>> from langchain_openai import OpenAIEmbeddings
        >>> from valori.adapters import ValoriAdapter, ValoriVectorStore
        >>> 
        >>> adapter = ValoriAdapter(base_url="http://localhost:3000")
        >>> embeddings = OpenAIEmbeddings()
        >>> 
        >>> vectorstore = ValoriVectorStore(
        ...     adapter=adapter,
        ...     embedding=embeddings
        ... )
        >>> 
        >>> # Add documents
        >>> vectorstore.add_texts(
        ...     texts=["Hello world", "Foo bar"],
        ...     metadatas=[{"source": "doc1"}, {"source": "doc2"}]
        ... )
        >>> 
        >>> # Similarity search
        >>> docs = vectorstore.similarity_search("Hello", k=1)
    """

    def __init__(
        self,
        adapter: ValoriAdapter,
        embedding: Embeddings,
    ):
        """
        Initialize Valori vector store.
        
        Args:
            adapter: ValoriAdapter instance (handles connection & retries)
            embedding: LangChain embeddings model
        """
        self._adapter = adapter
        self._embedding = embedding

    def add_texts(
        self,
        texts: Iterable[str],
        metadatas: Optional[List[dict]] = None,
        **kwargs: Any,
    ) -> List[str]:
        """
        Add texts to the vector store.
        
        Args:
            texts: Iterable of text strings
            metadatas: Optional metadata for each text
            **kwargs: Additional arguments (ignored)
            
        Returns:
            List of memory IDs assigned to each text
        """
        texts_list = list(texts)
        metadatas = metadatas or [{} for _ in texts_list]
        
        # Generate embeddings
        embeddings = self._embedding.embed_documents(texts_list)
        
        # Upsert to Valori
        ids = []
        for text, embedding, metadata in zip(texts_list, embeddings, metadatas):
            # Store text in metadata for retrieval
            full_metadata = {**metadata, "text": text}
            
            # Insert vector (adapter handles retries & validation)
            try:
                memory_id = self._adapter.upsert_vector(embedding, full_metadata)
                ids.append(memory_id)
            except Exception as e:
                logger.error(f"Failed to upsert text: {e}")
                # Append None for failed inserts
                ids.append(None)
        
        # Filter out None values
        return [id for id in ids if id is not None]

    def add_documents(
        self,
        documents: List[Document],
        **kwargs: Any,
    ) -> List[str]:
        """
        Add LangChain Documents (text + metadata).
        
        Args:
            documents: List of Document objects
            
        Returns:
            List of memory IDs
        """
        texts = [doc.page_content for doc in documents]
        metadatas = [doc.metadata for doc in documents]
        return self.add_texts(texts, metadatas, **kwargs)

    def similarity_search(
        self,
        query: str,
        k: int = 4,
        **kwargs: Any,
    ) -> List[Document]:
        """
        Search for similar documents.
        
        Args:
            query: Query text
            k: Number of results to return
            **kwargs: Additional arguments (ignored)
            
        Returns:
            List of Documents sorted by similarity
        """
        # Embed query
        query_embedding = self._embedding.embed_query(query)
        
        # Search Valori
        response = self._adapter.search_vector(query_embedding, top_k=k)
        
        # Extract results
        results = response.get("results", []) if isinstance(response, dict) else response
        
        # Convert to LangChain Documents
        documents = []
        for result in results:
            metadata = result.get("metadata", {})
            
            # Extract text from metadata
            text = metadata.pop("text", "")
            
            # Add distance/score to metadata
            metadata["distance"] = result.get("score", 0)
            metadata["memory_id"] = result.get("memory_id")
            
            documents.append(Document(page_content=text, metadata=metadata))
        
        return documents

    def similarity_search_with_score(
        self,
        query: str,
        k: int = 4,
        **kwargs: Any,
    ) -> List[Tuple[Document, float]]:
        """
        Search with distance scores.
        
        Args:
            query: Query text
            k: Number of results
            **kwargs: Additional arguments (ignored)
            
        Returns:
            List of (Document, distance) tuples
        """
        query_embedding = self._embedding.embed_query(query)
        response = self._adapter.search_vector(query_embedding, top_k=k)
        
        results = response.get("results", []) if isinstance(response, dict) else response
        
        docs_with_scores = []
        for result in results:
            metadata = result.get("metadata", {})
            distance = result.get("score", 0)
            
            text = metadata.pop("text", "")
            metadata["memory_id"] = result.get("memory_id")
            
            doc = Document(page_content=text, metadata=metadata)
            docs_with_scores.append((doc, float(distance)))
        
        return docs_with_scores

    @classmethod
    def from_texts(
        cls,
        texts: List[str],
        embedding: Embeddings,
        metadatas: Optional[List[dict]] = None,
        adapter: Optional[ValoriAdapter] = None,
        **kwargs: Any,
    ) -> "ValoriVectorStore":
        """
        Create a Valori vector store from texts.
        
        Args:
            texts: List of texts
            embedding: Embeddings model
            metadatas: Optional metadata
            adapter: ValoriAdapter instance (required)
            **kwargs: Additional arguments (ignored)
            
        Returns:
            ValoriVectorStore instance
        """
        if not adapter:
            raise ValueError("adapter parameter is required for ValoriVectorStore.from_texts()")
        
        store = cls(adapter=adapter, embedding=embedding)
        store.add_texts(texts, metadatas)
        return store

    @classmethod
    def from_documents(
        cls,
        documents: List[Document],
        embedding: Embeddings,
        adapter: Optional[ValoriAdapter] = None,
        **kwargs: Any,
    ) -> "ValoriVectorStore":
        """
        Create from LangChain Documents.
        
        Args:
            documents: List of Document objects
            embedding: Embeddings model
            adapter: ValoriAdapter instance (required)
            
        Returns:
            ValoriVectorStore instance
        """
        if not adapter:
            raise ValueError("adapter parameter is required")
        
        store = cls(adapter=adapter, embedding=embedding)
        store.add_documents(documents)
        return store


# Example usage
if __name__ == "__main__":
    print("ValoriVectorStore - LangChain Integration")
    print("=" * 50)
    print("\nExample:")
    print("""
from langchain_openai import OpenAIEmbeddings
from valori.adapters import ValoriAdapter, ValoriVectorStore

# Setup
adapter = ValoriAdapter(base_url="http://localhost:3000")
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

# Search
results = vectorstore.similarity_search("What is Valori?", k=2)

for doc in results:
    print(f"- {doc.page_content}")
    print(f"  Distance: {doc.metadata.get('distance')}")
    """)
