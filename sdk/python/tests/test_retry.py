# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Retry tests — Phase API-4A §8.

The rule under test is the one that matters: a write is never repeated unless
the caller gave the server something to dedup on. Everything else is backoff
arithmetic.
"""

from __future__ import annotations

import httpx
import pytest

from valori import RetryPolicy
from valori.errors import ServiceUnavailableError, ValoriConnectionError
from valori.retry import IDEMPOTENCY_HEADER, RetryPolicy as _RP

from .conftest import COLLECTIONS_OK, HEALTH_OK, INSERT_OK, make_client

NO_JITTER = RetryPolicy(jitter=0.0)


# ── policy arithmetic ────────────────────────────────────────────────────────


def test_safe_methods_are_retryable_without_any_further_evidence():
    p = NO_JITTER
    for method in ("GET", "HEAD", "OPTIONS", "get"):
        assert p.is_retryable_request(method, has_idempotency_key=False) is True


def test_writes_are_not_retryable_without_an_idempotency_key():
    p = NO_JITTER
    for method in ("POST", "PATCH", "DELETE", "PUT"):
        assert p.is_retryable_request(method, has_idempotency_key=False) is False


def test_writes_become_retryable_with_an_idempotency_key():
    assert NO_JITTER.is_retryable_request("POST", has_idempotency_key=True) is True


def test_idempotent_write_retry_can_be_switched_off():
    p = NO_JITTER.evolve(retry_idempotent_writes=False)
    assert p.is_retryable_request("POST", has_idempotency_key=True) is False
    assert p.is_retryable_request("GET", has_idempotency_key=False) is True


def test_max_attempts_one_disables_retry_entirely():
    p = NO_JITTER.evolve(max_attempts=1)
    assert p.is_retryable_request("GET", has_idempotency_key=False) is False


def test_backoff_is_exponential_and_capped():
    p = _RP(backoff_initial=1.0, backoff_multiplier=2.0, backoff_max=4.0, jitter=0.0)
    assert [p.delay_for(n) for n in (1, 2, 3, 4, 5)] == [1.0, 2.0, 4.0, 4.0, 4.0]


def test_retry_after_wins_over_computed_backoff():
    p = _RP(backoff_initial=10.0, jitter=0.0)
    assert p.delay_for(1, retry_after=2.0) == 2.0


def test_retry_after_is_clamped_so_a_hostile_header_cannot_park_the_caller():
    p = _RP(jitter=0.0, retry_after_max=30.0)
    assert p.delay_for(1, retry_after=3600.0) == 30.0


def test_retry_after_can_be_ignored_by_policy():
    p = _RP(backoff_initial=1.0, jitter=0.0, respect_retry_after=False)
    assert p.delay_for(1, retry_after=99.0) == 1.0


def test_jitter_only_ever_lengthens_the_wait():
    p = _RP(backoff_initial=1.0, backoff_multiplier=1.0, jitter=0.5)
    for _ in range(50):
        assert 1.0 <= p.delay_for(1) <= 1.5


def test_policy_is_immutable_and_evolve_returns_a_copy():
    p = NO_JITTER
    q = p.evolve(max_attempts=9)
    assert p.max_attempts == 3 and q.max_attempts == 9
    with pytest.raises(Exception):
        p.max_attempts = 4  # type: ignore[misc]


# ── behaviour through the real transport ─────────────────────────────────────


def _flaky(statuses, final_json):
    """Answers `statuses` in order, then 200 with `final_json` forever."""
    remaining = list(statuses)

    def handler(_request: httpx.Request) -> httpx.Response:
        if remaining:
            return httpx.Response(remaining.pop(0), json={"error": "later", "code": "unavailable"})
        return httpx.Response(200, json=final_json)

    return handler


def test_a_get_is_retried_until_it_succeeds():
    client = make_client(_flaky([503, 503], COLLECTIONS_OK), retry=NO_JITTER)
    assert client.collections.names() == ["docs", "notes"]
    assert client._recorder.count == 3


def test_a_get_stops_at_max_attempts_and_surfaces_the_last_error():
    client = make_client(_flaky([503, 503, 503], COLLECTIONS_OK), retry=NO_JITTER)
    with pytest.raises(ServiceUnavailableError):
        client.collections.list()
    assert client._recorder.count == 3


def test_a_post_without_a_request_id_is_never_retried():
    client = make_client(_flaky([503], INSERT_OK), retry=NO_JITTER)
    with pytest.raises(ServiceUnavailableError):
        client.collections["docs"].records.insert([0.1, 0.2, 0.3])
    assert client._recorder.count == 1, "a non-idempotent write must not be repeated"


def test_a_post_with_a_request_id_is_retried_and_carries_the_idempotency_header():
    client = make_client(
        _flaky([503], INSERT_OK),
        retry=NO_JITTER,
    )
    client.collections["docs"].records.insert([0.1, 0.2, 0.3], request_id="ins-1")
    assert client._recorder.count == 2
    for request in client._recorder.requests:
        assert request.headers[IDEMPOTENCY_HEADER] == "ins-1"


def test_the_idempotency_key_does_not_leak_into_the_next_call():
    bodies = [INSERT_OK, HEALTH_OK]

    def handler(_request):
        return httpx.Response(200, json=bodies.pop(0))

    client = make_client(handler, retry=NO_JITTER)
    client.collections["docs"].records.insert([0.1], request_id="ins-1")
    client.health()
    assert IDEMPOTENCY_HEADER not in client._recorder.last.headers


def test_non_retryable_statuses_are_not_retried():
    client = make_client(_flaky([400], COLLECTIONS_OK), retry=NO_JITTER)
    with pytest.raises(Exception):
        client.collections.list()
    assert client._recorder.count == 1


def test_connection_errors_are_retried_for_safe_methods():
    calls = {"n": 0}

    def handler(request: httpx.Request) -> httpx.Response:
        calls["n"] += 1
        if calls["n"] < 3:
            raise httpx.ConnectError("refused", request=request)
        return httpx.Response(200, json=COLLECTIONS_OK)

    client = make_client(handler, retry=NO_JITTER)
    assert client.collections.names() == ["docs", "notes"]
    assert calls["n"] == 3


def test_connection_error_retry_can_be_switched_off():
    def handler(request: httpx.Request) -> httpx.Response:
        raise httpx.ConnectError("refused", request=request)

    client = make_client(handler, retry=NO_JITTER.evolve(retry_on_connection_error=False))
    with pytest.raises(ValoriConnectionError):
        client.collections.list()
    assert client._recorder.count == 1


def test_server_named_retry_after_is_honoured():
    waited = []
    remaining = [429]

    def handler(_request: httpx.Request) -> httpx.Response:
        if remaining:
            remaining.pop()
            return httpx.Response(429, json={"error": "slow down"}, headers={"Retry-After": "7"})
        return httpx.Response(200, json=COLLECTIONS_OK)

    client = make_client(handler, retry=NO_JITTER, _sleep_probe=waited)
    client.collections.list()
    assert waited == [7.0]


def test_the_configured_policy_is_visible_on_the_client():
    client = make_client(lambda r: httpx.Response(200, json=HEALTH_OK),
                         retry=RetryPolicy(max_attempts=9))
    assert client.retry_policy.max_attempts == 9
