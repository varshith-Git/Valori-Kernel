"""API-key lifecycle + rate limiting.

Rate limiting reality check (from reading
valori-ui/ui/src/lib/server/project.ts's resolveProjectAccess() /
resolveProjectNodeUrlByApiKey() / verifyApiKey() directly, not assumed):
rate limiting IS implemented for real, and applies to EVERY route that
authenticates a `vlk_` key via `proxyToNode(..., { req, scope })` — the
`verify_api_key()` RPC is the single check point and it runs on every such
call, checking + bumping usage before the route-specific logic ever runs.
That includes /search AND /insert (both pass `{ req, scope }`) — there is
no route-specific carve-out.

The real, unmodified 'free' plan limit is 60/min (its production default —
see supabase/migrations/20260723040000_api_usage_and_rate_limits.sql). An
earlier draft of this test suite lowered it globally to 5/min in the seed
SQL "for fast testing" — that broke every OTHER test sharing the seeded
org's keys, since verify_api_key() counts every vlk_-authenticated call
against the SAME key, not just the two tests here. Fixed by leaving the
real 60/min limit alone and instead preconditioning each dedicated test
key's own `api_keys.rate_limit_window_count` directly via service_role
(the same substitution conftest.py's _create_project() already documents
for node_url/worker_auth_token) to sit one request below the ceiling —
trips the real limiter in 2 requests without touching global plan config.

Second real limit found the same way: max_api_keys per project is 3 (real
production value — "api key limit reached (3 of 3 active keys for this
project)"). This module's own three tests together need 4+ live keys,
and sharing conftest's session-scoped `project_a` (used by every other
test file) meant this module ran out of budget partway through — not
because 3 is wrong, but because the shared fixture wasn't designed for
a module that mints several keys per test. Fixed with `limits_project`,
a project dedicated to this file alone, so key-count state here never
depends on what other test files already created.
"""
import datetime
import os

import requests
import pytest

from conftest import _create_project, WORKER_A_TOKEN, WORKER_A_URL  # noqa: F401

POSTGREST_URL = os.environ["POSTGREST_URL"]
CLOUD_API_URL = os.environ["CLOUD_API_URL"]
SEED_ORG_ID = os.environ["SEED_ORG_ID"]

REAL_FREE_PLAN_LIMIT_PER_MINUTE = 60


@pytest.fixture(scope="module")
def limits_project(authenticated_jwt):  # noqa: F811
    return _create_project(authenticated_jwt, "E2E Project Limits", "e2e-project-limits", WORKER_A_URL, WORKER_A_TOKEN)


def _create_key(jwt: str, project_id: str, name: str) -> dict:
    resp = requests.post(
        f"{POSTGREST_URL}/rpc/create_api_key",
        headers={"Authorization": f"Bearer {jwt}", "Content-Type": "application/json"},
        json={"target_org_id": SEED_ORG_ID, "key_name": name, "p_project_id": project_id},
        timeout=10,
    )
    if resp.status_code >= 400:
        raise AssertionError(f"create_api_key failed [{resp.status_code}]: {resp.text}")
    return resp.json()[0] if isinstance(resp.json(), list) else resp.json()


def _revoke_key(jwt: str, key_id: str) -> None:
    resp = requests.patch(
        f"{POSTGREST_URL}/api_keys_public?id=eq.{key_id}",
        headers={"Authorization": f"Bearer {jwt}", "Content-Type": "application/json", "Prefer": "return=minimal"},
        json={"revoked_at": datetime.datetime.utcnow().isoformat() + "Z"},
        timeout=10,
    )
    resp.raise_for_status()


def _precondition_near_limit(service_jwt: str, key_id: str) -> None:
    """Directly sets this ONE dedicated test key's rate_limit_window_count
    to one below the real 60/min ceiling, in the current minute window —
    so the very next real request through verify_api_key() lands exactly
    on the boundary, and the one after that trips the real 429. Uses
    service_role (api_keys has no authenticated-role UPDATE grant on
    these columns at all — this table isn't in the client-writable
    surface, same access-control story as worker_auth_token)."""
    window_start = datetime.datetime.utcnow().replace(second=0, microsecond=0).isoformat() + "Z"
    resp = requests.patch(
        f"{POSTGREST_URL}/api_keys?id=eq.{key_id}",
        headers={"Authorization": f"Bearer {service_jwt}", "Content-Type": "application/json", "Prefer": "return=minimal"},
        json={
            "rate_limit_window_start": window_start,
            "rate_limit_window_count": REAL_FREE_PLAN_LIMIT_PER_MINUTE - 1,
        },
        timeout=10,
    )
    resp.raise_for_status()


def _search(api_key: str, project_id: str):
    return requests.post(
        f"{CLOUD_API_URL}/api/projects/{project_id}/search",
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
        json={"query": [0.1, 0.2, 0.3, 0.4], "k": 1},
        timeout=10,
    )


def test_full_key_lifecycle(authenticated_jwt, limits_project):  # noqa: F811
    key1 = _create_key(authenticated_jwt, limits_project["project_id"], "lifecycle-key-1")
    key2 = _create_key(authenticated_jwt, limits_project["project_id"], "lifecycle-key-2")

    assert _search(key1["plaintext_key"], limits_project["project_id"]).status_code == 200
    assert _search(key2["plaintext_key"], limits_project["project_id"]).status_code == 200

    _revoke_key(authenticated_jwt, key1["id"])

    assert _search(key1["plaintext_key"], limits_project["project_id"]).status_code == 401
    assert _search(key2["plaintext_key"], limits_project["project_id"]).status_code == 200

    # Cleanup: this module's project has a real 3-key-per-project ceiling
    # (see module docstring) shared across all three tests here — leave
    # only the project's own default key active so the next test has
    # budget for its own dedicated key.
    _revoke_key(authenticated_jwt, key2["id"])


def test_rate_limiting_on_search_route(authenticated_jwt, service_role_jwt, limits_project):  # noqa: F811
    """Precondition this dedicated key to one request below the real
    60/min ceiling, then fire 2 requests: the first lands ON the
    boundary (200), the second trips the real limiter (429) — never
    sleeps for a real minute, never asserts a passing result that wasn't
    actually observed."""
    key = _create_key(authenticated_jwt, limits_project["project_id"], "rate-limit-key")
    plaintext = key["plaintext_key"]
    _precondition_near_limit(service_role_jwt, key["id"])

    at_boundary = _search(plaintext, limits_project["project_id"])
    assert at_boundary.status_code == 200, f"expected the boundary request (#{REAL_FREE_PLAN_LIMIT_PER_MINUTE}) to clear, got {at_boundary.status_code}"

    over_limit = _search(plaintext, limits_project["project_id"])
    assert over_limit.status_code == 429, (
        f"expected 429 on request #{REAL_FREE_PLAN_LIMIT_PER_MINUTE + 1} within the minute window, "
        f"got {over_limit.status_code}. If this fails, rate limiting on /search is not actually "
        "enforcing the plan limit — report RATE_LIMITING_NOT_ENFORCED, do not treat this as a flake."
    )

    _revoke_key(authenticated_jwt, key["id"])  # see test_full_key_lifecycle's cleanup note


def test_rate_limiting_also_applies_to_insert_route(authenticated_jwt, service_role_jwt, limits_project):  # noqa: F811
    """verify_api_key() is the single check point for every vlk_-authenticated
    route (see module docstring) — confirm /insert hits the same ceiling
    as /search, not a route-specific behavior."""
    key = _create_key(authenticated_jwt, limits_project["project_id"], "insert-rate-limit-key")
    plaintext = key["plaintext_key"]
    _precondition_near_limit(service_role_jwt, key["id"])

    def _insert():
        return requests.post(
            f"{CLOUD_API_URL}/api/projects/{limits_project['project_id']}/insert",
            headers={"Authorization": f"Bearer {plaintext}", "Content-Type": "application/json"},
            json={"batch": [[0.1, 0.2, 0.3, 0.4]]},
            timeout=10,
        )

    at_boundary = _insert()
    assert at_boundary.status_code == 200, f"expected the boundary insert to clear, got {at_boundary.status_code}"

    over_limit = _insert()
    assert over_limit.status_code == 429, (
        f"expected 429 on the over-limit /insert, got {over_limit.status_code}."
    )
