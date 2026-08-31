# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Authentication tests — Phase API-4A §6."""

from __future__ import annotations

import httpx
import pytest

from valori import ValoriClient
from valori.errors import ValoriConfigError

from .conftest import HEALTH_OK, json_response, make_client


def test_api_key_is_sent_as_a_bearer_token():
    client = make_client(json_response(HEALTH_OK), api_key="sk-abc123")
    client.health()
    assert client._recorder.last.headers["authorization"] == "Bearer sk-abc123"


def test_no_api_key_means_no_authorization_header():
    client = make_client(json_response(HEALTH_OK), api_key=None)
    client.health()
    assert "authorization" not in client._recorder.last.headers


def test_api_key_never_appears_in_repr_or_str():
    client = make_client(json_response(HEALTH_OK), api_key="sk-super-secret")
    for rendering in (repr(client), str(client), repr(client._transport), str(client._transport)):
        assert "sk-super-secret" not in rendering
        assert "***" in rendering


def test_api_key_is_read_from_the_environment(monkeypatch):
    monkeypatch.setenv("VALORI_API_KEY", "sk-from-env")
    client = ValoriClient("http://node.test", transport=httpx.MockTransport(
        lambda request: httpx.Response(200, json=HEALTH_OK)))
    assert client._transport.authenticated is True
    assert "sk-from-env" not in repr(client)


def test_endpoint_is_read_from_the_environment(monkeypatch):
    monkeypatch.setenv("VALORI_ENDPOINT", "http://from-env:3000")
    client = ValoriClient(transport=httpx.MockTransport(
        lambda request: httpx.Response(200, json={})))
    assert client.endpoint == "http://from-env:3000"


# ── Cross-SDK endpoint-resolution contract (G2.14 parity) ──────────────────
# These five tests exist identically (same names, same assertions) in
# sdk/typescript/tests/auth.test.ts. Endpoint resolution, highest priority
# first: the endpoint argument, then VALORI_ENDPOINT, then — only when an
# api_key was given and neither of those named an endpoint — Cloud SaaS.
# No endpoint and no api_key at all is a configuration error, not a default.

def test_api_key_without_endpoint_defaults_to_cloud_saas(monkeypatch):
    monkeypatch.delenv("VALORI_ENDPOINT", raising=False)
    client = ValoriClient(api_key="vlk_test_key", transport=httpx.MockTransport(
        lambda request: httpx.Response(200, json={})))
    assert client.endpoint == "https://app.valori.systems"


def test_explicit_endpoint_wins_over_env_and_cloud_default(monkeypatch):
    monkeypatch.setenv("VALORI_ENDPOINT", "http://from-env:3000")
    client = ValoriClient(
        "http://explicit:9000",
        api_key="vlk_test_key",
        transport=httpx.MockTransport(lambda request: httpx.Response(200, json={})),
    )
    assert client.endpoint == "http://explicit:9000"


def test_env_endpoint_wins_over_cloud_default_when_api_key_is_also_set(monkeypatch):
    monkeypatch.setenv("VALORI_ENDPOINT", "http://from-env:3000")
    client = ValoriClient(
        api_key="vlk_test_key",
        transport=httpx.MockTransport(lambda request: httpx.Response(200, json={})),
    )
    assert client.endpoint == "http://from-env:3000"


def test_trailing_slashes_are_stripped_from_every_endpoint_source():
    client = ValoriClient(
        "http://node.test///",
        transport=httpx.MockTransport(lambda request: httpx.Response(200, json={})),
    )
    assert client.endpoint == "http://node.test"


def test_missing_endpoint_is_a_configuration_error(monkeypatch):
    monkeypatch.delenv("VALORI_ENDPOINT", raising=False)
    monkeypatch.delenv("VALORI_API_KEY", raising=False)
    with pytest.raises(ValoriConfigError) as exc:
        ValoriClient()
    assert "no endpoint given" in str(exc.value)


def test_non_string_api_key_is_rejected():
    with pytest.raises(ValoriConfigError):
        ValoriClient("http://node.test", api_key=12345)  # type: ignore[arg-type]


def test_default_timeout_is_thirty_seconds():
    # Matches @valori/sdk's default of 30_000ms exactly (G2.14 parity). The
    # underlying httpx client is lazy/private, so this checks the one public,
    # stable surface for the default: the constructor's own signature.
    import inspect

    assert inspect.signature(ValoriClient.__init__).parameters["timeout"].default == 30.0


def test_custom_headers_are_merged_and_do_not_displace_auth():
    client = make_client(json_response(HEALTH_OK), api_key="sk-1", headers={"X-Tenant": "acme"})
    client.health()
    headers = client._recorder.last.headers
    assert headers["x-tenant"] == "acme"
    assert headers["authorization"] == "Bearer sk-1"
