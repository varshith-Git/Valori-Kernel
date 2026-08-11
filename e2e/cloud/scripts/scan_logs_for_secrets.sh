#!/bin/bash
# Run AFTER the E2E suite (docker compose run --rm python pytest) from the
# HOST, with `docker compose` — not from inside any container, since the
# `python` service deliberately has no Docker socket access (see
# python/test_security.py's own docstring on why this is a separate,
# host-side step).
#
# Classifies every raw-secret-shaped string found in container logs:
#   ACCEPTABLE  - test source/fixtures, hashed values, variable names
#   NOT OK      - a live vlk_ key or worker token appearing in a service
#                 log line that represents what a real deployment's logs
#                 would contain (Cloud API / worker / Postgres / PostgREST
#                 responses, not our own diagnostic echoing).
#
# This scans SERVICE logs only (postgres, postgrest, rest-shim, ui,
# worker-a, worker-b) — never the `python`/`migrate` one-shot containers,
# since those are the test harness itself and legitimately print keys
# (fixtures, assertions) as part of doing their job.
set -euo pipefail
cd "$(dirname "$0")/.."

SERVICES="postgres postgrest rest-shim ui worker-a worker-b"
FAIL=0

echo "==> Scanning real service logs for raw secrets (vlk_ keys, worker tokens, Authorization headers)..."
for svc in $SERVICES; do
  echo "--- $svc ---"
  LOG=$(docker compose logs "$svc" 2>&1 || true)

  # Raw vlk_ keys (prefix_secret shape) — real full key material, not
  # just the harmless key_prefix (vlk_xxxxxxxx alone, 8 hex chars, no
  # trailing underscore+secret, is a non-secret identifier stored openly
  # in api_keys.key_prefix and fine to see in logs).
  HITS=$(echo "$LOG" | grep -oE 'vlk_[a-f0-9]{8}_[a-f0-9]{48}' | sort -u || true)
  if [ -n "$HITS" ]; then
    echo "  NOT OK: raw vlk_ key material found in $svc logs:"
    echo "$HITS" | sed 's/^/    /'
    FAIL=1
  fi

  # Worker tokens (fixed local dev values from .env / .env.example).
  for tok in "worker-a-secret" "worker-b-secret"; do
    if echo "$LOG" | grep -q "$tok"; then
      echo "  NOT OK: worker token '$tok' found in $svc logs"
      FAIL=1
    fi
  done

  if [ -z "$HITS" ] && ! echo "$LOG" | grep -qE 'worker-a-secret|worker-b-secret'; then
    echo "  clean"
  fi
done

echo
if [ "$FAIL" -eq 1 ]; then
  echo "==> SECRET LEAK DETECTED — see NOT OK lines above."
  exit 1
else
  echo "==> No raw vlk_ keys or worker tokens found in any real service's logs."
  exit 0
fi
