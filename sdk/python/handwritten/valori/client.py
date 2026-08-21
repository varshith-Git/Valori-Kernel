# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""The Valori Python SDK entry point."""

from __future__ import annotations

import os
from typing import Any, Optional, Union

import httpx

from .errors import ValoriConfigError
from .resources import (
    Cluster,
    Collection,
    Collections,
    Community,
    Crypto,
    IndexConfig,
    Ingest,
    Meta,
    Operations,
    Proof,
    Snapshots,
    Storage,
    Tree,
)
from .retry import RetryPolicy
from .transport import Transport
from .version import API_CONTRACT_VERSION, __version__

__all__ = ["ValoriClient"]

#: Environment variables the client falls back to, so an API key never has to be
#: hardcoded in application source (§6).
ENDPOINT_ENV = "VALORI_ENDPOINT"
API_KEY_ENV = "VALORI_API_KEY"


class ValoriClient:
    """A client for one Valori node.

    ::

        from valori import ValoriClient

        client = ValoriClient(endpoint="http://localhost:3000", api_key=...)
        docs = client.collections.create("docs", dimension=384, metric="squared_l2")
        docs.records.insert([0.1] * 384, request_id="ins-1")
        docs.search([0.1] * 384, k=5)

    Valori is self-hosted, so there is no default endpoint: pass ``endpoint``
    or set ``VALORI_ENDPOINT``. The API key is read from ``VALORI_API_KEY``
    when not passed, and is never included in ``repr`` or ``str`` (§6).
    """

    #: The Valori REST API contract this SDK targets (§14).
    api_contract_version = API_CONTRACT_VERSION

    def __init__(
        self,
        endpoint: Optional[str] = None,
        *,
        api_key: Optional[str] = None,
        timeout: float = 30.0,
        retry: Optional[RetryPolicy] = None,
        verify_ssl: Union[bool, str] = True,
        follow_redirects: bool = False,
        headers: Optional[dict] = None,
        transport: Optional[httpx.BaseTransport] = None,
        async_transport: Optional[httpx.AsyncBaseTransport] = None,
        _sleep=None,
    ) -> None:
        endpoint = endpoint or os.environ.get(ENDPOINT_ENV)
        if not endpoint:
            raise ValoriConfigError(
                "no endpoint given — pass endpoint=... or set "
                f"{ENDPOINT_ENV} (Valori is self-hosted; there is no default host)"
            )
        if api_key is None:
            api_key = os.environ.get(API_KEY_ENV)

        self._transport = Transport(
            endpoint,
            api_key=api_key,
            timeout=timeout,
            verify_ssl=verify_ssl,
            follow_redirects=follow_redirects,
            headers=headers,
            retry=retry,
            httpx_transport=transport,
            async_httpx_transport=async_transport,
            sleep=_sleep,
        )

        self.collections = Collections(self._transport)
        self.operations = Operations(self._transport)
        self.index = IndexConfig(self._transport)
        self.meta = Meta(self._transport)
        self.ingest = Ingest(self._transport)
        self.tree = Tree(self._transport)
        self.community = Community(self._transport)
        self.proof = Proof(self._transport)
        self.snapshots = Snapshots(self._transport)
        self.storage = Storage(self._transport)
        self.cluster = Cluster(self._transport)
        self.crypto = Crypto(self._transport)

    # ── identity ─────────────────────────────────────────────────────────────

    @property
    def endpoint(self) -> str:
        return self._transport.base_url

    @property
    def sdk_version(self) -> str:
        return __version__

    @property
    def retry_policy(self) -> RetryPolicy:
        return self._transport.retry_policy

    def __repr__(self) -> str:
        # §6: the API key is deliberately absent. Redacted, not omitted-and-
        # forgotten — the field is shown so its absence is visibly intentional.
        key = "***" if self._transport.authenticated else None
        return (
            f"ValoriClient(endpoint={self.endpoint!r}, api_key={key!r}, "
            f"api_contract={self.api_contract_version!r}, sdk={__version__!r})"
        )

    __str__ = __repr__

    # ── shortcuts ────────────────────────────────────────────────────────────

    def collection(self, name: str) -> Collection:
        """Unchecked handle to a collection. Same as ``client.collections[name]``."""
        return self.collections[name]

    def health(self) -> Any:
        """``GET /health``. Shortcut for ``client.meta.health()``."""
        return self.meta.health()

    def version(self) -> Any:
        """``GET /v1/version``. Shortcut for ``client.meta.version()``."""
        return self.meta.version()

    # ── low-level escape hatch ───────────────────────────────────────────────

    @property
    def raw(self):
        """The generated client, for operations the ergonomic layer has not wrapped.

        Every operation in the contract *is* wrapped today (see
        ``api-coverage.yaml``); this exists so a contract that grows an
        operation is usable before the wrapper lands, without anyone standing up
        a second HTTP client.
        """
        return self._transport.raw()

    # ── lifecycle ────────────────────────────────────────────────────────────

    def close(self) -> None:
        self._transport.close()

    async def aclose(self) -> None:
        await self._transport.aclose()

    def __enter__(self) -> "ValoriClient":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()
