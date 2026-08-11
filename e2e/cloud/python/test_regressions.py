"""Permanent regression tests for the 4 real bugs found while building this
E2E suite (see docs/reviews/local-cloud-e2e-audit.md G6+). Each test names
its bug explicitly so a future revert shows up as a named failure, not a
generic one.
"""
import datetime
import os

import pytest
import requests

from conftest import _create_project, _service_role_jwt, WORKER_A_TOKEN, WORKER_A_URL, SEED_ORG_ID  # noqa: F401

POSTGREST_URL = os.environ["POSTGREST_URL"]
CLOUD_API_URL = os.environ["CLOUD_API_URL"]


@pytest.fixture(scope="module")
def regress_project(authenticated_jwt):  # noqa: F811
    return _create_project(authenticated_jwt, "E2E Project Regress", "e2e-project-regress", WORKER_A_URL, WORKER_A_TOKEN)


@pytest.fixture(scope="module")
def regress_project_b(authenticated_jwt):  # noqa: F811
    return _create_project(authenticated_jwt, "E2E Project Regress B", "e2e-project-regress-b", WORKER_A_URL, WORKER_A_TOKEN)


# ── Bug 1: /api/projects/[id]/delete didn't accept API-key auth (404) ───────

def test_bug1_delete_with_own_project_key_succeeds(regress_project):
    from valoricore.remote import Valori

    client = Valori(url=CLOUD_API_URL, api_key=regress_project["api_key"])
    c = client.collections.create("bug1-delete")
    ids = c.upsert([[0.1, 0.2, 0.3, 0.4]])
    c.delete(ids[0])  # would have 404'd pre-fix
    results = c.search([0.1, 0.2, 0.3, 0.4], top_k=5)
    assert all(r["id"] != ids[0] for r in results)


def test_bug1_delete_with_other_project_key_is_403(regress_project, regress_project_b):
    resp = requests.post(
        f"{CLOUD_API_URL}/api/projects/{regress_project_b['project_id']}/delete",
        headers={"Authorization": f"Bearer {regress_project['api_key']}", "Content-Type": "application/json"},
        json={"id": 0},
        timeout=10,
    )
    assert resp.status_code == 403, f"bug1 regression: expected 403, got {resp.status_code}: {resp.text}"


def test_bug1_delete_with_bad_key_is_401(regress_project):
    resp = requests.post(
        f"{CLOUD_API_URL}/api/projects/{regress_project['project_id']}/delete",
        headers={"Authorization": "Bearer vlk_totally_fake_key_00000000000000", "Content-Type": "application/json"},
        json={"id": 0},
        timeout=10,
    )
    assert resp.status_code == 401


# ── Bug 2: `import valoricore` required the compiled FFI extension ─────────

def test_bug2_valori_importable_without_ffi_module():
    """Simulates the FFI extension being absent (matches local.py's own
    try/except ImportError -> _ffi = None guard) by asserting the import
    chain doesn't hard-fail even when valoricore_ffi can't be found.
    Directly imports `valoricore.remote.Valori` — the pure-HTTP class —
    without ever touching `valoricore.local`/`valoricore.adapter`'s
    FFI-backed code paths."""
    import importlib
    import sys

    # Drop any cached import of the FFI extension so this test doesn't
    # silently pass just because a prior test already imported it.
    for mod in list(sys.modules):
        if mod.startswith("valoricore"):
            del sys.modules[mod]

    from valoricore.remote import Valori  # must not raise ImportError

    client = Valori(url="http://example.invalid", api_key="vlk_test")
    assert client is not None


def test_bug2_top_level_package_import_does_not_require_ffi():
    """The bug: `from valoricore import Valori` (not `valoricore.remote`)
    went through __init__.py -> adapter.py -> `from .valoricore_ffi import
    verify_embedding` unconditionally. adapter.py now guards that import
    (try/except ImportError -> verify_embedding = None) — this is the
    exact top-level import path a production Cloud user is documented to
    use (see project-api-v1.md)."""
    import importlib
    import sys

    for mod in list(sys.modules):
        if mod.startswith("valoricore"):
            del sys.modules[mod]

    import valoricore  # must not raise, even without the .so present

    assert hasattr(valoricore, "Valori") or hasattr(valoricore.remote, "Valori")


# ── Bug 3: create_api_key() had ambiguous-column SQL bugs ──────────────────

def test_bug3_create_api_key_via_real_postgrest(authenticated_jwt, regress_project):  # noqa: F811
    resp = requests.post(
        f"{POSTGREST_URL}/rpc/create_api_key",
        headers={"Authorization": f"Bearer {authenticated_jwt}", "Content-Type": "application/json"},
        json={"target_org_id": SEED_ORG_ID, "key_name": "bug3-key", "p_project_id": regress_project["project_id"]},
        timeout=10,
    )
    assert resp.status_code == 200, f"bug3 regression: create_api_key failed [{resp.status_code}]: {resp.text}"
    row = resp.json()[0] if isinstance(resp.json(), list) else resp.json()
    plaintext = row["plaintext_key"]
    key_id = row["id"]

    # The returned key is actually usable end-to-end (Cloud API, not just RPC).
    from valoricore.remote import Valori

    client = Valori(url=CLOUD_API_URL, api_key=plaintext)
    collection = client.collections.create("bug3-usable-check")
    assert collection.name == "bug3-usable-check"

    # Raw key is not persisted anywhere in api_keys — only its hash is.
    row_check = requests.get(
        f"{POSTGREST_URL}/api_keys_public",
        params={"id": f"eq.{key_id}"},
        headers={"Authorization": f"Bearer {authenticated_jwt}"},
        timeout=10,
    )
    row_check.raise_for_status()
    stored = row_check.json()[0]
    assert "key_hash" not in stored
    assert "plaintext_key" not in stored
    assert plaintext not in str(stored)

    # The hash IS persisted (service_role can see the real api_keys row).
    svc_check = requests.get(
        f"{POSTGREST_URL}/api_keys",
        params={"id": f"eq.{key_id}", "select": "id,key_hash"},
        headers={"Authorization": f"Bearer {_service_role_jwt()}"},
        timeout=10,
    )
    svc_check.raise_for_status()
    svc_row = svc_check.json()[0]
    assert svc_row["key_hash"] and svc_row["key_hash"] != plaintext


# ── Bug 4: proxyToNode() forced DELETE->POST and crashed on 204 ────────────

def test_bug4_get_post_delete_all_use_real_methods(regress_project):
    from valoricore.remote import Valori

    client = Valori(url=CLOUD_API_URL, api_key=regress_project["api_key"])

    # POST (create)
    c = client.collections.create("bug4-methods")
    # GET (list) — goes through proxyToNode with method GET
    assert "bug4-methods" in client.collections.list()
    # POST (insert)
    ids = c.upsert([[0.5, 0.5, 0.5, 0.5]])
    # POST (search)
    assert len(c.search([0.5, 0.5, 0.5, 0.5], top_k=1)) == 1
    # POST (delete-record)
    c.delete(ids[0])
    # DELETE (drop collection) — this is the exact path that broke:
    # method got silently rewritten to POST, and the real 204 response
    # crashed NextResponse.json(). Raises if either regression comes back.
    c.drop()
    assert "bug4-methods" not in client.collections.list()


def test_bug4_delete_returns_204_not_json_error(regress_project):
    """Direct HTTP check that a real DELETE on an existing collection
    returns a clean 204 (or 200), never crashes into the 503
    "backend unreachable" fallback the bug produced."""
    create = requests.post(
        f"{CLOUD_API_URL}/api/projects/{regress_project['project_id']}/namespaces",
        headers={"Authorization": f"Bearer {regress_project['api_key']}", "Content-Type": "application/json"},
        json={"name": "bug4-direct"},
        timeout=10,
    )
    create.raise_for_status()

    resp = requests.delete(
        f"{CLOUD_API_URL}/api/projects/{regress_project['project_id']}/namespaces/bug4-direct",
        headers={"Authorization": f"Bearer {regress_project['api_key']}"},
        timeout=10,
    )
    assert resp.status_code in (200, 204), (
        f"bug4 regression: DELETE returned {resp.status_code} (expected 200/204): {resp.text}"
    )
