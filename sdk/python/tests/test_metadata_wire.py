# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""Phase API-4D §2/§11 — metadata wire-encoding regression suite.

Before API-4D the Python SDK passed a caller's ``metadata`` mapping straight
into the generated request model, whose permissive ``from_dict`` accepted it
and emitted a JSON *object*. The contract says:

* ``POST /v1/records``              → ``list[int]``  (UTF-8 JSON bytes)
* ``POST /v1/vectors/batch-insert`` → ``list[str]``  (UTF-8 JSON strings)
* memory upsert / sidecar / filter  → a JSON object, verbatim

Every assertion below inspects the bytes that actually left the SDK, so each
one fails against the pre-fix implementation.
"""

from __future__ import annotations

import json

import httpx
import pytest

from valori._wire import (
    encode_metadata_bytes,
    encode_metadata_filter,
    encode_metadata_object,
    encode_metadata_string,
    encode_metadata_string_list,
)

from .conftest import REPO_ROOT, Recorder, make_client

# Phase API-4D §7 — the same table the TypeScript suite reads. Both SDKs must
# produce these exact bytes; see the file's own header for why.
FIXTURES = json.loads(
    (REPO_ROOT / "sdk" / "metadata-wire-fixtures.json").read_text(encoding="utf-8")
)["cases"]


def anything(_request: httpx.Request) -> httpx.Response:
    """204, no body.

    These cases assert on what the SDK *sent*; fabricating a response fixture
    for each model would be fiction. Mirrors ``test_resources.anything``.
    """
    return httpx.Response(204)


def sent_by(fn) -> Recorder:
    """Run ``fn(client)`` and hand back the recorder."""
    client = make_client(anything)
    fn(client)
    return client._recorder


# ── the encoders in isolation ────────────────────────────────────────────────

CASES = [
    ("string", {"author": "alice"}),
    ("number-int", {"page": 4}),
    ("number-float", {"score": 0.5}),
    ("boolean", {"draft": False}),
    ("null-value", {"parent": None}),
    ("nested-object", {"src": {"file": "a.md", "line": 12}}),
    ("array", {"tags": ["a", "b", "c"]}),
    ("nested-array-of-objects", {"spans": [{"s": 0, "e": 4}, {"s": 5, "e": 9}]}),
    ("unicode", {"title": "Übersicht — 東京 🗼"}),
    ("empty", {}),
    ("mixed", {"a": 1, "b": "two", "c": [3, {"d": True}], "e": None}),
]


@pytest.mark.parametrize("name,value", CASES, ids=[c[0] for c in CASES])
def test_encode_metadata_bytes_roundtrips(name, value):
    encoded = encode_metadata_bytes(value)
    assert isinstance(encoded, list)
    assert all(isinstance(b, int) and 0 <= b <= 255 for b in encoded)
    assert json.loads(bytes(encoded).decode("utf-8")) == value


@pytest.mark.parametrize("name,value", CASES, ids=[c[0] for c in CASES])
def test_encode_metadata_string_roundtrips(name, value):
    encoded = encode_metadata_string(value)
    assert isinstance(encoded, str)
    assert json.loads(encoded) == value


def test_encoders_are_byte_identical_to_each_other():
    """The two shapes must be the same JSON text — one is just the other's bytes."""
    for _, value in CASES:
        assert bytes(encode_metadata_bytes(value)).decode("utf-8") == encode_metadata_string(value)


def test_serialisation_matches_javascript_json_stringify():
    """Cross-SDK byte identity — the audit chain commits these bytes.

    ``JSON.stringify`` emits no whitespace, real UTF-8, and insertion order.
    A Python default ``json.dumps`` would emit ``", "`` / ``": "`` separators
    and ``\\uXXXX`` escapes, producing a different event hash for the same
    metadata written from the TypeScript SDK.
    """
    value = {"b": 1, "a": "é", "n": [1, 2]}
    text = encode_metadata_string(value)
    assert text == '{"b":1,"a":"é","n":[1,2]}'
    assert " " not in text
    assert "\\u" not in text


def test_unicode_is_encoded_as_utf8_bytes_not_escapes():
    encoded = encode_metadata_bytes({"t": "東"})
    # U+6771 is 3 bytes in UTF-8; escaping would have produced 6 ASCII chars.
    assert bytes([0xE6, 0x9D, 0xB1]) in bytes(encoded)


def test_empty_metadata_is_sent_not_dropped():
    """``{}`` is a real value and must reach the wire; only ``None`` is 'absent'."""
    assert encode_metadata_bytes({}) == [0x7B, 0x7D]  # b"{}"
    assert encode_metadata_string({}) == "{}"
    assert encode_metadata_bytes(None) is None
    assert encode_metadata_string(None) is None


def test_batch_list_preserves_null_entries():
    assert encode_metadata_string_list([{"a": 1}, None, {}]) == ['{"a":1}', None, "{}"]
    assert encode_metadata_string_list(None) is None


def test_object_shaped_encoders_do_not_transform():
    value = {"author": "alice", "year": {"gte": 2020}}
    assert encode_metadata_object(value) == value
    assert encode_metadata_filter(value) == value
    assert encode_metadata_filter(None) is None


@pytest.mark.parametrize(
    "bad",
    [
        pytest.param(["not", "a", "mapping"], id="list"),
        pytest.param("a string", id="str"),
        pytest.param(7, id="int"),
    ],
)
def test_non_mapping_metadata_is_rejected_at_the_boundary(bad):
    with pytest.raises(TypeError):
        encode_metadata_bytes(bad)


def test_non_json_serialisable_metadata_is_rejected():
    with pytest.raises(TypeError):
        encode_metadata_bytes({"when": object()})


def test_nan_is_rejected_rather_than_emitting_invalid_json():
    """``json.dumps`` would happily emit bare ``NaN``, which is not JSON."""
    with pytest.raises(TypeError):
        encode_metadata_bytes({"x": float("nan")})


def test_non_string_keys_are_rejected():
    with pytest.raises(TypeError):
        encode_metadata_bytes({1: "one"})


# ── end-to-end: what actually leaves the SDK ─────────────────────────────────


def test_insert_sends_metadata_as_byte_array():
    rec = sent_by(
        lambda client: client.collection("docs").records.insert(
            [0.1, 0.2], metadata={"author": "alice", "page": 4}
        )
    )

    wire = rec.body["metadata"]
    assert isinstance(wire, list), f"metadata must be a byte array, got {type(wire).__name__}"
    assert all(isinstance(b, int) for b in wire)
    assert json.loads(bytes(wire).decode("utf-8")) == {"author": "alice", "page": 4}


def test_insert_omits_metadata_when_not_supplied():
    rec = sent_by(lambda client: client.collection("docs").records.insert([0.1, 0.2]))
    assert "metadata" not in rec.body


def test_insert_sends_empty_metadata_object():
    rec = sent_by(lambda client: client.collection("docs").records.insert([0.1], metadata={}))
    assert bytes(rec.body["metadata"]).decode("utf-8") == "{}"


def test_insert_batch_sends_metadata_as_string_array():
    rec = sent_by(
        lambda client: client.collection("docs").records.insert_batch(
            [[0.1], [0.2]], metadata=[{"i": 0}, {"i": 1}]
        )
    )

    wire = rec.body["metadata"]
    assert isinstance(wire, list)
    assert all(isinstance(s, str) for s in wire), f"batch metadata must be JSON strings, got {wire}"
    assert [json.loads(s) for s in wire] == [{"i": 0}, {"i": 1}]


def test_insert_batch_preserves_null_metadata_entry():
    rec = sent_by(lambda client: client.collection("docs").records.insert_batch([[0.1], [0.2]], metadata=[{"i": 0}, None]))
    assert rec.body["metadata"] == ['{"i":0}', None]


def test_memory_upsert_sends_metadata_as_object():
    rec = sent_by(lambda client: client.collection("docs").memory.upsert([0.1], metadata={"role": "note"}))
    assert rec.body["metadata"] == {"role": "note"}


def test_update_metadata_sends_object():
    rec = sent_by(lambda client: client.collection("docs").records.update_metadata(7, {"author": "alice", "page": 4}))
    assert rec.body == {"author": "alice", "page": 4}


def test_set_metadata_sidecar_sends_object():
    rec = sent_by(lambda client: client.collection("docs").memory.set_metadata("rec:7", {"a": 1}))
    assert rec.body["metadata"] == {"a": 1}


def test_search_metadata_filter_sent_verbatim():
    rec = sent_by(lambda client: client.collection("docs").search([0.1], k=3, metadata_filter={"author": "alice", "year": {"gte": 2020}}))
    assert rec.body["metadata_filter"] == {"author": "alice", "year": {"gte": 2020}}


def test_search_multi_metadata_filter_sent_verbatim():
    rec = sent_by(lambda client: client.collections.search_multi([0.1], k=3, collections=["a", "b"], metadata_filter={"x": 1}))
    assert rec.body["metadata_filter"] == {"x": 1}


def test_memory_search_metadata_filter_sent_verbatim():
    rec = sent_by(lambda client: client.collection("docs").memory.search([0.1], k=3, metadata_filter={"x": 1}))
    assert rec.body["metadata_filter"] == {"x": 1}


def test_bad_metadata_fails_before_any_request_is_made():
    """Validation happens in the SDK, not as a 400 from the server."""
    client = make_client(anything)
    with pytest.raises(TypeError):
        client.collection("docs").records.insert([0.1], metadata=["nope"])
    assert client._recorder.count == 0


# ── cross-SDK parity (§7) ────────────────────────────────────────────────────


def test_fixture_file_is_present_and_non_trivial():
    assert len(FIXTURES) >= 10


@pytest.mark.parametrize("fx", FIXTURES, ids=[f["name"] for f in FIXTURES])
def test_records_insert_matches_the_shared_fixture(fx):
    """The bytes POST /v1/records commits must match the TypeScript SDK's."""
    rec = sent_by(
        lambda client: client.collection("docs").records.insert([0.1], metadata=fx["metadata"])
    )
    assert rec.body["metadata"] == fx["bytes"]
    assert bytes(rec.body["metadata"]).decode("utf-8") == fx["json"]


@pytest.mark.parametrize("fx", FIXTURES, ids=[f["name"] for f in FIXTURES])
def test_batch_insert_matches_the_shared_fixture(fx):
    rec = sent_by(
        lambda client: client.collection("docs").records.insert_batch(
            [[0.1]], metadata=[fx["metadata"]]
        )
    )
    assert rec.body["metadata"] == [fx["json"]]


def test_key_insertion_order_is_preserved_not_sorted():
    """A sorted serialiser would change the committed bytes, and so the state hash."""
    rec = sent_by(
        lambda client: client.collection("docs").records.insert([0.1], metadata={"b": 1, "a": 2})
    )
    assert bytes(rec.body["metadata"]).decode("utf-8") == '{"b":1,"a":2}'
