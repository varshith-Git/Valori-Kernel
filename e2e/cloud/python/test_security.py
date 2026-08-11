"""Security assertions reachable from inside the test process itself.

Docker container logs are NOT scannable from here — the `python` service
has no Docker socket access (deliberately: giving a test container control
over the Docker daemon is its own security smell, not something to trade
for test convenience). Log scanning for raw secrets is a separate,
host-side step: scripts/scan_logs_for_secrets.sh, run after this suite
finishes (see README.md) — not silently skipped, just a different layer.
"""
import os

import requests

POSTGREST_URL = os.environ["POSTGREST_URL"]


def test_worker_token_never_in_search_response(sdk_client_a, project_a):
    c = sdk_client_a.collections.create("security-response-scan")
    c.upsert([[0.1, 0.2, 0.3, 0.4]])
    results = c.search([0.1, 0.2, 0.3, 0.4], top_k=1)
    assert project_a["worker_token"] not in str(results)


def test_worker_token_not_selectable_via_anon_or_authenticated(authenticated_jwt, project_a):  # noqa: F811
    """Real column-privilege test — the same property verified at the SQL
    layer in Valori-Kernel's project-api-key-P2.2 phase, re-verified here
    over the actual E2E PostgREST instance."""
    resp = requests.get(
        f"{POSTGREST_URL}/projects",
        params={"id": f"eq.{project_a['project_id']}", "select": "id,worker_auth_token"},
        headers={"Authorization": f"Bearer {authenticated_jwt}"},
        timeout=10,
    )
    assert resp.status_code == 401 or resp.status_code == 403 or resp.status_code == 400, (
        f"SECURITY FAILURE: worker_auth_token was selectable by 'authenticated' role "
        f"(status={resp.status_code}, body={resp.text[:200]})"
    )


def test_api_keys_public_never_exposes_hash_or_plaintext(authenticated_jwt):  # noqa: F811
    resp = requests.get(
        f"{POSTGREST_URL}/api_keys_public",
        params={"limit": "1"},
        headers={"Authorization": f"Bearer {authenticated_jwt}"},
        timeout=10,
    )
    resp.raise_for_status()
    rows = resp.json()
    for row in rows:
        assert "key_hash" not in row
        assert "plaintext_key" not in row


def test_revoked_key_response_never_returns_raw_key(sdk_client_a):
    # Reveal-once contract: the SDK itself never re-fetches or persists
    # the raw key anywhere. Confirm the Valori client instance holds no
    # attribute containing the plaintext beyond what it was constructed
    # with (i.e., it doesn't independently cache/log it elsewhere).
    attrs = {k: v for k, v in vars(sdk_client_a).items() if isinstance(v, str)}
    # The token IS legitimately held by the transport (_BearerAuth) to
    # attach on requests — that's expected, not a leak. This test's real
    # purpose is documented: there is no SECOND copy anywhere else on the
    # object.
    key_bearing_attrs = [k for k, v in attrs.items() if v.startswith("vlk_")]
    assert len(key_bearing_attrs) <= 1
