# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import Optional, Union
from .local import LocalClient
from .remote import SyncRemoteClient, AsyncRemoteClient

def Valoricore(
    remote: Optional[str] = None,
    path: str = "./valori_db",
    index_kind: str = "bruteforce",
    max_records: int = 0,
    dim: int = 0,
    max_nodes: int = 0,
    max_edges: int = 0,
) -> Union[LocalClient, SyncRemoteClient]:
    """
    Standard Synchronous Factory.
    - If remote is provided -> SyncRemoteClient (capacity args ignored)
    - Else -> LocalClient

    Args:
        max_records: Vector pool capacity (default 1024 — always set explicitly).
        dim:         Vector dimension — must match your embedding model.
        max_nodes:   Knowledge Graph node capacity.
        max_edges:   Knowledge Graph edge capacity.
    """
    if remote:
        return SyncRemoteClient(base_url=remote)
    else:
        return LocalClient(
            path=path,
            index_kind=index_kind,
            max_records=max_records,
            dim=dim,
            max_nodes=max_nodes,
            max_edges=max_edges,
        )

def AsyncValoricore(
    remote: Optional[str] = None,
    path: str = "./valori_db",
    index_kind: str = "bruteforce",
    max_records: int = 0,
    dim: int = 0,
    max_nodes: int = 0,
    max_edges: int = 0,
) -> Union[LocalClient, AsyncRemoteClient]:
    """
    Standard Asynchronous Factory.
    - If remote is provided -> AsyncRemoteClient (capacity args ignored)
    - Else -> LocalClient
    """
    if remote:
        return AsyncRemoteClient(base_url=remote)
    else:
        return LocalClient(
            path=path,
            index_kind=index_kind,
            max_records=max_records,
            dim=dim,
            max_nodes=max_nodes,
            max_edges=max_edges,
        )
