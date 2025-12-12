# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import Optional
from .local import LocalClient
from .remote import RemoteClient

class Valori:
    def __new__(cls, remote: Optional[str] = None):
        """
        Factory yielding either a LocalClient (FFI) or RemoteClient (HTTP).
        
        Args:
            remote: If None (default), uses LocalClient (ffi). 
                    If a URL string, uses RemoteClient.
        """
        if remote is None:
            return LocalClient()
        else:
            return RemoteClient(base_url=remote)

from .memory import MemoryClient
from .protocol import ProtocolClient
from . import adapters

__all__ = ["Valori", "RemoteClient", "LocalClient", "MemoryClient", "ProtocolClient", "adapters"]
