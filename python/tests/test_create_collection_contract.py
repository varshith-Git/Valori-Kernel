# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Phase 3.3 §17 — pins the exact wire contract `create_collection` sends.

No live node required: `_t.post_rpc` is stubbed to capture the payload
instead of making an HTTP call. This is what the Phase 3.2 audit found
missing — the SDK method itself was already correct, but nothing actually
exercised it.
"""
from unittest.mock import MagicMock

from valoricore.remote import SyncRemoteClient, AsyncRemoteClient


def _client_with_captured_payload():
    client = SyncRemoteClient("http://localhost:3000")
    client._t.post_rpc = MagicMock(return_value={"name": "x", "id": 0, "created": True})
    return client


def test_create_collection_with_dimension_and_metric():
    client = _client_with_captured_payload()
    client.create_collection("docs", dimension=768, metric="squared_l2")

    client._t.post_rpc.assert_called_once_with(
        "/v1/namespaces",
        {"name": "docs", "dimension": 768, "metric": "squared_l2"},
    )


def test_create_collection_with_optional_index():
    client = _client_with_captured_payload()
    client.create_collection("images", dimension=768, metric="squared_l2", index="hnsw")

    client._t.post_rpc.assert_called_once_with(
        "/v1/namespaces",
        {"name": "images", "dimension": 768, "metric": "squared_l2", "index": "hnsw"},
    )


def test_index_none_omitted_from_payload():
    """`index=None` (the default) must not serialize as `{"index": null}` —
    the REST API expects the key to be absent, not present-and-null, since
    "index" is optional and its absence (not `null`) means "no dedicated
    ANN structure" (Phase 3.2/3.3 contract)."""
    client = _client_with_captured_payload()
    client.create_collection("notes", dimension=384, metric="squared_l2")

    sent_payload = client._t.post_rpc.call_args[0][1]
    assert "index" not in sent_payload, f"index must be omitted, not null: {sent_payload}"


def test_default_name_carries_no_special_casing():
    """Phase 3.3: "default" is an ordinary name — the SDK must send its
    dimension/metric exactly like any other collection, never omitting them
    or special-casing the string "default" client-side."""
    client = _client_with_captured_payload()
    client.create_collection("default", dimension=4, metric="squared_l2")

    client._t.post_rpc.assert_called_once_with(
        "/v1/namespaces",
        {"name": "default", "dimension": 4, "metric": "squared_l2"},
    )


async def test_async_create_collection_matches_sync_payload_shape():
    client = AsyncRemoteClient("http://localhost:3000")
    captured = {}

    async def capturing_post_rpc(path, payload):
        captured["path"] = path
        captured["payload"] = payload
        return {"name": "docs", "id": 0, "created": True}

    client._t.post_rpc = capturing_post_rpc
    await client.create_collection("docs", dimension=768, metric="squared_l2", index="hnsw")

    assert captured["path"] == "/v1/namespaces"
    assert captured["payload"] == {
        "name": "docs",
        "dimension": 768,
        "metric": "squared_l2",
        "index": "hnsw",
    }
