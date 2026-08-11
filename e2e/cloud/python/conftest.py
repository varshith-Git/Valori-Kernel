"""Shared fixtures for the local Cloud E2E suite.

Uses the REAL create_project_with_default_key() RPC (via the real
PostgREST instance) to provision two real projects — the actual project
creation function, the same one `ui/src/app/dashboard/actions.ts` calls in
production, just invoked directly instead of through the dashboard's HTTP
handler (which does nothing but call this same RPC — see
docs/reviews/local-cloud-e2e-audit.md).

Manually inserts exactly two things a real signup flow would otherwise
have produced (a user + org row, done once in postgres/01_seed_e2e.sql,
mounted at container startup) — everything downstream of that (project
creation, key generation, verification, worker routing) is the real code
path, unmodified.
"""
import base64
import hashlib
import hmac
import json
import os
import time

import pytest
import requests

POSTGREST_URL = os.environ["POSTGREST_URL"]
CLOUD_API_URL = os.environ["CLOUD_API_URL"]
JWT_SECRET = os.environ["JWT_SECRET"]
SEED_USER_ID = os.environ["SEED_USER_ID"]
SEED_ORG_ID = os.environ["SEED_ORG_ID"]
WORKER_A_URL = os.environ["WORKER_A_URL"]
WORKER_B_URL = os.environ["WORKER_B_URL"]
WORKER_A_TOKEN = os.environ["WORKER_A_TOKEN"]
WORKER_B_TOKEN = os.environ["WORKER_B_TOKEN"]


def _b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def sign_jwt(payload: dict) -> str:
    header = _b64url(json.dumps({"alg": "HS256", "typ": "JWT"}).encode())
    body = _b64url(json.dumps(payload).encode())
    signing_input = f"{header}.{body}".encode()
    sig = _b64url(hmac.new(JWT_SECRET.encode(), signing_input, hashlib.sha256).digest())
    return f"{header}.{body}.{sig}"


@pytest.fixture(scope="session")
def authenticated_jwt() -> str:
    """A real HS256 JWT PostgREST will accept and SET ROLE authenticated
    for, with the seeded user's real auth.users.id as `sub` (what
    auth.uid() inside create_project_with_default_key() reads)."""
    return sign_jwt({"role": "authenticated", "sub": SEED_USER_ID, "exp": int(time.time()) + 3600})


@pytest.fixture(scope="session")
def service_role_jwt() -> str:
    """Public fixture wrapping _service_role_jwt() for test modules that
    need it directly (test_limits.py's rate-limit preconditioning) —
    same real service_role JWT _create_project() uses internally."""
    return _service_role_jwt()


def _service_role_jwt() -> str:
    """`node_url`/`worker_auth_token` are deliberately NOT in
    `authenticated`'s UPDATE column grant (see
    supabase/migrations/20260811000000_worker_auth_token.sql) — in real
    production those are written by the Rust provisioner over its own
    direct Postgres connection (service_role-equivalent, bypassing
    PostgREST/RLS entirely), never by a signed-in dashboard user's
    session. This is that same substitution, done with the same role a
    real provisioner would have — not a loosened `authenticated` grant."""
    return sign_jwt({"role": "service_role", "exp": int(time.time()) + 3600})


def _create_project(jwt: str, name: str, slug: str, node_url: str, worker_token: str) -> dict:
    """Calls the REAL create_project_with_default_key() RPC. Then, because
    this environment doesn't run the Rust provisioner (see the audit's
    documented scope decision — provisioning a real container isn't
    needed when the workers already exist as fixed compose services),
    directly sets node_url/worker_auth_token on the resulting row — the
    one place this test setup substitutes for "the provisioner deployed a
    container and told Cloud its address," which is legitimately outside
    what this phase proves (worker *deployment*, not worker *routing/auth*,
    which IS what's tested)."""
    resp = requests.post(
        f"{POSTGREST_URL}/rpc/create_project_with_default_key",
        headers={"Authorization": f"Bearer {jwt}", "Content-Type": "application/json"},
        json={
            "target_org_id": SEED_ORG_ID,
            "p_project_name": name,
            "p_project_slug": slug,
            "p_project_region": "local",
            "p_project_replication": 1,
            "p_project_dim": 4,
            "p_project_index_type": "brute",
        },
        timeout=10,
    )
    resp.raise_for_status()
    row = resp.json()[0] if isinstance(resp.json(), list) else resp.json()

    # The one substitution documented above — everything else in `row` came
    # straight out of the real RPC. Uses service_role, not the caller's own
    # jwt — see _service_role_jwt()'s docstring: node_url/worker_auth_token
    # are real-production writes made by the Rust provisioner's own
    # connection, never by an authenticated dashboard session.
    #
    # Also flips status 'creating' -> 'active': create_project_with_default_key()
    # always creates in 'creating' (public/schema.sql's column default) —
    # the real provisioner flips it to 'active' once the deployed
    # container is actually healthy (confirmed by grepping for
    # `status = 'active'` writes in backend/apps/api's provisioning code).
    # Every vlk_-authenticated route 409s ("project not active yet") on a
    # 'creating' project (resolveProjectNodeUrlByApiKey's own check) — the
    # exact same substitution as node_url/worker_auth_token above, just
    # one more column the real provisioner sets once its job is done.
    patch = requests.patch(
        f"{POSTGREST_URL}/projects?id=eq.{row['project_id']}",
        headers={
            "Authorization": f"Bearer {_service_role_jwt()}",
            "Content-Type": "application/json",
            "Prefer": "return=minimal",
        },
        json={"node_url": node_url, "worker_auth_token": worker_token, "status": "active"},
        timeout=10,
    )
    patch.raise_for_status()

    return {
        "project_id": row["project_id"],
        "name": row["project_name"],
        "api_key": row["plaintext_key"],
        "node_url": node_url,
        "worker_token": worker_token,
    }


@pytest.fixture(scope="session")
def project_a(authenticated_jwt) -> dict:
    return _create_project(authenticated_jwt, "E2E Project A", "e2e-project-a", WORKER_A_URL, WORKER_A_TOKEN)


@pytest.fixture(scope="session")
def project_b(authenticated_jwt) -> dict:
    return _create_project(authenticated_jwt, "E2E Project B", "e2e-project-b", WORKER_B_URL, WORKER_B_TOKEN)


@pytest.fixture()
def sdk_client_a(project_a):
    from valoricore.remote import Valori

    return Valori(url=CLOUD_API_URL, api_key=project_a["api_key"])


@pytest.fixture()
def sdk_client_b(project_b):
    from valoricore.remote import Valori

    return Valori(url=CLOUD_API_URL, api_key=project_b["api_key"])
