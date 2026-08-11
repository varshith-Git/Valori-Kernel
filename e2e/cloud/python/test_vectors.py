"""Vector insert/search/delete through the real SDK, real node, real
project-dim (4, set at E2E project creation) deterministic vectors."""


def test_insert_and_search_deterministic(sdk_client_a):
    c = sdk_client_a.collections.create("vectors-basic")
    ids = c.upsert(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.9, 0.1, 0.0, 0.0],
        ]
    )
    assert len(ids) == 3

    results = c.search([1.0, 0.0, 0.0, 0.0], top_k=2)
    assert len(results) == 2
    # The exact match (id[0]) must be the top result.
    assert results[0]["id"] == ids[0]
    assert results[0]["score"] == 0


def test_delete_removes_from_search(sdk_client_a):
    c = sdk_client_a.collections.create("vectors-delete")
    ids = c.upsert([[0.5, 0.5, 0.5, 0.5]])
    assert len(c.search([0.5, 0.5, 0.5, 0.5], top_k=1)) == 1

    c.delete(ids[0])

    after = c.search([0.5, 0.5, 0.5, 0.5], top_k=5)
    assert all(r["id"] != ids[0] for r in after)


def test_top_k_is_respected(sdk_client_a):
    c = sdk_client_a.collections.create("vectors-topk")
    c.upsert([[float(i), 0.0, 0.0, 0.0] for i in range(10)])
    results = c.search([0.0, 0.0, 0.0, 0.0], top_k=3)
    assert len(results) == 3
