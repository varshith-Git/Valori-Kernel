#!/usr/bin/env bash
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
#
# Phase API-4D §4/§9 — a deterministic, disposable Valori node for the SDK
# integration suites.
#
#   scripts/sdk-integration-node.sh <command...>
#
# Starts a standalone node on a free port with its own throwaway storage root,
# waits for /health, exports the VALORI_TEST_* variables the suites read, runs
# <command...>, then shuts the node down and deletes the storage root — whether
# the command passed, failed, or the shell was interrupted.
#
# Why a script rather than per-suite fixtures: Python and TypeScript are two
# independent consumers of the same OpenAPI contract (§15), and "the same
# representative scenarios in both" is only meaningful if both meet the same
# server, configured the same way. One harness, two callers.
#
# NOTHING HERE MAY DEPEND ON A DEVELOPER'S EXISTING LOCAL NODE. The port is
# chosen at run time, the storage root is a fresh mktemp, and an already-running
# node on :3000 is neither used nor disturbed. Set VALORI_TEST_ENDPOINT in your
# own environment only if you deliberately want to bypass this script.
#
# Every variable the node needs is set explicitly — §9: "do not rely on
# unspecified defaults". In particular VALORI_EVENT_LOG_PATH is REQUIRED: with
# it absent the node answers `GET /v1/proof/event-log` with "Event log not
# enabled", and the proof cases would be silently testing an error path.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <command...>" >&2
  exit 2
fi

# ── configuration (explicit; no unspecified defaults) ────────────────────────
DIM="${VALORI_TEST_DIM:-8}"
BUILD_PROFILE="${VALORI_TEST_PROFILE:-release}"

STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/valori-sdk-it.XXXXXX")"
NODE_PID=""

cleanup() {
  local status=$?
  if [ -n "$NODE_PID" ] && kill -0 "$NODE_PID" 2>/dev/null; then
    kill "$NODE_PID" 2>/dev/null || true
    # Give it a moment to snapshot and release its file locks, then insist.
    for _ in $(seq 1 20); do
      kill -0 "$NODE_PID" 2>/dev/null || break
      sleep 0.25
    done
    kill -9 "$NODE_PID" 2>/dev/null || true
    wait "$NODE_PID" 2>/dev/null || true
  fi
  rm -rf "$STATE_DIR"
  exit "$status"
}
trap cleanup EXIT INT TERM

# ── a free port, not a hard-coded one ────────────────────────────────────────
PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"

# ── build ────────────────────────────────────────────────────────────────────
echo "building valori-node ($BUILD_PROFILE)"
if [ "$BUILD_PROFILE" = "release" ]; then
  cargo build -q -p valori-node --release
  BIN="$ROOT/target/release/valori-node"
else
  cargo build -q -p valori-node
  BIN="$ROOT/target/debug/valori-node"
fi

# ── start ────────────────────────────────────────────────────────────────────
echo "starting node on 127.0.0.1:$PORT (state: $STATE_DIR)"
VALORI_DIM="$DIM" \
VALORI_BIND="127.0.0.1:$PORT" \
VALORI_EVENT_LOG_PATH="$STATE_DIR/events.log" \
VALORI_SNAPSHOT_PATH="$STATE_DIR/snapshot.bin" \
VALORI_MAX_RECORDS=100000 \
VALORI_MAX_NODES=10000 \
VALORI_MAX_EDGES=50000 \
VALORI_INDEX=brute \
  "$BIN" >"$STATE_DIR/node.log" 2>&1 &
NODE_PID=$!

# ── wait for health ──────────────────────────────────────────────────────────
READY=0
for _ in $(seq 1 120); do
  if ! kill -0 "$NODE_PID" 2>/dev/null; then
    echo "node exited during startup; log follows:" >&2
    cat "$STATE_DIR/node.log" >&2
    exit 1
  fi
  if curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 0.5
done

if [ "$READY" -ne 1 ]; then
  echo "node did not become healthy within 60s; log follows:" >&2
  cat "$STATE_DIR/node.log" >&2
  exit 1
fi
echo "node healthy: $(curl -s "http://127.0.0.1:$PORT/health")"

# ── run the caller's command against it ──────────────────────────────────────
set +e
VALORI_TEST_ENDPOINT="http://127.0.0.1:$PORT" \
VALORI_TEST_MODE=standalone \
VALORI_TEST_DIM="$DIM" \
VALORI_TEST_STATE_DIR="$STATE_DIR" \
  "$@"
STATUS=$?
set -e

if [ "$STATUS" -ne 0 ]; then
  echo "--- node log (last 60 lines) ---" >&2
  tail -60 "$STATE_DIR/node.log" >&2
fi

exit "$STATUS"
