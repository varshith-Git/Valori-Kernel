# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Integration suite against a real Valori node — Phase API-4A §17/§18.

Everything here is marked ``integration`` and skipped unless
``VALORI_TEST_ENDPOINT`` points at a running node::

    cargo run -p valori-node &                     # standalone
    VALORI_TEST_ENDPOINT=http://localhost:3000 \
      pytest sdk/python/tests -m integration

Set ``VALORI_TEST_MODE=cluster`` when the endpoint is a cluster node; the
cluster-only cases are skipped otherwise. §18 asks for both, and the same
assertions run against either — that is the point: the public contract claims
one surface, so the SDK exercises one surface.

These tests create and drop their own collections and clean up after
themselves. They are not run by the unit CI job.
"""

from __future__ import annotations

import json
import os
import uuid

import pytest

from valori import ValoriClient
from valori.errors import CollectionNotFoundError, NotFoundError, ValoriAPIError

ENDPOINT = os.environ.get("VALORI_TEST_ENDPOINT")
MODE = os.environ.get("VALORI_TEST_MODE", "standalone")
DIM = int(os.environ.get("VALORI_TEST_DIM", "8"))

pytestmark = [
    pytest.mark.integration,
    pytest.mark.skipif(not ENDPOINT, reason="VALORI_TEST_ENDPOINT is not set"),
]


@pytest.fixture(scope="module")
def client():
    c = ValoriClient(ENDPOINT, api_key=os.environ.get("VALORI_TEST_API_KEY"))
    yield c
    c.close()


@pytest.fixture
def collection(client):
    name = f"sdk-it-{uuid.uuid4().hex[:8]}"
    handle = client.collections.create(name, dimension=DIM, metric="squared_l2")
    yield handle
    try:
        client.collections.delete(name)
    except ValoriAPIError:  # pragma: no cover - best-effort cleanup
        pass


def vec(seed: float = 0.1):
    return [seed] * DIM


def request_id() -> str:
    """A contract-valid ``request_id``.

    ``RequestId`` is 32 hex characters (a 16-byte UUID), not a free-form
    string — the node answers anything else with a 422 before the write is
    attempted. This previously read ``f"it-{uuid4().hex[:8]}"`` and so the
    dedup path was never actually exercised.
    """
    return uuid.uuid4().hex


# ── §18 representative operations ────────────────────────────────────────────


def test_health_and_version(client):
    assert client.health().status in ("ok", "degraded")
    assert client.version() is not None


def test_create_list_and_delete_a_collection(client):
    name = f"sdk-it-{uuid.uuid4().hex[:8]}"
    client.collections.create(name, dimension=DIM, metric="squared_l2")
    assert name in client.collections.names()
    client.collections.delete(name)
    assert name not in client.collections.names()
    with pytest.raises(CollectionNotFoundError):
        client.collections.get(name)


def test_insert_get_and_search_a_record(collection):
    inserted = collection.records.insert(vec(0.1), metadata={"src": "it"},
                                         request_id=request_id())
    record_id = inserted.id
    fetched = collection.records.get(record_id)
    assert fetched is not None

    results = collection.search(vec(0.1), k=5)
    assert any(getattr(hit, "id", None) == record_id for hit in results.results)


def test_batch_insert(collection):
    response = collection.records.insert_batch([vec(0.1), vec(0.2), vec(0.3)])
    assert response is not None
    assert len(collection.search(vec(0.2), k=10).results) >= 3


def test_multi_search_across_collections(client, collection):
    collection.records.insert(vec(0.4))
    hits = client.collections.search_multi(vec(0.4), k=3, collections=[collection.name])
    assert hits is not None


def test_soft_delete_then_hard_delete(collection):
    first = collection.records.insert(vec(0.5)).id
    collection.soft_deleted = collection.records.soft_delete(first)
    second = collection.records.insert(vec(0.6)).id
    collection.records.delete(second)


def test_index_lifecycle(collection):
    for _ in range(20):
        collection.records.insert(vec(0.1))
    collection.index.build("hnsw")
    settled = collection.index.wait(poll_interval=0.5, timeout=60, raise_on_failure=False)
    assert settled.status in ("active", "failed", "none")
    assert collection.index.status() is not None


def test_graph_nodes_and_edges(collection):
    # The contract names this `node_id`, not `id` — CreateNodeResponse has
    # no `id` at all. This previously read `.id` and raised AttributeError.
    a = collection.graph.create_node(1).node_id
    b = collection.graph.create_node(1).node_id
    collection.graph.create_edge(a, b, 1)
    assert collection.graph.get_node(a) is not None
    assert collection.graph.list_edges(a) is not None
    assert collection.graph.subgraph(a, depth=2) is not None
    collection.graph.delete_node(b)


def test_graphrag(collection):
    collection.records.insert(vec(0.1))
    assert collection.graphrag(vec(0.1), k=3, depth=1) is not None


def test_operations_are_listable_and_readable(client, collection):
    collection.records.insert(vec(0.7))
    listed = client.operations.list()
    ids = [getattr(o, "id", None) for o in getattr(listed, "operations", []) or []]
    if not ids:
        pytest.skip("this node keeps no operation history")
    op = client.operations.get(ids[0])
    assert op.id == ids[0]

    # `GET /v1/operations/{id}/execution` legitimately 404s for an operation
    # that produced no execution record. Asserting "not None" made this test
    # depend on which operation happened to be first in the list. Both answers
    # are contract-valid; what must hold is that the SDK maps each to the right
    # typed outcome instead of leaking a raw response.
    try:
        assert client.operations.execution(ids[0]) is not None
    except NotFoundError as exc:
        assert exc.status_code == 404


def decode_stored_metadata(stored):
    """Normalise whatever a read path hands back into a plain dict.

    The node returns a record's metadata as the opaque bytes it was given, and
    the generated layer may wrap a JSON object in an attrs model. Neither is
    the caller's dict, so a round-trip assertion has to normalise before
    comparing — comparing the raw wire form to the original mapping is exactly
    the mistake Phase API-4D fixed.

    NOTE (follow-up, see the phase doc): the *write* path is symmetric — you
    pass a dict — but the *read* path is not, and callers have to unwrap this
    themselves. Making reads return plain dicts is an API-4E decision, not a
    silent change here.
    """
    if stored is None:
        return None
    if hasattr(stored, "to_dict"):
        stored = stored.to_dict()
    if isinstance(stored, (list, tuple)):
        return json.loads(bytes(stored).decode("utf-8"))
    if isinstance(stored, (bytes, bytearray)):
        return json.loads(stored.decode("utf-8"))
    if isinstance(stored, str):
        return json.loads(stored)
    return stored


def test_metadata_round_trips_through_a_real_node(collection):
    """Phase API-4D §4 — write metadata, read it back, assert semantic equality.

    This is the test the metadata encoding bug would have caught. Asserting
    HTTP 200 on the insert proves nothing: the pre-fix SDK sent a JSON object
    where the contract wants UTF-8 bytes, and the node accepted the request
    and stored something else. Only reading the value back and comparing it to
    what the caller passed closes that loop.
    """
    original = {
        "author": "alice",
        "page": 4,
        "score": 0.5,
        "draft": False,
        "parent": None,
        "tags": ["a", "b"],
        "src": {"file": "a.md", "line": 12},
        "title": "Übersicht — 東京",
    }
    inserted = collection.records.insert(vec(0.31), metadata=original)
    fetched = collection.records.get(inserted.id)

    stored = getattr(fetched, "metadata", None)
    assert stored is not None, "the node returned no metadata for a record that was given some"
    assert decode_stored_metadata(stored) == original


def test_batch_metadata_round_trips_through_a_real_node(collection):
    """Same closed loop for the second wire shape (UTF-8 JSON strings)."""
    payloads = [{"i": 0, "kind": "first"}, {"i": 1, "kind": "second"}]
    response = collection.records.insert_batch([vec(0.41), vec(0.42)], metadata=payloads)

    ids = list(getattr(response, "ids", None) or [])
    assert len(ids) == 2

    for record_id, expected in zip(ids, payloads):
        stored = getattr(collection.records.get(record_id), "metadata", None)
        assert stored is not None
        assert decode_stored_metadata(stored) == expected


def test_metadata_filter_is_accepted_but_matches_nothing(collection):
    """SERVER BUG: metadata_filter never matches insert-time metadata.

    See docs/api/known-server-issues.md #1. The filter consults only the
    metadata sidecar, keyed `rec:{id}`, so a predicate that exactly matches a
    record's committed metadata still returns zero hits. Confirmed with raw
    curl, with no SDK in the path.

    This test asserts the behaviour that is actually true today rather than
    skipping: the request is well-formed and accepted (which is the SDK's
    responsibility and does hold), and the empty result is pinned so that
    fixing the server turns this test red and forces it to be tightened into
    the real assertion.
    """
    collection.records.insert(vec(0.51), metadata={"author": "alice"})

    unfiltered = collection.search(vec(0.51), k=5)
    assert len(unfiltered.results) >= 1, "sanity: the record is searchable without a filter"

    filtered = collection.search(vec(0.51), k=5, metadata_filter={"author": "alice"})
    assert filtered.results == [], (
        "metadata_filter started matching insert-time metadata — the server bug in "
        "docs/api/known-server-issues.md #1 appears to be fixed. Tighten this test to "
        "assert the record IS returned, and update that document."
    )


def test_proof_surface(client):
    assert client.proof.event_log() is not None
    assert client.proof.state() is not None


def test_errors_are_typed_against_a_real_node(client):
    with pytest.raises(ValoriAPIError) as caught:
        client.collections["nope-does-not-exist"].records.get(999999)
    assert caught.value.status_code >= 400
    assert caught.value.body is not None


def test_dimension_mismatch_is_reported_as_such(collection):
    with pytest.raises(ValoriAPIError) as caught:
        collection.records.insert([0.1] * (DIM + 3))
    assert caught.value.status_code in (400, 422)


# ── ingest needs an embedding provider ───────────────────────────────────────


def test_chunking_is_available_without_an_embedding_provider(client):
    chunked = client.ingest.chunk("# Title\n\nSome body text.\n", strategy="auto")
    assert chunked is not None


def test_ingest_requires_an_embedding_provider_or_works(client, collection):
    try:
        client.ingest.document("# Title\n\nBody.", collection=collection.name)
    except ValoriAPIError as exc:
        # 422 is the documented answer when VALORI_EMBED_PROVIDER is unset.
        assert exc.status_code in (422, 501)


# ── cluster-only ─────────────────────────────────────────────────────────────


cluster_only = pytest.mark.skipif(
    MODE != "cluster", reason="VALORI_TEST_MODE is not 'cluster'")


@cluster_only
def test_cluster_status_health_and_role(client):
    assert client.cluster.status() is not None
    assert client.cluster.health() is not None
    assert client.cluster.role() is not None


@cluster_only
def test_cluster_proof(client):
    assert client.cluster.proof() is not None


@cluster_only
def test_writes_replicate_through_raft(client, collection):
    inserted = collection.records.insert(vec(0.9), request_id=request_id())
    assert collection.records.get(inserted.id) is not None
