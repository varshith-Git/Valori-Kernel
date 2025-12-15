# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import List, Callable, Optional, Any

try:
    from sentence_transformers import SentenceTransformer
except ImportError:
    SentenceTransformer = None

class SentenceTransformerAdapter:
    """
    Adapter for using 'sentence_transformers' models with Valori.
    
    Installation:
        pip install sentence-transformers
        
    Usage:
        adapter = SentenceTransformerAdapter("all-MiniLM-L6-v2")
        client = ProtocolClient(embed=adapter.embed, ...)
    """
    def __init__(self, model_name: str = "all-MiniLM-L6-v2", device: Optional[str] = None, output_dim: Optional[int] = None):
        if SentenceTransformer is None:
            raise ImportError("sentence-transformers is not installed. Run `pip install sentence-transformers`.")
            
        self.model = SentenceTransformer(model_name, device=device)
        self.model_name = model_name
        self.output_dim = output_dim

    def embed(self, text: str) -> List[float]:
        """
        Embeds a single string into a list of floats.
        Valori expects a list of floats.
        """
        # encode returns a numpy array, we need to convert to list[float]
        embedding = self.model.encode(text, convert_to_numpy=True)
        as_list = embedding.tolist()
        
        if self.output_dim:
            return as_list[:self.output_dim]
        return as_list

    def embed_batch(self, texts: List[str]) -> List[List[float]]:
        """
        Efficient batch embedding.
        """
        embeddings = self.model.encode(texts, convert_to_numpy=True)
        if self.output_dim:
            # Slicing numpy array is faster
            return embeddings[:, :self.output_dim].tolist()
        return embeddings.tolist()
