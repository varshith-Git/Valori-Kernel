# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import Optional, Union
from .local import LocalClient
from .remote import SyncRemoteClient, AsyncRemoteClient

def Valoricore(
    remote: Optional[str] = None,
    path: str = "./valori_db",
    index_kind: str = "bruteforce",
) -> Union[LocalClient, SyncRemoteClient]:
    """
    Standard Synchronous Factory.
    - If remote is provided -> SyncRemoteClient
    - Else -> LocalClient
    """
    if remote:
        return SyncRemoteClient(base_url=remote)
    else:
        return LocalClient(path=path, index_kind=index_kind)

def AsyncValoricore(
    remote: Optional[str] = None,
    path: str = "./valori_db",
    index_kind: str = "bruteforce",
) -> Union[LocalClient, AsyncRemoteClient]:
    """
    Standard Asynchronous Factory.
    - If remote is provided -> AsyncRemoteClient
    - Else -> LocalClient
    """
    if remote:
        return AsyncRemoteClient(base_url=remote)
    else:
        return LocalClient(path=path, index_kind=index_kind)
