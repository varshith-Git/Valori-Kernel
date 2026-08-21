# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Error-mapping tests — Phase API-4A §7."""

from __future__ import annotations

import httpx
import pytest

from valori import errors
from valori.errors import (
    AuthenticationError,
    AuthorizationError,
    CapacityExceededError,
    CollectionNotFoundError,
    ConflictError,
    DimensionMismatchError,
    NotLeaderError,
    RateLimitError,
    RecordNotFoundError,
    ServerError,
    ServiceUnavailableError,
    ValidationError,
    ValoriAPIError,
    ValoriConnectionError,
    ValoriTimeoutError,
    error_for,
)

from .conftest import HEALTH_OK, json_response, make_client

CODE_EXPECTATIONS = {
    "validation_error": ValidationError,
    "unauthorized": AuthenticationError,
    "forbidden": AuthorizationError,
    "not_found": errors.NotFoundError,
    "collection_not_found": CollectionNotFoundError,
    "record_not_found": RecordNotFoundError,
    "dimension_mismatch": DimensionMismatchError,
    "invalid_metric": errors.InvalidMetricError,
    "invalid_index": errors.InvalidIndexError,
    "index_build_failed": errors.IndexBuildFailedError,
    "conflict": ConflictError,
    "capacity_exceeded": CapacityExceededError,
    "not_leader": NotLeaderError,
    "unavailable": ServiceUnavailableError,
    "not_implemented": errors.NotImplementedAPIError,
    "internal_error": ServerError,
}


@pytest.mark.parametrize("code,expected", sorted(CODE_EXPECTATIONS.items()))
def test_each_code_maps_to_its_exception(code, expected):
    exc = error_for(400, code=code, message="boom", body={"error": "boom", "code": code})
    assert isinstance(exc, expected)
    assert exc.code == code
    assert exc.message == "boom"


def test_every_contract_error_code_has_an_exception(contract):
    """The closed enum in the contract must be fully covered.

    This is the check that keeps the table honest: add a code to the Rust
    ``ErrorCode`` enum and this test fails until the SDK names it.
    """
    declared = set(contract["components"]["schemas"]["ErrorCode"]["enum"])
    mapped = set(errors._CODE_MAP)
    assert declared - mapped == set(), f"contract codes with no SDK exception: {declared - mapped}"
    assert mapped - declared == set(), f"SDK exceptions for codes not in the contract: {mapped - declared}"


def test_unknown_code_degrades_to_the_generic_error_without_losing_anything():
    body = {"error": "something new", "code": "quantum_desync", "request_id": "req-9"}
    exc = error_for(418, code="quantum_desync", message="something new", body=body)
    assert type(exc) is ValoriAPIError
    assert exc.status_code == 418
    assert exc.code == "quantum_desync"
    assert exc.message == "something new"
    assert exc.request_id == "req-9"
    assert exc.body == body


def test_status_is_the_fallback_when_there_is_no_code():
    assert isinstance(error_for(401, body="<html>nginx</html>"), AuthenticationError)
    assert isinstance(error_for(503, body=None), ServiceUnavailableError)
    assert type(error_for(418, body=None)) is ValoriAPIError


def test_429_is_a_rate_limit_regardless_of_code_and_carries_retry_after():
    exc = error_for(429, code=None, headers={"Retry-After": "12"})
    assert isinstance(exc, RateLimitError)
    assert exc.retry_after == 12.0


def test_http_date_retry_after_is_not_guessed_at():
    exc = error_for(429, headers={"Retry-After": "Wed, 21 Oct 2026 07:28:00 GMT"})
    assert isinstance(exc, RateLimitError)
    assert exc.retry_after is None
    assert exc.headers["Retry-After"].startswith("Wed")


def test_request_id_is_read_from_headers_then_body():
    from_header = error_for(500, headers={"X-Request-Id": "hdr-1"}, body={"request_id": "body-1"})
    assert from_header.request_id == "hdr-1"
    from_body = error_for(500, body={"request_id": "body-1"})
    assert from_body.request_id == "body-1"


def test_error_raised_through_the_real_call_path_carries_the_response():
    client = make_client(json_response(
        {"error": "no such collection", "code": "collection_not_found"}, status=404))
    with pytest.raises(CollectionNotFoundError) as caught:
        client.collections.delete("ghost")
    exc = caught.value
    assert exc.status_code == 404
    assert exc.code == "collection_not_found"
    assert exc.message == "no such collection"
    assert exc.body == {"error": "no such collection", "code": "collection_not_found"}


def test_create_collection_conflict_becomes_collection_already_exists():
    client = make_client(json_response({"error": "exists", "code": "conflict"}, status=409))
    with pytest.raises(errors.CollectionAlreadyExistsError) as caught:
        client.collections.create("docs", dimension=3, metric="squared_l2")
    # Still a ConflictError, so `except ConflictError` keeps working.
    assert isinstance(caught.value, ConflictError)
    assert caught.value.code == "conflict"


def test_conflict_elsewhere_stays_a_plain_conflict_error():
    client = make_client(json_response({"error": "busy", "code": "conflict"}, status=409))
    with pytest.raises(ConflictError) as caught:
        client.collections["docs"].records.delete(1)
    assert type(caught.value) is ConflictError


def test_non_json_error_body_is_preserved_as_text():
    def handler(_request):
        return httpx.Response(502, content=b"<html>bad gateway</html>")

    client = make_client(handler)
    with pytest.raises(ValoriAPIError) as caught:
        client.health()
    assert "bad gateway" in caught.value.body


def test_connection_failure_becomes_a_connection_error():
    def handler(request):
        raise httpx.ConnectError("refused", request=request)

    client = make_client(handler)
    with pytest.raises(ValoriConnectionError):
        client.health()


def test_timeout_becomes_a_timeout_error():
    def handler(request):
        raise httpx.ReadTimeout("slow", request=request)

    client = make_client(handler)
    with pytest.raises(ValoriTimeoutError):
        client.health()


def test_success_is_not_turned_into_an_error():
    client = make_client(json_response(HEALTH_OK))
    assert client.health().status == "ok"
