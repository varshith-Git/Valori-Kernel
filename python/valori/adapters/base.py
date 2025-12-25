# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import time
import uuid
import logging
# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import List, Dict, Any, Optional
from dataclasses import dataclass

from ..protocol import ProtocolClient, ProtocolRemoteClient
from .utils import validate_float_range

logger = logging.getLogger("valori.adapter")

@dataclass
class UpsertItem:
    text: str
    metadata: Dict[str, Any]
    id: Optional[str] = None
    embedding: Optional[List[float]] = None

class ValoriAdapter:
    def __init__(
        self, 
        base_url: str, 
        api_key: Optional[str] = None, 
        embed_fn = None,
        timeout: int = 30,
        max_retries: int = 5,
        dev_mode: bool = False
    ):
        self.client = ProtocolRemoteClient(base_url, embed_fn, expected_dim=16, api_key=api_key)
        self.max_retries = max_retries
        self.timeout = timeout
        
        if not api_key and not dev_mode:
            logger.warning("ValoriAdapter initialized without API Key in production mode (dev_mode=False). Requests may fail.")

    def search_vector(self, query_vec: List[float], top_k: int = 4):
        # Validate
        qs = validate_float_range(query_vec)
        # ProtocolRemoteClient.search_vector logic
        # But search_vector in ProtocolRemoteClient expects list[float].
        # utils returns list[float].
        
        return self._retry(lambda: self.client.search_vector(qs, k=top_k))

    def upsert_vector(
        self,
        vector: List[float],
        metadata: Optional[Dict[str, Any]] = None
    ) -> str:
        """
        Upsert a vector with metadata.
        
        Args:
            vector: Embedding vector
            metadata: Optional metadata dict
            
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
        Upsert a text document with automatic embedding.
        
        Args:
            text: Document text
            metadata: Optional metadata
            embedding: Optional pre-computed embedding (uses embed_fn if not provided)
            
        Returns:
            memory_id assigned by Valori
        """
        if not embedding:
            if not self.client.embed_fn:
                raise ValueError("No embedding function configured and no embedding provided")
            embedding = self.client.embed_fn(text)
        
        full_metadata = metadata.copy() if metadata else {}
        full_metadata["text"] = text
        
        return self.upsert_vector(embedding, full_metadata)

    def _retry(self, func):
        attempts = 0
        while attempts < self.max_retries:
            try:
                return func()
            except Exception as e:
                attempts += 1
                if attempts >= self.max_retries:
                    raise e
                sleep_ms = min(100 * (2 ** (attempts - 1)), 800)
                time.sleep(sleep_ms / 1000.0)
