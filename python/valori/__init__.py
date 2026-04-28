# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import Optional
from .local import LocalClient
from .remote import RemoteClient

class Valori:
    def __new__(cls, remote: Optional[str] = None, path: str = "./valori_db"):
        """
        Factory yielding either a LocalClient (FFI) or RemoteClient (HTTP).
        
        Args:
            remote: If None (default), uses LocalClient (ffi). 
                    If a URL string, uses RemoteClient.
            path: Path to database directory (only used for LocalClient).
        """
        if remote is None:
            return LocalClient(path=path)
        else:
            return RemoteClient(base_url=remote)

from .memory import MemoryClient
from .protocol import ProtocolClient
from . import adapters

# Bridge functions — deterministic proof generation (all logic in Rust)
from .valori_ffi import (
    ingest_embedding,    # Vec<f32> → Vec<i32> (Q16.16)
    generate_proof,      # Vec<i32> → str (BLAKE3 Merkle root hex)
    verify_embedding,    # Vec<f32> + str → bool
)
from .adapter import ValoriAdapter

__all__ = [
    "Valori", "RemoteClient", "LocalClient", "MemoryClient", "ProtocolClient", "adapters",
    "ingest_embedding", "generate_proof", "verify_embedding", "ValoriAdapter",
]
