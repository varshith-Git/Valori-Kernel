# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Handwritten wrapper tests — Phase API-4A §17/§18.

Each case drives a wrapper end to end and asserts on what actually reached the
wire: method, path, query string and JSON body. That is the property that
matters — a wrapper is only correct if it hits the operation the coverage
manifest claims it hits.
"""

from __future__ import annotations

import io
import json

import httpx
import pytest

from valori.errors import CollectionNotFoundError, ValoriConfigError

from .conftest import (
    COLLECTIONS_OK, HEALTH_OK, INSERT_OK, SEARCH_OK, TREE_INDEX, TREE_RECEIPT,
    make_client)


def anything(_request: httpx.Request) -> httpx.Response:
    """Answer with no content.

    These cases assert on what the SDK *sent*, so the response body is
    irrelevant — and a bodiless 204 avoids fabricating a plausible-looking
    payload for forty different response models, which would be fiction the
    reader might mistake for a contract fixture.
    """
    return httpx.Response(204)


def call(fn, *, response=anything, **client_kwargs):
    """Run `fn(client)` and hand back the recorder."""
    client = make_client(response, **client_kwargs)
    fn(client)
    return client._recorder


# ── collections ──────────────────────────────────────────────────────────────


def test_create_collection_posts_the_required_triple():
    rec = call(lambda c: c.collections.create("docs", dimension=384, metric="squared_l2"))
    assert rec.last.method == "POST"
    assert rec.last.url.path == "/v1/namespaces"
    assert rec.body == {"name": "docs", "dimension": 384, "metric": "squared_l2"}


def test_create_collection_passes_an_optional_index_through():
    rec = call(lambda c: c.collections.create("d", dimension=3, metric="cosine", index="hnsw"))
    assert rec.body["index"] == "hnsw"


def test_create_collection_returns_a_usable_handle():
    client = make_client(anything)
    handle = client.collections.create("docs", dimension=3, metric="cosine")
    assert handle.name == "docs"
    assert handle.records.collection == "docs"


def test_omitted_optionals_are_absent_from_the_body_not_null():
    rec = call(lambda c: c.collections.create("d", dimension=3, metric="cosine"))
    assert "index" not in rec.body


def test_list_collections_is_a_get():
    rec = call(lambda c: c.collections.list(), response=lambda r: httpx.Response(200, json=COLLECTIONS_OK))
    assert rec.last.method == "GET"
    assert rec.last.url.path == "/v1/namespaces"


def test_delete_collection_uses_the_name_in_the_path():
    rec = call(lambda c: c.collections.delete("docs"))
    assert rec.last.method == "DELETE"
    assert rec.last.url.path == "/v1/namespaces/docs"


def test_getitem_is_a_handle_and_makes_no_request():
    client = make_client(anything)
    handle = client.collections["docs"]
    assert handle.name == "docs"
    assert client._recorder.count == 0


def test_get_verifies_existence_and_raises_when_absent():
    client = make_client(lambda r: httpx.Response(200, json=COLLECTIONS_OK))
    assert client.collections.get("docs").name == "docs"
    with pytest.raises(CollectionNotFoundError):
        client.collections.get("ghost")


def test_contains_uses_the_listing():
    client = make_client(lambda r: httpx.Response(200, json=COLLECTIONS_OK))
    assert "docs" in client.collections
    assert "ghost" not in client.collections


# ── records ──────────────────────────────────────────────────────────────────


def test_insert_carries_the_collection_from_the_handle():
    rec = call(lambda c: c.collections["docs"].records.insert([0.1, 0.2], metadata={"a": 1}),
               response=lambda r: httpx.Response(200, json=INSERT_OK))
    assert rec.last.url.path == "/v1/records"
    assert rec.body["collection"] == "docs"
    assert rec.body["values"] == [0.1, 0.2]
    # API-4D: `POST /v1/records` takes opaque UTF-8 JSON *bytes*, not an object.
    # This assertion previously read `== {"a": 1}` and so encoded the bug.
    assert json.loads(bytes(rec.body["metadata"]).decode("utf-8")) == {"a": 1}


def test_insert_sends_request_id_in_the_body_and_as_the_idempotency_header():
    rec = call(lambda c: c.collections["docs"].records.insert([0.1], request_id="r-1"),
               response=lambda r: httpx.Response(200, json=INSERT_OK))
    assert rec.body["request_id"] == "r-1"
    assert rec.last.headers["Idempotency-Key"] == "r-1"


def test_insert_batch_hits_the_batch_path():
    rec = call(lambda c: c.collections["docs"].records.insert_batch(
        [[0.1], [0.2]], texts=["a", "b"]))
    assert rec.last.url.path == "/v1/vectors/batch-insert"
    assert rec.body["batch"] == [[0.1], [0.2]]
    assert rec.body["texts"] == ["a", "b"]


def test_insert_encrypted_hits_the_encrypted_path():
    rec = call(lambda c: c.collections["docs"].records.insert_encrypted("cipher", key_id="k1"))
    assert rec.last.url.path == "/v1/records/encrypted"
    assert rec.body == {"payload": "cipher", "collection": "docs", "key_id": "k1"}


def test_get_record_puts_the_collection_in_the_query_string():
    rec = call(lambda c: c.collections["docs"].records.get(42))
    assert rec.last.method == "GET"
    assert rec.last.url.path == "/v1/records/42"
    assert dict(rec.last.url.params) == {"collection": "docs"}


def test_delete_and_soft_delete_hit_distinct_paths():
    assert call(lambda c: c.collections["d"].records.delete(1)).last.url.path == "/v1/delete"
    assert call(lambda c: c.collections["d"].records.soft_delete(1)).last.url.path == "/v1/soft-delete"


def test_update_metadata_is_a_patch_with_a_free_form_body():
    rec = call(lambda c: c.collections["docs"].records.update_metadata(9, {"tier": {"x": 1}}))
    assert rec.last.method == "PATCH"
    assert rec.last.url.path == "/v1/records/9/metadata"
    assert rec.body == {"tier": {"x": 1}}


# ── search ───────────────────────────────────────────────────────────────────


def test_search_sends_query_k_and_collection():
    rec = call(lambda c: c.collections["docs"].search([0.1, 0.2], k=5),
               response=lambda r: httpx.Response(200, json=SEARCH_OK))
    assert rec.last.url.path == "/v1/search"
    assert rec.body == {"query": [0.1, 0.2], "k": 5, "collection": "docs"}


def test_search_passes_the_optional_ranking_knobs():
    rec = call(lambda c: c.collections["docs"].search(
        [0.1], k=1, query_text="optimizer", rerank=True,
        decay_half_life_secs=86400, metadata_filter={"year": {"gte": 2020}},
        graph_rerank={"weight": 0.15}),
        response=lambda r: httpx.Response(200, json=SEARCH_OK))
    body = rec.body
    assert body["query_text"] == "optimizer"
    assert body["rerank"] is True
    assert body["decay_half_life_secs"] == 86400
    assert body["metadata_filter"] == {"year": {"gte": 2020}}
    assert body["graph_rerank"] == {"weight": 0.15}


def test_multi_search_is_not_scoped_to_one_collection():
    rec = call(lambda c: c.collections.search_multi([0.1], k=3, collections=["a", "b"]))
    assert rec.last.url.path == "/v1/search/multi"
    assert rec.body["collections"] == ["a", "b"]


def test_graphrag_uses_query_vector_not_query():
    rec = call(lambda c: c.collections["docs"].graphrag([0.1], k=5, depth=2))
    assert rec.last.url.path == "/v1/graphrag"
    assert rec.body == {"query_vector": [0.1], "collection": "docs", "k": 5, "depth": 2}


# ── index ────────────────────────────────────────────────────────────────────


def test_index_build_posts_to_the_collection_index_path():
    rec = call(lambda c: c.collections["docs"].index.build("hnsw", parameters={"m": 16}))
    assert rec.last.method == "POST"
    assert rec.last.url.path == "/v1/namespaces/docs/index"
    assert rec.body == {"type": "hnsw", "parameters": {"m": 16}}


def test_index_status_is_a_get_on_the_same_path():
    rec = call(lambda c: c.collections["docs"].index.status())
    assert rec.last.method == "GET"
    assert rec.last.url.path == "/v1/namespaces/docs/index"


def test_node_wide_index_config_and_rebuild():
    assert call(lambda c: c.index.config()).last.url.path == "/v1/index/config"
    rec = call(lambda c: c.index.rebuild("hnsw"))
    assert rec.last.url.path == "/v1/index/rebuild"
    assert rec.body == {"index": "hnsw"}


def test_index_operations_require_a_collection():
    client = make_client(anything)
    from valori.resources.index import CollectionIndex

    unscoped = CollectionIndex(client._transport, None)
    with pytest.raises(ValoriConfigError):
        unscoped.build("hnsw")
    with pytest.raises(ValoriConfigError):
        unscoped.status()


# ── graph ────────────────────────────────────────────────────────────────────


def test_create_edge_translates_from_node_to_the_from_wire_field():
    rec = call(lambda c: c.collections["docs"].graph.create_edge(1, 2, 7))
    assert rec.last.url.path == "/v1/graph/edge"
    assert rec.body == {"from": 1, "to": 2, "kind": 7, "collection": "docs"}


def test_create_node_and_node_reads():
    assert call(lambda c: c.collections["d"].graph.create_node(3)).last.url.path == "/v1/graph/node"
    assert call(lambda c: c.collections["d"].graph.get_node(5)).last.url.path == "/v1/graph/node/5"
    rec = call(lambda c: c.collections["d"].graph.delete_node(5))
    assert rec.last.method == "DELETE"
    assert rec.last.url.path == "/v1/graph/node/5"


def test_graph_listing_and_traversal_query_strings():
    rec = call(lambda c: c.collections["d"].graph.list_nodes(kind=2, offset=10, limit=5))
    assert rec.last.url.path == "/v1/graph/nodes"
    assert dict(rec.last.url.params) == {
        "collection": "d", "kind": "2", "offset": "10", "limit": "5"}

    rec = call(lambda c: c.collections["d"].graph.subgraph(9, depth=2))
    assert rec.last.url.path == "/v1/graph/subgraph"
    assert dict(rec.last.url.params) == {"root": "9", "depth": "2", "collection": "d"}

    rec = call(lambda c: c.collections["d"].graph.query(1, direction="out", limit=3))
    assert rec.last.url.path == "/v1/graph/query"
    assert dict(rec.last.url.params) == {
        "start": "1", "direction": "out", "limit": "3", "collection": "d"}

    assert call(lambda c: c.collections["d"].graph.list_edges(4)).last.url.path == "/v1/graph/edges/4"


def test_omitted_query_parameters_are_absent_from_the_url():
    rec = call(lambda c: c.collections["d"].graph.list_nodes())
    assert dict(rec.last.url.params) == {"collection": "d"}


# ── memory ───────────────────────────────────────────────────────────────────


def test_memory_upsert_and_upsert_vector_are_distinct_paths():
    assert call(lambda c: c.collections["d"].memory.upsert([0.1])).last.url.path == "/v1/memory/upsert"
    assert call(lambda c: c.collections["d"].memory.upsert_vector([0.1])).last.url.path == \
        "/v1/memory/upsert_vector"


def test_memory_search_and_search_vector_are_distinct_paths():
    assert call(lambda c: c.collections["d"].memory.search([0.1], 3)).last.url.path == "/v1/memory/search"
    assert call(lambda c: c.collections["d"].memory.search_vector([0.1], 3)).last.url.path == \
        "/v1/memory/search_vector"


def test_memory_search_explain_is_a_query_flag_not_a_body_field():
    rec = call(lambda c: c.collections["d"].memory.search([0.1], 3, explain=True))
    assert dict(rec.last.url.params) == {"explain": "true"}
    assert "explain" not in rec.body


def test_memory_maintenance_and_sidecar():
    rec = call(lambda c: c.collections["d"].memory.consolidate(7, [0.2]))
    assert rec.last.url.path == "/v1/memory/consolidate"
    assert rec.body == {"old_record_id": 7, "new_vector": [0.2], "collection": "d"}

    rec = call(lambda c: c.collections["d"].memory.contradict(3, 9, threshold=0.9))
    assert rec.body == {"record_a": 3, "record_b": 9, "threshold": 0.9, "collection": "d"}

    rec = call(lambda c: c.collections["d"].memory.get_metadata("mem-1"))
    assert rec.last.url.path == "/v1/memory/meta/get"
    assert dict(rec.last.url.params) == {"target_id": "mem-1"}

    rec = call(lambda c: c.collections["d"].memory.set_metadata("mem-1", {"k": {"v": 1}}))
    assert rec.last.url.path == "/v1/memory/meta/set"
    assert rec.body["target_id"] == "mem-1"


# ── node-scoped resources ────────────────────────────────────────────────────


@pytest.mark.parametrize("invoke,method,path", [
    (lambda c: c.meta.health(), "GET", "/health"),
    (lambda c: c.meta.version(), "GET", "/v1/version"),
    (lambda c: c.meta.usage(), "GET", "/v1/usage"),
    (lambda c: c.meta.models_health(), "GET", "/v1/models/health"),
    (lambda c: c.meta.shard_routing(), "GET", "/v1/shard/routing"),
    (lambda c: c.ingest.chunk("hello"), "POST", "/v1/ingest/document"),
    (lambda c: c.ingest.document("hello"), "POST", "/v1/ingest"),
    (lambda c: c.ingest.update(1, "hello"), "POST", "/v1/ingest/update"),
    (lambda c: c.ingest.status("job-1"), "GET", "/v1/ingest/status/job-1"),
    (lambda c: c.ingest.extract_entities("hello"), "POST", "/v1/ingest/extract-entities"),
    (lambda c: c.tree.build("# doc"), "POST", "/v1/tree/build"),
    (lambda c: c.tree.query("q"), "POST", "/v1/tree/query"),
    (lambda c: c.tree.hybrid("q"), "POST", "/v1/tree/hybrid"),
    (lambda c: c.tree.verify(TREE_INDEX, TREE_RECEIPT), "POST", "/v1/tree/verify"),
    (lambda c: c.tree.chain_verify([]), "POST", "/v1/tree/chain-verify"),
    (lambda c: c.community.detect(), "POST", "/v1/community/detect"),
    (lambda c: c.community.search([0.1]), "POST", "/v1/community/search"),
    (lambda c: c.community.overview(), "GET", "/v1/community/overview"),
    (lambda c: c.proof.event_log(), "GET", "/v1/proof/event-log"),
    (lambda c: c.proof.state(), "GET", "/v1/proof/state"),
    (lambda c: c.proof.receipt("r-1"), "GET", "/v1/proof/receipt/r-1"),
    (lambda c: c.proof.latest_receipt(), "GET", "/v1/proof/receipt"),
    (lambda c: c.proof.timeline(limit=10), "GET", "/v1/timeline"),
    (lambda c: c.snapshots.save(), "POST", "/v1/snapshot/save"),
    (lambda c: c.snapshots.restore("/tmp/s"), "POST", "/v1/snapshot/restore"),
    (lambda c: c.snapshots.download(), "GET", "/v1/snapshot/download"),
    (lambda c: c.storage.upload_snapshot(), "POST", "/v1/storage/snapshots/upload"),
    (lambda c: c.storage.restore_snapshot(), "POST", "/v1/storage/snapshots/restore"),
    (lambda c: c.storage.list_snapshots(), "GET", "/v1/storage/snapshots"),
    (lambda c: c.storage.manifest(), "GET", "/v1/storage/manifest"),
    (lambda c: c.storage.archive_wal("/tmp/w"), "POST", "/v1/storage/wal/archive"),
    (lambda c: c.storage.list_wal_segments(), "GET", "/v1/storage/wal"),
    (lambda c: c.cluster.status(), "GET", "/v1/cluster/status"),
    (lambda c: c.cluster.health(), "GET", "/v1/cluster/health"),
    (lambda c: c.cluster.role(), "GET", "/v1/cluster/role"),
    (lambda c: c.cluster.proof(), "GET", "/v1/cluster/proof"),
    (lambda c: c.crypto.key_status("k-1"), "GET", "/v1/crypto/status/k-1"),
    (lambda c: c.operations.list(), "GET", "/v1/operations"),
    (lambda c: c.operations.execution("op-1"), "GET", "/v1/operations/op-1/execution"),
])
def test_node_scoped_wrapper_hits_the_declared_operation(invoke, method, path):
    rec = call(invoke)
    assert rec.last.method == method
    assert rec.last.url.path == path


def test_background_ingest_maps_to_the_async_query_flag():
    rec = call(lambda c: c.ingest.document("hello", background=True))
    assert dict(rec.last.url.params) == {"async": "true"}


def test_snapshot_upload_sends_a_binary_body():
    rec = call(lambda c: c.snapshots.upload(io.BytesIO(b"SNAPBYTES"), file_name="s.bin"))
    assert rec.last.method == "POST"
    assert rec.last.url.path == "/v1/snapshot/upload"
    assert b"SNAPBYTES" in rec.last.content


def test_health_is_reachable_without_an_api_key():
    client = make_client(lambda r: httpx.Response(200, json=HEALTH_OK), api_key=None)
    assert client.health().status == "ok"


def test_client_exposes_the_generated_client_as_an_escape_hatch():
    client = make_client(anything)
    assert client.raw is client._transport.raw()
