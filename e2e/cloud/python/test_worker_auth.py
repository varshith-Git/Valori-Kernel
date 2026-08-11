"""Direct HTTP tests against the real valori-node containers (Worker A),
bypassing the Cloud API entirely — proves the node's OWN auth_guard_v2,
not Cloud's proxying of it.

Verified by reading crates/valori-node/src/server.rs directly:
- `/health` and `/metrics` are in the `public` router (no auth layer at
  all) — intentional, not a gap. Documented here rather than "fixed" to
  force uniformity, per instruction not to change /health.
- Every `/v1/*` route sits behind `auth_guard_v2`: no bearer -> 401;
  bearer present but matches neither the key store nor the constant-time
  legacy-token compare -> 401 (this is what a customer's vlk_ key does —
  it's just an arbitrary string to the node, which has no concept of
  Cloud's key format); correct VALORI_AUTH_TOKEN -> passes through.
"""
import os

import requests

WORKER_A_URL = os.environ["WORKER_A_URL"]
WORKER_A_TOKEN = os.environ["WORKER_A_TOKEN"]


def _hdr(token: str | None) -> dict:
    return {"Authorization": f"Bearer {token}"} if token else {}


def test_health_is_unauthenticated_by_design():
    """Not a gap — /health is deliberately public (load balancer / operator
    probe). Confirmed via crates/valori-node/src/server.rs's own `public`
    router, which never gets auth_guard_v2's layer."""
    resp = requests.get(f"{WORKER_A_URL}/health", timeout=10)
    assert resp.status_code == 200


def test_create_collection_no_token_is_401():
    resp = requests.post(f"{WORKER_A_URL}/v1/namespaces", json={"name": "worker-auth-noauth"}, timeout=10)
    assert resp.status_code == 401


def test_create_collection_wrong_token_is_401():
    resp = requests.post(
        f"{WORKER_A_URL}/v1/namespaces",
        headers=_hdr("definitely-not-the-real-token"),
        json={"name": "worker-auth-wrongtoken"},
        timeout=10,
    )
    assert resp.status_code == 401


def test_create_collection_customer_vlk_key_is_401():
    """The node has no concept of Cloud's vlk_ key format — it's just an
    arbitrary bearer string that matches neither the key store nor the
    legacy token. Confirms customer keys are never independently valid
    against a worker (Cloud's worker_auth_token is the only credential
    the node itself understands)."""
    resp = requests.post(
        f"{WORKER_A_URL}/v1/namespaces",
        headers=_hdr("vlk_fake0000_deadbeefdeadbeefdeadbeefdeadbeef"),
        json={"name": "worker-auth-vlkkey"},
        timeout=10,
    )
    assert resp.status_code == 401


def test_create_collection_correct_token_succeeds():
    resp = requests.post(
        f"{WORKER_A_URL}/v1/namespaces",
        headers=_hdr(WORKER_A_TOKEN),
        json={"name": "worker-auth-ok"},
        timeout=10,
    )
    assert resp.status_code == 200, resp.text


def test_get_collection_list_correct_token_succeeds():
    resp = requests.get(f"{WORKER_A_URL}/v1/namespaces", headers=_hdr(WORKER_A_TOKEN), timeout=10)
    assert resp.status_code == 200
    names = [c["name"] for c in resp.json().get("collections", [])]
    assert "worker-auth-ok" in names


def test_insert_vector_correct_token_succeeds():
    resp = requests.post(
        f"{WORKER_A_URL}/v1/vectors/batch-insert",
        headers=_hdr(WORKER_A_TOKEN),
        json={"batch": [[0.1, 0.2, 0.3, 0.4]], "collection": "worker-auth-ok"},
        timeout=10,
    )
    assert resp.status_code == 200, resp.text


def test_insert_vector_no_token_is_401():
    resp = requests.post(
        f"{WORKER_A_URL}/v1/vectors/batch-insert",
        json={"batch": [[0.1, 0.2, 0.3, 0.4]], "collection": "worker-auth-ok"},
        timeout=10,
    )
    assert resp.status_code == 401


def test_search_vector_correct_token_succeeds():
    resp = requests.post(
        f"{WORKER_A_URL}/v1/search",
        headers=_hdr(WORKER_A_TOKEN),
        json={"query": [0.1, 0.2, 0.3, 0.4], "k": 1, "collection": "worker-auth-ok"},
        timeout=10,
    )
    assert resp.status_code == 200, resp.text
    assert len(resp.json().get("results", [])) == 1


def test_search_vector_wrong_token_is_401():
    resp = requests.post(
        f"{WORKER_A_URL}/v1/search",
        headers=_hdr("wrong-token"),
        json={"query": [0.1, 0.2, 0.3, 0.4], "k": 1, "collection": "worker-auth-ok"},
        timeout=10,
    )
    assert resp.status_code == 401


def test_delete_vector_correct_token_succeeds():
    ins = requests.post(
        f"{WORKER_A_URL}/v1/vectors/batch-insert",
        headers=_hdr(WORKER_A_TOKEN),
        json={"batch": [[0.9, 0.9, 0.9, 0.9]], "collection": "worker-auth-ok"},
        timeout=10,
    )
    ins.raise_for_status()
    record_id = ins.json()["ids"][0]

    resp = requests.post(
        f"{WORKER_A_URL}/v1/delete",
        headers=_hdr(WORKER_A_TOKEN),
        json={"id": record_id, "collection": "worker-auth-ok"},
        timeout=10,
    )
    assert resp.status_code == 200, resp.text


def test_delete_vector_no_token_is_401():
    resp = requests.post(
        f"{WORKER_A_URL}/v1/delete",
        json={"id": 0, "collection": "worker-auth-ok"},
        timeout=10,
    )
    assert resp.status_code == 401
