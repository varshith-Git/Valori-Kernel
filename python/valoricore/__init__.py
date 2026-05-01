# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from .local import LocalClient
from .remote import SyncRemoteClient, AsyncRemoteClient
from .memory import MemoryClient
from .async_memory import AsyncMemoryClient
from .valoricore_ffi import ingest_embedding, generate_proof, verify_embedding
from .adapter import ValoricoreAdapter
from .exceptions import ValoricoreError, IntegrityError, ValidationError, ConnectionError, NotFoundError
from .factory import Valoricore, AsyncValoricore

__all__ = [
    "Valoricore", "AsyncValoricore",
    "SyncRemoteClient", "AsyncRemoteClient", "LocalClient",
    "MemoryClient", "AsyncMemoryClient",
    "ingest_embedding", "generate_proof", "verify_embedding",
    "ValoricoreAdapter",
    "ValoricoreError", "IntegrityError", "ValidationError", "ConnectionError", "NotFoundError"
]
