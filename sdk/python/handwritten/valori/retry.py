# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Retry policy and the httpx transport that enforces it.

Phase API-4A §8. Three rules shape this module:

1. **Retry lives only here.** The generated layer never retries; it issues one
   request and reports what came back. Retry is a policy decision, and policy
   is human-owned code.
2. **Nothing is retried blindly.** A retry is allowed only when the method is
   safe (``GET``/``HEAD``/``OPTIONS``) or the request carries an idempotency
   signal the server honours (``request_id``, surfaced as the
   ``Idempotency-Key`` header). A bare ``POST /v1/records`` is never retried:
   Valori dedups on ``request_id``, and without one a retry can double-insert.
3. **``Retry-After`` wins.** When the server names a delay, we wait exactly
   that long instead of applying our own backoff.

The transport wraps the *real* httpx transport, so retries happen underneath
the generated client and are invisible to it — no second HTTP stack.
"""

from __future__ import annotations

import contextlib
import random
import time
from contextvars import ContextVar
from dataclasses import dataclass, field, replace
from typing import FrozenSet, Iterator, Optional

import httpx

__all__ = [
    "RetryPolicy",
    "RetryTransport",
    "AsyncRetryTransport",
    "IDEMPOTENCY_HEADER",
    "idempotency_key",
]

#: Header the SDK sets from a caller-supplied ``request_id``. Its presence is
#: what makes a write eligible for retry at all.
IDEMPOTENCY_HEADER = "Idempotency-Key"

# The generated endpoint functions take no per-call header argument, and the
# generated client's ``with_headers`` mutates shared state. A context variable
# is the one mechanism that carries a per-call idempotency key down to the
# transport without leaking it into the next call or another thread/task.
_ACTIVE_KEY: ContextVar[Optional[str]] = ContextVar("valori_idempotency_key", default=None)


@contextlib.contextmanager
def idempotency_key(key: Optional[str]) -> Iterator[None]:
    """Attach ``key`` as the ``Idempotency-Key`` header for requests in this block."""
    token = _ACTIVE_KEY.set(key)
    try:
        yield
    finally:
        _ACTIVE_KEY.reset(token)


def _apply_active_key(request: httpx.Request) -> None:
    key = _ACTIVE_KEY.get()
    if key and IDEMPOTENCY_HEADER not in request.headers:
        request.headers[IDEMPOTENCY_HEADER] = key


_DEFAULT_RETRY_STATUS: FrozenSet[int] = frozenset({408, 429, 500, 502, 503, 504})
_DEFAULT_SAFE_METHODS: FrozenSet[str] = frozenset({"GET", "HEAD", "OPTIONS"})


@dataclass(frozen=True)
class RetryPolicy:
    """Configurable retry behaviour. Immutable; use :meth:`evolve` to tweak."""

    #: Total attempts including the first. ``1`` disables retry entirely.
    max_attempts: int = 3
    #: First backoff delay, in seconds.
    backoff_initial: float = 0.25
    #: Multiplier applied to the delay after each attempt.
    backoff_multiplier: float = 2.0
    #: Ceiling on a single backoff delay, in seconds.
    backoff_max: float = 8.0
    #: Full-jitter fraction, 0.0–1.0. ``0`` makes delays deterministic, which
    #: is what the tests use.
    jitter: float = 0.1
    #: Statuses worth trying again.
    retry_status: FrozenSet[int] = field(default=_DEFAULT_RETRY_STATUS)
    #: Methods that are safe to repeat with no further evidence.
    safe_methods: FrozenSet[str] = field(default=_DEFAULT_SAFE_METHODS)
    #: Retry transport-level failures (connection reset, read timeout) for safe
    #: methods and idempotent writes.
    retry_on_connection_error: bool = True
    #: When False, a write is never retried even if it carries an idempotency
    #: key. Set this if you would rather see the failure than risk a duplicate.
    retry_idempotent_writes: bool = True
    #: Honour ``Retry-After`` in preference to computed backoff.
    respect_retry_after: bool = True
    #: Upper bound on a server-named ``Retry-After``, so a hostile or buggy
    #: header cannot park a caller for an hour.
    retry_after_max: float = 60.0

    def evolve(self, **changes: object) -> "RetryPolicy":
        """Return a copy with the named fields replaced."""
        return replace(self, **changes)  # type: ignore[arg-type]

    # ── decisions ────────────────────────────────────────────────────────────

    def is_retryable_request(self, method: str, has_idempotency_key: bool) -> bool:
        """Is this request shape eligible for a second attempt at all?"""
        if self.max_attempts <= 1:
            return False
        if method.upper() in self.safe_methods:
            return True
        return self.retry_idempotent_writes and has_idempotency_key

    def should_retry_status(self, status_code: int) -> bool:
        return status_code in self.retry_status

    def delay_for(self, attempt: int, retry_after: Optional[float] = None) -> float:
        """Seconds to wait before attempt ``attempt + 1`` (``attempt`` is 1-based)."""
        if self.respect_retry_after and retry_after is not None:
            return max(0.0, min(retry_after, self.retry_after_max))
        base = self.backoff_initial * (self.backoff_multiplier ** (attempt - 1))
        base = min(base, self.backoff_max)
        if self.jitter:
            base += base * self.jitter * random.random()
        return base


def _retry_after_seconds(response: httpx.Response) -> Optional[float]:
    raw = response.headers.get("retry-after")
    if raw is None:
        return None
    try:
        return float(raw)
    except (TypeError, ValueError):
        return None


def _has_idempotency_key(request: httpx.Request) -> bool:
    return IDEMPOTENCY_HEADER.lower() in {k.lower() for k in request.headers.keys()}


class RetryTransport(httpx.BaseTransport):
    """Sync httpx transport that applies :class:`RetryPolicy` to an inner transport."""

    def __init__(
        self,
        inner: httpx.BaseTransport,
        policy: RetryPolicy,
        *,
        sleep=time.sleep,  # injectable so tests do not actually wait
    ) -> None:
        self._inner = inner
        self._policy = policy
        self._sleep = sleep

    @property
    def policy(self) -> RetryPolicy:
        return self._policy

    def handle_request(self, request: httpx.Request) -> httpx.Response:
        _apply_active_key(request)
        eligible = self._policy.is_retryable_request(
            request.method, _has_idempotency_key(request)
        )
        attempt = 0
        while True:
            attempt += 1
            try:
                response = self._inner.handle_request(request)
            except (httpx.ConnectError, httpx.ReadError, httpx.WriteError, httpx.TimeoutException):
                if (
                    not eligible
                    or not self._policy.retry_on_connection_error
                    or attempt >= self._policy.max_attempts
                ):
                    raise
                self._sleep(self._policy.delay_for(attempt))
                continue

            if (
                not eligible
                or attempt >= self._policy.max_attempts
                or not self._policy.should_retry_status(response.status_code)
            ):
                return response

            # Drain and close so the connection can be reused for the retry.
            response.read()
            retry_after = _retry_after_seconds(response)
            response.close()
            self._sleep(self._policy.delay_for(attempt, retry_after))

    def close(self) -> None:
        self._inner.close()


class AsyncRetryTransport(httpx.AsyncBaseTransport):
    """Async counterpart to :class:`RetryTransport`."""

    def __init__(self, inner: httpx.AsyncBaseTransport, policy: RetryPolicy, *, sleep=None) -> None:
        import asyncio

        self._inner = inner
        self._policy = policy
        self._sleep = sleep or asyncio.sleep

    @property
    def policy(self) -> RetryPolicy:
        return self._policy

    async def handle_async_request(self, request: httpx.Request) -> httpx.Response:
        _apply_active_key(request)
        eligible = self._policy.is_retryable_request(
            request.method, _has_idempotency_key(request)
        )
        attempt = 0
        while True:
            attempt += 1
            try:
                response = await self._inner.handle_async_request(request)
            except (httpx.ConnectError, httpx.ReadError, httpx.WriteError, httpx.TimeoutException):
                if (
                    not eligible
                    or not self._policy.retry_on_connection_error
                    or attempt >= self._policy.max_attempts
                ):
                    raise
                await self._sleep(self._policy.delay_for(attempt))
                continue

            if (
                not eligible
                or attempt >= self._policy.max_attempts
                or not self._policy.should_retry_status(response.status_code)
            ):
                return response

            await response.aread()
            retry_after = _retry_after_seconds(response)
            await response.aclose()
            await self._sleep(self._policy.delay_for(attempt, retry_after))

    async def aclose(self) -> None:
        await self._inner.aclose()
