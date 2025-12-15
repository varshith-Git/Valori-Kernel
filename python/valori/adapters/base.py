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
