"""Project isolation — the single most important test in this suite.

Every operation Project A's key performs against Project A must succeed.
Every operation the SAME key attempts against Project B's project_id in
the URL must be rejected with 403 — proving the server resolves the
authoritative project from the AUTHENTICATED key, not the URL, and
compares the two (docs/architecture/project-api-key-architecture.md Q11).
"""
import os

import pytest
import requests

CLOUD_API_URL = os.environ["CLOUD_API_URL"]


def test_project_a_key_full_crud_on_project_a(sdk_client_a):
    collection = sdk_client_a.collections.create("isolation-a")
    ids = collection.upsert([[0.1, 0.2, 0.3, 0.4]])
    assert len(ids) == 1

    results = collection.search([0.1, 0.2, 0.3, 0.4], top_k=1)
    assert len(results) == 1
    assert results[0]["id"] == ids[0]

    collection.delete(ids[0])
    results_after = collection.search([0.1, 0.2, 0.3, 0.4], top_k=1)
    assert all(r["id"] != ids[0] for r in results_after)

    collection.drop()
    assert "isolation-a" not in sdk_client_a.collections.list()


@pytest.mark.parametrize(
    "path,method,body",
    [
        # Exhaustive: every /api/projects/[id]/* route that actually
        # accepts a vlk_ key today (verified by grepping every route.ts
        # under ui/src/app/api/projects/[id] for `{ req, scope }` /
        # resolveProjectAccess — routes without it are session-only and
        # not part of this invariant at all: snapshots, proof, operations,
        # tree, community, meta, graph, records are all dashboard-session-
        # only, so a vlk_ key gets 401 there regardless of which project
        # the URL names, not a meaningful 403 isolation check).
        ("/namespaces", "POST", {"name": "should-not-be-created-in-b"}),
        ("/namespaces", "GET", None),
        ("/namespaces/some-collection", "DELETE", None),
        ("/search", "POST", {"query": [0.1, 0.2, 0.3, 0.4], "k": 1}),
        ("/insert", "POST", {"batch": [[0.1, 0.2, 0.3, 0.4]]}),
        ("/delete", "POST", {"id": 0}),
    ],
)
def test_project_a_key_against_project_b_is_403(project_a, project_b, path, method, body):
    """Direct HTTP, not the SDK — the SDK auto-resolves its own project
    and would never construct a request naming a different one, which is
    exactly why this test bypasses it: it's simulating a URL tampering
    attempt (or a bug in some OTHER client), which is precisely the case
    the server-side comparison exists to catch regardless of what any
    particular client does."""
    resp = requests.request(
        method,
        f"{CLOUD_API_URL}/api/projects/{project_b['project_id']}{path}",
        headers={"Authorization": f"Bearer {project_a['api_key']}", "Content-Type": "application/json"},
        json=body,
        timeout=10,
    )
    assert resp.status_code == 403, (
        f"SECURITY FAILURE: Project A's key against Project B's {path} "
        f"returned {resp.status_code}, expected 403"
    )


def test_data_inserted_in_a_is_not_visible_from_b(sdk_client_a, sdk_client_b):
    a = sdk_client_a.collections.create("cross-project-data")
    a.upsert([[0.9, 0.9, 0.9, 0.9]])

    # Project B never had this collection created in it — it's a
    # completely separate namespace on a completely separate node
    # (Worker B), so a collection of the same NAME in B starts empty.
    b = sdk_client_b.collections.create("cross-project-data")
    results = b.search([0.9, 0.9, 0.9, 0.9], top_k=5)
    assert results == [], "Project B can see Project A's data — cross-project data leak"
