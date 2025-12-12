# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import List, Any, Optional
try:
    from langchain.schema import BaseRetriever, Document
except ImportError:
    # Fallback for dev environ without langchain
    class BaseRetriever: pass
    class Document: 
        def __init__(self, page_content, metadata): 
            self.page_content = page_content
            self.metadata = metadata

from .base import ValoriAdapter
from .utils import validate_float_range

class ValoriRetriever(BaseRetriever):
    def __init__(self, adapter: ValoriAdapter, embed_fn, k: int = 4):
        self.adapter = adapter
        self.embed_fn = embed_fn
        self.k = k

    def get_relevant_documents(self, query: str) -> List[Document]:
        """
        Retrieve documents relevant to a query.
        """
        # 1. Embed
        emb = self.embed_fn(query)
        
        # 2. Search via ValoriAdapter (handles FXP validation and retries)
        resp = self.adapter.search_vector(emb, top_k=self.k)
        
        # 3. Map to Documents
        docs = []
        if isinstance(resp, dict): 
             hits = resp.get("results", [])
        elif isinstance(resp, list):
             hits = resp
        else:
             hits = []

        for hit in hits:
             # Hit structure: {memory_id, record_id, score, metadata}
             # Metadata contains "text" if upserted via upsert_document or custom metadata.
             meta = hit.get("metadata", {}) or {}
             text_content = meta.get("text", "") 
             
             # If text is empty, maybe it's in a chunk node? 
             # Current Valori Node doesn't automatically join chunks yet in V1 search.
             # But the prompt says "Adapter should chunk text... upsert chunks".
             # So we should store the chunk text in metadata when we implement upsert_document.
             
             docs.append(Document(page_content=text_content, metadata=meta))

        return docs
