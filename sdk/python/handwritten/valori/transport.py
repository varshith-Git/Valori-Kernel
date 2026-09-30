# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""The one place the handwritten layer touches the generated client.

Phase API-4A §4/§6/§7. :class:`Transport` owns:

* construction of the generated ``AuthenticatedClient``/``Client``,
* the bearer-auth header,
* installation of the retry transport underneath httpx,
* the single ``call`` funnel that turns a generated ``Response`` into either a
  parsed model or a typed exception.

Every resource wrapper goes through :meth:`Transport.call`. Nothing else in the
handwritten layer is allowed to speak HTTP, and the generated layer is never
allowed to import anything from here (§4: the arrow points one way).
"""

from __future__ import annotations

import json
from typing import Any, Optional, Union

import httpx

from valori_generated import errors as _gen_errors
from valori_generated.client import AuthenticatedClient, Client
from valori_generated.models.api_error import ApiError
from valori_generated.types import UNSET, Response, Unset

from .errors import (
    ValoriAPIError,
    ValoriConfigError,
    ValoriConnectionError,
    ValoriTimeoutError,
    error_for,
)
from .retry import AsyncRetryTransport, RetryPolicy, RetryTransport, idempotency_key

__all__ = ["Transport", "unset_if_none"]

_REDACTED = "***"


def unset_if_none(value: Any) -> Union[Any, Unset]:
    """Translate ``None`` to the generated layer's ``UNSET`` sentinel.

    The generated models distinguish "field absent" (``UNSET``) from "field
    present and null" (``None``). Python callers naturally write ``None`` for
    "I did not supply this", so every wrapper funnels optional arguments
    through here rather than repeating the conditional.
    """
    return UNSET if value is None else value


class Transport:
    """Owns the generated client and converts its results into SDK semantics."""

    def __init__(
        self,
        base_url: str,
        *,
        api_key: Optional[str] = None,
        timeout: float = 30.0,
        verify_ssl: Union[bool, str] = True,
        follow_redirects: bool = False,
        headers: Optional[dict] = None,
        retry: Optional[RetryPolicy] = None,
        httpx_transport: Optional[httpx.BaseTransport] = None,
        async_httpx_transport: Optional[httpx.AsyncBaseTransport] = None,
        sleep=None,
    ) -> None:
        if not base_url or not isinstance(base_url, str):
            raise ValoriConfigError("base_url (endpoint) is required")
        if api_key is not None and not isinstance(api_key, str):
            raise ValoriConfigError("api_key must be a string")

        self._base_url = base_url.rstrip("/")
        self._has_key = bool(api_key)
        self.retry_policy = retry if retry is not None else RetryPolicy()

        # Retry is layered *under* the generated client by wrapping the real
        # httpx transport, so the generated code stays a single-shot caller.
        inner = httpx_transport if httpx_transport is not None else httpx.HTTPTransport()
        sync_transport: httpx.BaseTransport = RetryTransport(
            inner, self.retry_policy, **({"sleep": sleep} if sleep else {})
        )
        async_inner = (
            async_httpx_transport
            if async_httpx_transport is not None
            else httpx.AsyncHTTPTransport()
        )
        async_transport = AsyncRetryTransport(async_inner, self.retry_policy)

        common = dict(
            base_url=self._base_url,
            timeout=httpx.Timeout(timeout),
            verify_ssl=verify_ssl,
            follow_redirects=follow_redirects,
            headers=dict(headers or {}),
            raise_on_unexpected_status=False,
            httpx_args={"transport": sync_transport},
        )

        if api_key:
            # §6: the SDK sends `Authorization: Bearer <api_key>`. The generated
            # client's `prefix`/`auth_header_name` defaults are exactly that;
            # they are named explicitly so a generator default change is a
            # visible diff here rather than a silent auth break.
            self._client: Union[AuthenticatedClient, Client] = AuthenticatedClient(
                token=api_key, prefix="Bearer", auth_header_name="Authorization", **common
            )
        else:
            self._client = Client(**common)

        self._async_transport = async_transport
        self._async_ready = False

    # ── identity / hygiene ───────────────────────────────────────────────────

    @property
    def base_url(self) -> str:
        return self._base_url

    @property
    def authenticated(self) -> bool:
        return self._has_key

    def __repr__(self) -> str:
        # §6: never leak the key. Not in repr, not in logs, not in a traceback.
        key = _REDACTED if self._has_key else None
        return f"Transport(base_url={self._base_url!r}, api_key={key!r})"

    __str__ = __repr__

    # ── the funnel ───────────────────────────────────────────────────────────

    def raw(self) -> Union[AuthenticatedClient, Client]:
        """Escape hatch: the underlying generated client.

        Provided so a caller can reach an operation the ergonomic layer has not
        wrapped yet, without building a second HTTP client. Using it means you
        take on the generated layer's raw error semantics.
        """
        return self._client

    def call(self, endpoint: Any, *, request_id: Optional[str] = None, **kwargs: Any) -> Any:
        """Invoke a generated endpoint module and return its parsed success body.

        ``endpoint`` is a generated module such as
        ``valori_generated.api.collections.create_collection``.

        Raises the mapped :class:`~valori.errors.ValoriAPIError` subclass on any
        4xx/5xx, :class:`~valori.errors.ValoriTimeoutError` on timeout, and
        :class:`~valori.errors.ValoriConnectionError` when the node is
        unreachable.
        """
        try:
            with idempotency_key(request_id):
                response: Response = endpoint.sync_detailed(client=self._client, **kwargs)
        except httpx.TimeoutException as exc:
            raise ValoriTimeoutError(f"request to {self._base_url} timed out: {exc}") from exc
        except httpx.TransportError as exc:
            raise ValoriConnectionError(f"could not reach {self._base_url}: {exc}") from exc
        except _gen_errors.UnexpectedStatus as exc:  # pragma: no cover - defensive
            raise ValoriAPIError(
                "server returned an undocumented status",
                status_code=exc.status_code,
                body=exc.content,
            ) from exc
        return self._unwrap(response)

    async def acall(
        self, endpoint: Any, *, request_id: Optional[str] = None, **kwargs: Any
    ) -> Any:
        """Async counterpart to :meth:`call`."""
        self._ensure_async()
        try:
            with idempotency_key(request_id):
                response: Response = await endpoint.asyncio_detailed(
                    client=self._client, **kwargs
                )
        except httpx.TimeoutException as exc:
            raise ValoriTimeoutError(f"request to {self._base_url} timed out: {exc}") from exc
        except httpx.TransportError as exc:
            raise ValoriConnectionError(f"could not reach {self._base_url}: {exc}") from exc
        return self._unwrap(response)

    # ── internals ────────────────────────────────────────────────────────────

    def _ensure_async(self) -> None:
        if self._async_ready:
            return
        # The generated client builds its httpx.AsyncClient lazily and offers no
        # constructor slot for an async transport, so it is injected on first use.
        self._client.set_async_httpx_client(
            httpx.AsyncClient(
                base_url=self._base_url,
                headers=self._auth_headers(),
                transport=self._async_transport,
            )
        )
        self._async_ready = True

    def _auth_headers(self) -> dict:
        if isinstance(self._client, AuthenticatedClient):
            return {
                self._client.auth_header_name: f"{self._client.prefix} {self._client.token}".strip()
            }
        return {}

    def _unwrap(self, response: Response) -> Any:
        status = int(response.status_code)
        if 200 <= status < 300:
            return response.parsed

        parsed = response.parsed
        code: Optional[str] = None
        message: Optional[str] = None
        body: Any

        if isinstance(parsed, ApiError):
            message = parsed.error
            code = getattr(parsed.code, "value", parsed.code)
            body = parsed.to_dict()
        else:
            body = self._decode(response.content)
            if isinstance(body, dict):
                message = body.get("error") or body.get("message")
                raw_code = body.get("code")
                code = raw_code if isinstance(raw_code, str) else None

        raise error_for(
            status,
            code=code,
            message=message,
            body=body,
            headers=response.headers,
        )

    @staticmethod
    def _decode(content: bytes) -> Any:
        if not content:
            return None
        try:
            return json.loads(content.decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            return content.decode("utf-8", errors="replace")

    def close(self) -> None:
        client = getattr(self._client, "_client", None)
        if client is not None:
            client.close()

    async def aclose(self) -> None:
        client = getattr(self._client, "_async_client", None)
        if client is not None:
            await client.aclose()
