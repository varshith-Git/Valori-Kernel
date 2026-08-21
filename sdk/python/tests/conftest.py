# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Shared fixtures.

Tests drive the SDK through ``httpx.MockTransport``, which is injected *below*
the retry transport and below the generated client. That means a wrapper test
exercises the real code path — model construction, the generated endpoint
module, header assembly, response parsing, error mapping — and stubs only the
socket. Nothing is monkeypatched out of the SDK itself.
"""

from __future__ import annotations

import json
import pathlib
from typing import Any, Callable, List, Optional

import httpx
import pytest

from valori import ValoriClient

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
CONTRACT_PATH = REPO_ROOT / "api" / "openapi" / "valori-v1.yaml"


class Recorder:
    """Captures every request the SDK actually put on the wire."""

    def __init__(self) -> None:
        self.requests: List[httpx.Request] = []

    @property
    def last(self) -> httpx.Request:
        assert self.requests, "no request was made"
        return self.requests[-1]

    @property
    def body(self) -> Any:
        raw = self.last.content
        return json.loads(raw) if raw else None

    @property
    def count(self) -> int:
        return len(self.requests)


def make_client(
    handler: Callable[[httpx.Request], httpx.Response],
    *,
    api_key: Optional[str] = "test-key",
    recorder: Optional[Recorder] = None,
    _sleep_probe: Optional[List[float]] = None,
    **kwargs: Any,
) -> ValoriClient:
    rec = recorder if recorder is not None else Recorder()

    def wrapped(request: httpx.Request) -> httpx.Response:
        request.read()
        rec.requests.append(request)
        return handler(request)

    def sleep(seconds: float) -> None:
        # Retries must never actually wait in a unit test; when a test cares how
        # long the SDK *decided* to wait, it passes a probe list to read back.
        if _sleep_probe is not None:
            _sleep_probe.append(seconds)

    client = ValoriClient(
        "http://node.test",
        api_key=api_key,
        transport=httpx.MockTransport(wrapped),
        _sleep=sleep,
        **kwargs,
    )
    client._recorder = rec  # type: ignore[attr-defined]
    return client


def json_response(payload: Any, status: int = 200, headers: Optional[dict] = None):
    """A handler that always answers with the same JSON body."""

    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(status, json=payload, headers=headers or {})

    return handler


# Minimal bodies that satisfy the contract's required fields. Kept small on
# purpose: a test that needs a field asserts on it explicitly.
HEALTH_OK = {"status": "ok", "mode": "standalone", "version": "0.0.0-test", "shard_count": 1}
COLLECTIONS_OK = {"collections": [{"name": "docs", "id": 0}, {"name": "notes", "id": 1}]}
SEARCH_OK = {"results": []}
INSERT_RECEIPT = {
    "record_id": 7, "old_root": "00", "new_root": "ab", "proof": [],
    "sequence": 1, "timestamp": 0, "state_hash": "ab",
}
INSERT_OK = {"id": 7, "deduplicated": False, "receipt": INSERT_RECEIPT}
# A TreeReceipt with every required field present, for wrappers that must build
# one on the request side.
TREE_RECEIPT = {
    "query": "q", "query_hash": "00", "visited_node_ids": [], "fetched_ranges": [],
    "evidence_hash": "00", "answer_hash": "00", "prev_hash": "00",
    "receipt_hash": "00", "hash_algo": "blake3", "timestamp": 0,
}
TREE_INDEX = {"doc_name": "doc.md", "roots": [], "nodes": []}


@pytest.fixture
def recorder() -> Recorder:
    return Recorder()


@pytest.fixture(scope="session")
def contract() -> dict:
    yaml = pytest.importorskip("yaml")
    if not CONTRACT_PATH.exists():  # pragma: no cover - only in a stripped sdist
        pytest.skip(f"contract not available at {CONTRACT_PATH}")
    return yaml.safe_load(CONTRACT_PATH.read_text())
