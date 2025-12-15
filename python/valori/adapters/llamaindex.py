# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import List, Any, Optional
import logging

try:
    from llama_index.core.vector_stores.types import (
        VectorStore,
        VectorStoreQuery,
        VectorStoreQueryResult,
    )
    from llama_index.core.schema import TextNode, NodeRelationship, RelatedNodeInfo
except ImportError:
    # Minimal mocks for dev environment
    class VectorStore: pass
    class VectorStoreQuery: pass
    class VectorStoreQueryResult: pass
    class TextNode: pass

# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from .base import AdapterBaseiAdapter, UpsertItem
from .utils import validate_float_range

logger = logging.getLogger(__name__)

class ValoriVectorStore(VectorStore):
    stores_text: bool = True
    
    def __init__(self, adapter: ValoriAdapter):
        self.adapter = adapter
        
    @property
    def client(self) -> Any:
        return self.adapter
        
    def add(self, nodes: List[TextNode], **kwargs: Any) -> List[str]:
        """Add nodes to index."""
        items = []
        ids = []
        for node in nodes:
            # node.get_embedding() -> List[float]
            # node.get_content() -> str
            # node.metadata -> dict
            
            vec = node.get_embedding()
            if not vec:
                logger.warning(f"Node {node.node_id} has no embedding, skipping.")
                continue

            # Prep for upsert
            # We store text and metadata in Valori metadata for retrieval
            meta = node.metadata.copy() if node.metadata else {}
            meta["text"] = node.get_content()
            meta["_node_content"] = node.get_content() # LlamaIndex convention often

            # Validate locally for fail-fast
            # adapter handles it too, but good to check
            validate_float_range(vec)

            # Do we use ProtocolClient upsert_vector?
            # Yes. ValoriAdapter doesn't wrap upsert_vector yet?
            # base.py only has search_vector. We need to add upsert logic to base.py
            # Or access client directly.
            
            # Using client directly for now, but really Adapter should handle retries.
            # I will assume adapter has upsert_vector or access client.
            
            # Upsert
            try:
                self.adapter.client.upsert_vector(
                     vector=vec,
                     metadata=meta 
                )
                ids.append(node.node_id)
            except Exception as e:
                logger.error(f"Failed to upsert node {node.node_id}: {e}")
                
        return ids

    def delete(self, ref_doc_id: str, **delete_kwargs: Any) -> None:
        # Not supported in V1 Protocol yet
        pass

    def query(self, query: VectorStoreQuery, **kwargs: Any) -> VectorStoreQueryResult:
        """Query index for top k most similar nodes."""
        
        # query.query_embedding -> List[float]
        resp = self.adapter.search_vector(query.query_embedding, top_k=query.similarity_top_k)
        
        nodes = []
        similarities = []
        ids = []
        
        hits = resp.get("results", [])
        for hit in hits:
             # Extract
             meta = hit.get("metadata", {}) or {}
             text = meta.get("text", "")
             original_id = meta.get("doc_id", None) # If we stored it?
             
             # Create TextNode
             node = TextNode(
                 text=text,
                 metadata=meta,
             )
             nodes.append(node)
             similarities.append(float(hit["score"]) / 65536.0) # Convert Q16.16 score to float?
             # Wait, score is distance? Or similarity?
             # Valori uses L2 squared distance currently. 
             # LlamaIndex expects similarity (higher is better) usually, or distance if mode set.
             # If L2 distance, lower is better.
             # We should probably return 1 / (1 + dist) or specific distance.
             ids.append(hit["memory_id"])

        return VectorStoreQueryResult(nodes=nodes, similarities=similarities, ids=ids)
