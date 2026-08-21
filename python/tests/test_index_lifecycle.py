# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Phase 4.1 + 4.2 — pins the exact wire contract for the four index-lifecycle
SDK methods and validates error handling and the status model.

No live node required: `_t.post` / `_t.get` are stubbed to capture the
payload instead of making an HTTP call.

Methods tested (sync + async each):
  - collection_index_status  → GET /v1/namespaces/{name}/index
  - create_collection_index  → POST /v1/namespaces/{name}/index
  - change_collection_index  → POST /v1/namespaces/{name}/index (alias)
  - drop_collection_index    → POST /v1/namespaces/{name}/index {type: null}

Error scenarios:
  - 409 Conflict  → already building (raises requests.HTTPError)
  - 501 Not Implemented → cluster mode (raises requests.HTTPError)
  - 404 Not Found → unknown collection (raises requests.HTTPError)
  - 400 Bad Request → invalid index type (raises requests.HTTPError)

Status model:
  - Full building payload (active + building generations coexist)
  - Full active payload
  - Full failed payload
"""
import pytest
import requests
from unittest.mock import MagicMock

from valoricore.remote import SyncRemoteClient, AsyncRemoteClient


# ── helpers ───────────────────────────────────────────────────────────────────


def _make_get_response(body: dict, status: int = 200):
    """Stub response returned by `_t.get` that mimics requests.Response."""
    r = MagicMock()
    r.status_code = status
    r.json.return_value = body
    if status >= 400:
        r.raise_for_status.side_effect = requests.HTTPError(response=r)
    else:
        r.raise_for_status = MagicMock()
    return r


def _make_post_response(body: dict, status: int = 202):
    """Stub response returned by `_t.post` that mimics requests.Response."""
    r = MagicMock()
    r.status_code = status
    r.json.return_value = body
    if status >= 400:
        r.raise_for_status.side_effect = requests.HTTPError(response=r)
    else:
        r.raise_for_status = MagicMock()
    return r


def _sync_client():
    return SyncRemoteClient("http://localhost:3000")


# ── collection_index_status ────────────────────────────────────────────────────


def test_sync_collection_index_status_url():
    """collection_index_status must GET the correct URL."""
    client = _sync_client()
    expected_body = {
        "collection": "docs",
        "status": "active",
        "index_type": "hnsw",
        "generation": 0,
    }
    client._t.get = MagicMock(return_value=_make_get_response(expected_body))

    result = client.collection_index_status("docs")

    client._t.get.assert_called_once()
    call_url = client._t.get.call_args[0][0]
    assert call_url.endswith("/v1/namespaces/docs/index"), (
        f"expected /v1/namespaces/docs/index, got {call_url!r}"
    )
    assert result == expected_body


def test_sync_collection_index_status_returns_full_body():
    """Status payload is returned verbatim (no field stripping)."""
    client = _sync_client()
    body = {"collection": "images", "status": "building", "generation": 1}
    client._t.get = MagicMock(return_value=_make_get_response(body))

    result = client.collection_index_status("images")
    assert result["status"] == "building"
    assert result["generation"] == 1


# ── create_collection_index ────────────────────────────────────────────────────


def test_sync_create_index_minimal_payload():
    """create_collection_index with no parameters must send {type}."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response({"collection": "docs", "generation": 0})
    )

    client.create_collection_index("docs", "hnsw")

    client._t.post.assert_called_once()
    _, kwargs = client._t.post.call_args
    payload = kwargs.get("json") or client._t.post.call_args[1].get("json")
    assert payload == {"type": "hnsw"}, f"expected {{type: hnsw}}, got {payload}"


def test_sync_create_index_with_parameters():
    """create_collection_index with parameters must include them in the payload."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response({"collection": "vec", "generation": 0})
    )

    params = {"n_list": 64, "n_probe": 8}
    client.create_collection_index("vec", "ivf", parameters=params)

    _, kwargs = client._t.post.call_args
    payload = kwargs.get("json") or client._t.post.call_args[1].get("json")
    assert payload == {"type": "ivf", "parameters": {"n_list": 64, "n_probe": 8}}


def test_sync_create_index_url():
    """create_collection_index must POST to /v1/namespaces/{name}/index."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response({"collection": "col", "generation": 0})
    )

    client.create_collection_index("col", "bq")

    call_url = client._t.post.call_args[0][0]
    assert call_url.endswith("/v1/namespaces/col/index"), (
        f"unexpected URL: {call_url!r}"
    )


def test_sync_change_collection_index_is_alias():
    """change_collection_index delegates to create_collection_index."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response({"collection": "x", "generation": 1})
    )

    client.change_collection_index("x", "hnsw", parameters={"m": 32})

    _, kwargs = client._t.post.call_args
    payload = kwargs.get("json") or client._t.post.call_args[1].get("json")
    assert payload["type"] == "hnsw"
    assert payload.get("parameters", {}).get("m") == 32


# ── drop_collection_index ──────────────────────────────────────────────────────


def test_sync_drop_index_sends_type_null():
    """drop_collection_index must POST {type: null} to the index endpoint."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response({"collection": "docs", "status": "none"})
    )

    client.drop_collection_index("docs")

    _, kwargs = client._t.post.call_args
    payload = kwargs.get("json") or client._t.post.call_args[1].get("json")
    assert payload == {"type": None}, (
        f"drop must send {{type: null}}, got {payload}"
    )


def test_sync_drop_index_url():
    """drop_collection_index must POST to /v1/namespaces/{name}/index."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response({"collection": "col", "status": "none"})
    )

    client.drop_collection_index("col")

    call_url = client._t.post.call_args[0][0]
    assert call_url.endswith("/v1/namespaces/col/index")


# ── error scenario tests (Phase 4.2) ──────────────────────────────────────────


def test_sync_create_raises_on_conflict_409():
    """create_collection_index propagates HTTP 409 (already building)."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response(
            {"error": "a build is already in progress"}, status=409
        )
    )

    with pytest.raises((requests.HTTPError, Exception)):
        client.create_collection_index("docs", "hnsw")


def test_sync_create_raises_on_cluster_501():
    """create_collection_index propagates HTTP 501 (cluster mode, ANN unsupported)."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response(
            {
                "error": "ANN index management is not yet supported in cluster mode",
                "note": "cluster nodes use exact brute-force search",
            },
            status=501,
        )
    )

    with pytest.raises((requests.HTTPError, Exception)):
        client.create_collection_index("docs", "hnsw")


def test_sync_create_raises_on_not_found_404():
    """create_collection_index propagates HTTP 404 (unknown collection)."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response(
            {"error": "collection 'ghost' not found"}, status=404
        )
    )

    with pytest.raises((requests.HTTPError, Exception)):
        client.create_collection_index("ghost", "hnsw")


def test_sync_drop_raises_on_not_found_404():
    """drop_collection_index propagates HTTP 404 for unknown collection."""
    client = _sync_client()
    client._t.post = MagicMock(
        return_value=_make_post_response(
            {"error": "collection 'ghost' not found"}, status=404
        )
    )

    with pytest.raises((requests.HTTPError, Exception)):
        client.drop_collection_index("ghost")


# ── status model validation tests (Phase 4.2) ─────────────────────────────────


def test_sync_status_model_building_with_active():
    """
    Status response for a building state should carry both active and
    building generation fields — the active index keeps serving while
    the new one builds.
    """
    building_body = {
        "collection": "docs",
        "active_type": "hnsw",
        "active_generation": 7,
        "desired_type": "ivf",
        "status": "building",
        "building_generation": 8,
        "base_lsn": 100000,
        "build_started_at": 1700000000,
    }
    client = _sync_client()
    client._t.get = MagicMock(return_value=_make_get_response(building_body))

    result = client.collection_index_status("docs")

    assert result["status"] == "building"
    assert result["active_type"] == "hnsw"
    assert result["active_generation"] == 7
    assert result["desired_type"] == "ivf"
    assert result["building_generation"] == 8
    assert result["base_lsn"] == 100000


def test_sync_status_model_active():
    """Active state carries active_type and active_generation; no build fields."""
    body = {
        "collection": "docs",
        "active_type": "hnsw",
        "active_generation": 7,
        "status": "active",
    }
    client = _sync_client()
    client._t.get = MagicMock(return_value=_make_get_response(body))

    result = client.collection_index_status("docs")
    assert result["status"] == "active"
    assert result["active_type"] == "hnsw"
    assert result["active_generation"] == 7
    assert "building_generation" not in result


def test_sync_status_model_failed_with_error():
    """Failed state should include the error message from the backend."""
    body = {
        "collection": "docs",
        "active_type": "hnsw",       # previous active still serving
        "active_generation": 7,
        "status": "failed",
        "error": "IVF training failed: not enough records (need ≥ n_list)",
    }
    client = _sync_client()
    client._t.get = MagicMock(return_value=_make_get_response(body))

    result = client.collection_index_status("docs")
    assert result["status"] == "failed"
    assert result["active_type"] == "hnsw"  # old index still active
    assert "error" in result
    assert "IVF" in result["error"]


def test_sync_status_model_none():
    """None state: active_type is 'none', status is 'none', no generation fields."""
    body = {
        "collection": "docs",
        "active_type": "none",
        "status": "none",
    }
    client = _sync_client()
    client._t.get = MagicMock(return_value=_make_get_response(body))

    result = client.collection_index_status("docs")
    assert result["status"] == "none"
    assert result["active_type"] == "none"
    assert result.get("active_generation") is None
    assert result.get("building_generation") is None


# ── async variants ─────────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_async_collection_index_status_url():
    client = AsyncRemoteClient("http://localhost:3000")
    body = {"collection": "docs", "status": "active", "index_type": "hnsw"}

    async def fake_get(url, **kwargs):
        r = MagicMock()
        r.status_code = 200
        r.json = MagicMock(return_value=body)
        r.raise_for_status = MagicMock()
        fake_get.captured_url = url
        return r

    client._t.get = fake_get
    result = await client.collection_index_status("docs")
    assert fake_get.captured_url.endswith("/v1/namespaces/docs/index")
    assert result == body


@pytest.mark.asyncio
async def test_async_create_index_payload():
    client = AsyncRemoteClient("http://localhost:3000")
    captured = {}

    async def fake_post(url, **kwargs):
        captured["url"] = url
        captured["json"] = kwargs.get("json")
        r = MagicMock()
        r.status_code = 202
        r.json = MagicMock(return_value={"collection": "col", "generation": 0})
        r.raise_for_status = MagicMock()
        return r

    client._t.post = fake_post
    await client.create_collection_index("col", "hnsw", parameters={"m": 16})
    assert captured["url"].endswith("/v1/namespaces/col/index")
    assert captured["json"] == {"type": "hnsw", "parameters": {"m": 16}}


@pytest.mark.asyncio
async def test_async_drop_index_payload():
    client = AsyncRemoteClient("http://localhost:3000")
    captured = {}

    async def fake_post(url, **kwargs):
        captured["url"] = url
        captured["json"] = kwargs.get("json")
        r = MagicMock()
        r.status_code = 202
        r.json = MagicMock(return_value={"collection": "col", "status": "none"})
        r.raise_for_status = MagicMock()
        return r

    client._t.post = fake_post
    await client.drop_collection_index("col")
    assert captured["json"] == {"type": None}


@pytest.mark.asyncio
async def test_async_create_raises_on_cluster_501():
    """Async create_collection_index also propagates 501 from cluster nodes."""
    client = AsyncRemoteClient("http://localhost:3000")

    async def fake_post(url, **kwargs):
        r = MagicMock()
        r.status_code = 501
        r.json = MagicMock(return_value={"error": "ANN not supported in cluster mode"})
        r.raise_for_status.side_effect = Exception("501")
        return r

    client._t.post = fake_post
    with pytest.raises(Exception):
        await client.create_collection_index("docs", "hnsw")


@pytest.mark.asyncio
async def test_async_status_model_building():
    """Async collection_index_status returns building payload verbatim."""
    client = AsyncRemoteClient("http://localhost:3000")
    body = {
        "collection": "docs",
        "active_type": "hnsw",
        "active_generation": 3,
        "desired_type": "ivf",
        "status": "building",
        "building_generation": 4,
    }

    async def fake_get(url, **kwargs):
        r = MagicMock()
        r.status_code = 200
        r.json = MagicMock(return_value=body)
        r.raise_for_status = MagicMock()
        return r

    client._t.get = fake_get
    result = await client.collection_index_status("docs")
    assert result["status"] == "building"
    assert result["active_generation"] == 3
    assert result["building_generation"] == 4
