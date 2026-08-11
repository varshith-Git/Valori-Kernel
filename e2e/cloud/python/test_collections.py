"""Collection (namespace) CRUD through the real SDK.

Index-type/dimension selection is deliberately NOT tested per-collection —
Valori's real model fixes dim/index at the PROJECT level (set once at
project creation), shared by every collection inside it. See
Collection/`_CollectionsResource.create()`'s own docstring
(python/valoricore/remote.py) for why `dimension=`/`index=` kwargs were
never added there — this test file doesn't invent them either.
"""


def test_create_get_list_drop(sdk_client_a):
    sdk_client_a.collections.create("coll-crud")
    assert "coll-crud" in sdk_client_a.collections.list()

    same = sdk_client_a.collections.get("coll-crud")
    assert same.name == "coll-crud"

    sdk_client_a.collections.create("coll-crud")  # idempotent by name
    assert sdk_client_a.collections.list().count("coll-crud") == 1

    same.drop()
    assert "coll-crud" not in sdk_client_a.collections.list()


def test_get_does_not_require_prior_create(sdk_client_a):
    # get() itself has no existence check — it's a pure client-side handle
    # construction, no request made. But the real node DOES require a
    # namespace to exist before /search is called against it (400 "unknown
    # collection ... create it first with POST /v1/namespaces") — an
    # earlier version of this test assumed search() on a never-created
    # collection would just return an empty list, which is not what the
    # real node does. Asserting the real behavior instead.
    import pytest
    from valoricore.exceptions import ValidationError

    handle = sdk_client_a.collections.get("never-created-yet")
    with pytest.raises(ValidationError):
        handle.search([0.1, 0.2, 0.3, 0.4], top_k=1)
