"""Restart/persistence: stop and start the real Worker A container,
confirm data survives without recreating anything.

NOTE ON EXECUTION: this test calls `docker compose stop/start worker-a`
via subprocess from INSIDE the `python` service container, which needs
the Docker CLI + socket. If those aren't available in this container
(the default `python:3.11-slim` image has neither), this test is skipped
with a clear reason rather than silently reported as passing — per the
project's own instruction not to claim a check passed unless it actually
ran. Run `scripts/run_persistence_check.sh` from the HOST instead for a
guaranteed real run (see that script's own header).
"""
import hashlib
import os
import shutil
import subprocess

import pytest

WORKER_A_URL = os.environ["WORKER_A_URL"]
CLOUD_API_URL = os.environ["CLOUD_API_URL"]

pytestmark = pytest.mark.skipif(
    shutil.which("docker") is None,
    reason="docker CLI not available inside the python service container — "
    "run scripts/run_persistence_check.sh from the host for a real restart test",
)


def test_data_survives_worker_restart(sdk_client_a, project_a):
    collection = sdk_client_a.collections.create("persistence-check")
    vectors = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0]]
    ids = collection.upsert(vectors)
    assert len(ids) == 3

    before = collection.search([1.0, 0.0, 0.0, 0.0], top_k=1)
    assert before[0]["id"] == ids[0]
    assert before[0]["score"] == 0

    # Real stop/start of the actual container — not a mock, not a
    # re-create. `docker compose` must be run from the compose project
    # directory (this test's cwd is /e2e inside the python container, so
    # cwd isn't it — the compose file lives on the host at
    # e2e/cloud/docker-compose.yml, bind-mounted nowhere inside this
    # container). This only works when this test itself runs on the host
    # with Docker access; skipped otherwise (see pytestmark above).
    compose_dir = "/compose" if os.path.isdir("/compose") else None
    if compose_dir is None:
        pytest.skip("docker-compose.yml not reachable from inside the python container")

    subprocess.run(["docker", "compose", "stop", "worker-a"], cwd=compose_dir, check=True, timeout=60)
    subprocess.run(["docker", "compose", "start", "worker-a"], cwd=compose_dir, check=True, timeout=60)

    # Same project, same API key, same collection name — no recreation.
    after = collection.search([1.0, 0.0, 0.0, 0.0], top_k=3)
    after_ids = {r["id"] for r in after}
    assert after_ids == set(ids), f"expected all 3 record ids to survive restart, got {after_ids}"
    assert after[0]["id"] == ids[0]
    assert after[0]["score"] == 0


def test_snapshot_hash_unchanged_across_restart(sdk_client_a, project_a):
    """Where practical: BLAKE3 state-hash comparison before/after restart,
    via the node's own /v1/proof/state — same real invariant
    `dr_disaster_recovery` (Valori-Kernel's own mandatory DR test) checks,
    reused here at the container level instead of the in-process level."""
    import requests

    compose_dir = "/compose" if os.path.isdir("/compose") else None
    if compose_dir is None:
        pytest.skip("docker-compose.yml not reachable from inside the python container")

    collection = sdk_client_a.collections.create("persistence-hash-check")
    collection.upsert([[0.3, 0.3, 0.3, 0.3]])

    worker_token = project_a["worker_token"]
    before = requests.get(
        f"{WORKER_A_URL}/v1/proof/state",
        headers={"Authorization": f"Bearer {worker_token}"},
        timeout=10,
    )
    before.raise_for_status()
    before_hash = before.json().get("state_hash") or before.json().get("final_state_hash")

    subprocess.run(["docker", "compose", "stop", "worker-a"], cwd=compose_dir, check=True, timeout=60)
    subprocess.run(["docker", "compose", "start", "worker-a"], cwd=compose_dir, check=True, timeout=60)

    after = requests.get(
        f"{WORKER_A_URL}/v1/proof/state",
        headers={"Authorization": f"Bearer {worker_token}"},
        timeout=10,
    )
    after.raise_for_status()
    after_hash = after.json().get("state_hash") or after.json().get("final_state_hash")

    assert before_hash == after_hash, (
        f"BLAKE3 state hash changed across restart — persistence integrity failure. "
        f"before={before_hash} after={after_hash}"
    )
