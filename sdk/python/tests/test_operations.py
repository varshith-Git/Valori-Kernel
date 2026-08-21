# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Async-operation polling tests — Phase API-4A §9."""

from __future__ import annotations

import httpx
import pytest

from valori.errors import IndexBuildFailedError, OperationFailedError, OperationTimeoutError
from valori.resources.index import TERMINAL_INDEX_STATES
from valori.resources.operations import FAILED_STATES, TERMINAL_STATES

from .conftest import make_client


def _op(status: str) -> dict:
    return {
        "id": "op-1",
        "type": "insert_record",
        "status": status,
        "timing": "0ms",
        "timestamp_unix": 0,
        "collection": "docs",
        "overview": {"id": "op-1", "type": "insert_record", "status": status,
                     "timing": "0ms", "collection": "docs"},
        "results": {"status": status, "records_affected": 0, "nodes_affected": 0,
                    "edges_affected": 0, "message": ""},
        "metrics": {"duration_ms": 0, "memory_bytes": 0, "cpu_cycles": 0, "status": status},
        "proof": {},
    }


def _sequence(statuses):
    """Answers one operation status per call, repeating the last one forever."""
    remaining = list(statuses)

    def handler(_request: httpx.Request) -> httpx.Response:
        status = remaining.pop(0) if len(remaining) > 1 else remaining[0]
        return httpx.Response(200, json=_op(status))

    return handler


# ── clock injection ──────────────────────────────────────────────────────────


class Clock:
    """A monotonic clock the test advances by hand, so no test ever sleeps."""

    def __init__(self) -> None:
        self.t = 0.0

    def now(self) -> float:
        return self.t

    def sleep(self, seconds: float) -> None:
        self.t += seconds


# ── happy paths ──────────────────────────────────────────────────────────────


def test_get_returns_a_handle_carrying_the_current_status():
    client = make_client(_sequence(["processing"]))
    op = client.operations.get("op-1")
    assert op.id == "op-1"
    assert op.status == "processing"
    assert op.done is False
    assert "op-1" in repr(op)


def test_wait_polls_until_the_operation_completes():
    clock = Clock()
    client = make_client(_sequence(["processing", "processing", "completed"]))
    op = client.operations.get("op-1").wait(_now=clock.now, _sleep=clock.sleep)
    assert op.status == "completed"
    assert op.done is True
    # one for get(), then one per poll until it settled
    assert client._recorder.count == 3


def test_wait_on_the_resource_is_the_same_as_get_then_wait():
    clock = Clock()
    client = make_client(_sequence(["completed"]))
    op = client.operations.wait("op-1", _now=clock.now, _sleep=clock.sleep)
    assert op.status == "completed"


def test_an_already_terminal_operation_does_not_poll_again():
    clock = Clock()
    client = make_client(_sequence(["completed"]))
    client.operations.get("op-1").wait(_now=clock.now, _sleep=clock.sleep)
    assert client._recorder.count == 1


def test_refresh_re_reads_the_operation():
    client = make_client(_sequence(["processing", "completed"]))
    op = client.operations.get("op-1")
    assert op.status == "processing"
    assert op.refresh().status == "completed"


# ── failure conversion ───────────────────────────────────────────────────────


def test_a_failed_operation_raises_with_its_id_and_status():
    clock = Clock()
    client = make_client(_sequence(["processing", "failed"]))
    with pytest.raises(OperationFailedError) as caught:
        client.operations.get("op-1").wait(_now=clock.now, _sleep=clock.sleep)
    assert caught.value.operation_id == "op-1"
    assert caught.value.status == "failed"
    assert caught.value.detail is not None


def test_failure_conversion_can_be_switched_off():
    clock = Clock()
    client = make_client(_sequence(["failed"]))
    op = client.operations.get("op-1").wait(
        raise_on_failure=False, _now=clock.now, _sleep=clock.sleep)
    assert op.failed is True
    assert op.status == "failed"


def test_wait_times_out_and_reports_the_last_status_seen():
    clock = Clock()
    client = make_client(_sequence(["processing"]))
    with pytest.raises(OperationTimeoutError) as caught:
        client.operations.get("op-1").wait(
            poll_interval=1.0, timeout=3.0, _now=clock.now, _sleep=clock.sleep)
    assert caught.value.operation_id == "op-1"
    assert caught.value.last_status == "processing"


def test_the_poll_interval_is_what_actually_governs_the_wait():
    clock = Clock()
    client = make_client(_sequence(["processing"]))
    with pytest.raises(OperationTimeoutError):
        client.operations.get("op-1").wait(
            poll_interval=5.0, timeout=12.0, _now=clock.now, _sleep=clock.sleep)
    assert clock.t == 15.0  # three sleeps of five seconds


def test_terminal_and_failed_state_sets_are_consistent():
    assert FAILED_STATES <= TERMINAL_STATES
    assert "completed" in TERMINAL_STATES and "completed" not in FAILED_STATES
    assert "processing" not in TERMINAL_STATES


# ── index builds get the same ergonomics ─────────────────────────────────────


def _index(status: str) -> dict:
    return {"collection": "docs", "desired_type": "hnsw",
            "active_type": "none", "status": status}


def _index_sequence(statuses):
    remaining = list(statuses)

    def handler(_request: httpx.Request) -> httpx.Response:
        status = remaining.pop(0) if len(remaining) > 1 else remaining[0]
        return httpx.Response(200, json=_index(status))

    return handler


def test_index_wait_polls_until_the_build_is_active():
    clock = Clock()
    client = make_client(_index_sequence(["building", "building", "active"]))
    result = client.collections["docs"].index.wait(_now=clock.now, _sleep=clock.sleep)
    assert result.status == "active"
    assert client._recorder.count == 3


def test_a_failed_index_build_raises():
    clock = Clock()
    client = make_client(_index_sequence(["building", "failed"]))
    with pytest.raises(IndexBuildFailedError):
        client.collections["docs"].index.wait(_now=clock.now, _sleep=clock.sleep)


def test_index_wait_times_out():
    clock = Clock()
    client = make_client(_index_sequence(["building"]))
    with pytest.raises(OperationTimeoutError) as caught:
        client.collections["docs"].index.wait(
            poll_interval=2.0, timeout=4.0, _now=clock.now, _sleep=clock.sleep)
    assert caught.value.last_status == "building"


def test_index_terminal_states_match_what_the_engine_emits():
    # valori-engine's IndexStatusResponse::from_state emits exactly these four.
    assert TERMINAL_INDEX_STATES == {"active", "failed", "none"}
