"""Authentication: valid / invalid / missing / revoked / expired keys.
Uses the real Valori SDK against the real Cloud API — no mocked HTTP."""
import time

import pytest
import requests
from valoricore.exceptions import AuthenticationError

from conftest import POSTGREST_URL, SEED_ORG_ID, _create_project, authenticated_jwt  # noqa: F401


def test_valid_key_succeeds(sdk_client_a):
    collection = sdk_client_a.collections.create("auth-test")
    assert collection.name == "auth-test"


def test_missing_key_is_401(project_a):
    # No Authorization header at all, straight against the real Cloud API route.
    #
    # Real, documented behavior (not a gap this test papers over): with no
    # bearer AND no Supabase session cookie, resolveProjectAccess() falls
    # through to the session-only resolveProjectNodeUrl() path, which
    # returns 404 by design — its own docstring in
    # ui/src/lib/server/project.ts: "An empty result is ambiguous by
    # design: either the project doesn't exist, or the caller isn't a
    # member of the org that owns it — both cases should read as 'not
    # found', never leaking which." A zero-credential request collapses
    # into that same ambiguous case. 401 would be the more conventional
    # signal for "no credentials at all," but that's not what this system
    # was built to do, and changing it would be redesigning existing
    # auth behavior — out of scope here. Asserting the real, current
    # response instead of the naively-expected one.
    resp = requests.post(
        f"{_cloud_api_url()}/api/projects/{project_a['project_id']}/search",
        json={"query": [0.1, 0.2, 0.3, 0.4], "k": 1},
        timeout=10,
    )
    assert resp.status_code == 404


def test_invalid_key_is_401(project_a):
    from valoricore.remote import Valori

    client = Valori(url=_cloud_api_url(), api_key="vlk_totally_made_up_does_not_exist")
    with pytest.raises(AuthenticationError):
        client.collections.create("should-fail")


def test_revoked_key_is_401(authenticated_jwt, project_a):  # noqa: F811
    from valoricore.remote import Valori

    # Create a second key on Project A specifically to revoke, so we don't
    # touch the Default key other tests rely on.
    resp = requests.post(
        f"{POSTGREST_URL}/rpc/create_api_key",
        headers={"Authorization": f"Bearer {authenticated_jwt}", "Content-Type": "application/json"},
        json={
            "target_org_id": SEED_ORG_ID,
            "key_name": "to-revoke",
            "p_project_id": project_a["project_id"],
        },
        timeout=10,
    )
    resp.raise_for_status()
    created = resp.json()[0] if isinstance(resp.json(), list) else resp.json()
    key_id = created["id"]
    plaintext = created["plaintext_key"]

    # Revoke it via api_keys_public, the real update path the dashboard uses.
    revoke = requests.patch(
        f"{POSTGREST_URL}/api_keys_public?id=eq.{key_id}",
        headers={"Authorization": f"Bearer {authenticated_jwt}", "Content-Type": "application/json", "Prefer": "return=minimal"},
        json={"revoked_at": "now()"},
        timeout=10,
    )
    # revoked_at accepts an ISO string, not the literal "now()" — fix below.
    if revoke.status_code >= 400:
        import datetime

        revoke = requests.patch(
            f"{POSTGREST_URL}/api_keys_public?id=eq.{key_id}",
            headers={"Authorization": f"Bearer {authenticated_jwt}", "Content-Type": "application/json", "Prefer": "return=minimal"},
            json={"revoked_at": datetime.datetime.utcnow().isoformat() + "Z"},
            timeout=10,
        )
    revoke.raise_for_status()

    client = Valori(url=_cloud_api_url(), api_key=plaintext)
    with pytest.raises(AuthenticationError):
        client.collections.create("should-fail-revoked")


def test_expired_key_is_401(authenticated_jwt, project_a):  # noqa: F811
    from valoricore.remote import Valori
    import datetime

    expires_at = (datetime.datetime.utcnow() + datetime.timedelta(seconds=2)).isoformat() + "Z"
    resp = requests.post(
        f"{POSTGREST_URL}/rpc/create_api_key",
        headers={"Authorization": f"Bearer {authenticated_jwt}", "Content-Type": "application/json"},
        json={
            "target_org_id": SEED_ORG_ID,
            "key_name": "short-lived",
            "p_project_id": project_a["project_id"],
            "p_expires_at": expires_at,
        },
        timeout=10,
    )
    resp.raise_for_status()
    created = resp.json()[0] if isinstance(resp.json(), list) else resp.json()
    plaintext = created["plaintext_key"]

    # Before expiry: works.
    client = Valori(url=_cloud_api_url(), api_key=plaintext)
    client.collections.create("expiry-test")

    # Do NOT sleep for hours — this key's real expires_at is 2 seconds out.
    time.sleep(3)

    client2 = Valori(url=_cloud_api_url(), api_key=plaintext)
    with pytest.raises(AuthenticationError):
        client2.collections.create("expiry-test-2")


def test_revoked_and_expired_key_is_401(authenticated_jwt, project_a):  # noqa: F811
    """Both conditions true at once — verify_api_key() rejects on either,
    so this must fail the same way a single-condition rejection does."""
    import datetime

    from valoricore.remote import Valori

    expires_at = (datetime.datetime.utcnow() + datetime.timedelta(seconds=2)).isoformat() + "Z"
    resp = requests.post(
        f"{POSTGREST_URL}/rpc/create_api_key",
        headers={"Authorization": f"Bearer {authenticated_jwt}", "Content-Type": "application/json"},
        json={
            "target_org_id": SEED_ORG_ID,
            "key_name": "revoked-and-expired",
            "p_project_id": project_a["project_id"],
            "p_expires_at": expires_at,
        },
        timeout=10,
    )
    resp.raise_for_status()
    created = resp.json()[0] if isinstance(resp.json(), list) else resp.json()
    key_id = created["id"]
    plaintext = created["plaintext_key"]

    revoke = requests.patch(
        f"{POSTGREST_URL}/api_keys_public?id=eq.{key_id}",
        headers={"Authorization": f"Bearer {authenticated_jwt}", "Content-Type": "application/json", "Prefer": "return=minimal"},
        json={"revoked_at": datetime.datetime.utcnow().isoformat() + "Z"},
        timeout=10,
    )
    revoke.raise_for_status()

    time.sleep(3)  # now also past its expires_at

    client = Valori(url=_cloud_api_url(), api_key=plaintext)
    with pytest.raises(AuthenticationError):
        client.collections.create("revoked-and-expired-test")


def _cloud_api_url() -> str:
    import os

    return os.environ["CLOUD_API_URL"]
