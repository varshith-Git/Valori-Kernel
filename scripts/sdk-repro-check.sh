#!/usr/bin/env bash
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
#
# SDK generation reproducibility gate — Phase API-4A §13/§21.
#
# Proves two things about every generated SDK tree:
#
#   1. The committed generated/ is exactly what the pinned generator produces
#      from api/openapi/valori-v1.yaml right now. A hand-edit of machine output,
#      or a stale regeneration, fails here.
#   2. A second generation is byte-identical to the first — no timestamps, no
#      absolute paths, no random ordering.
#
# Both are checked by regenerating into a scratch directory and diffing; the
# working tree is only touched if --write is passed.
#
#   scripts/sdk-repro-check.sh            # check both, leave the tree alone
#   scripts/sdk-repro-check.sh --sdk python
#   scripts/sdk-repro-check.sh --write    # regenerate in place (for a bump)
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGETS="python typescript"
WRITE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --sdk) TARGETS="$2"; shift 2 ;;
    --write) WRITE=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

FAILURES=0

echo "====================================="
echo " VALORI SDK REPRODUCIBILITY CHECK"
echo "====================================="
echo " contract: api/openapi/valori-v1.yaml"
echo ""

# Compare two trees or two files. `.ruff_cache` is removed by the generator
# script itself; `__pycache__` is interpreter output that appears whenever the
# SDK has been imported (an editable install, a test run) and is never committed.
# Everything else that differs is a real difference.
compare() {
  if [ -d "$1" ]; then
    diff -r -x '__pycache__' -x '*.pyc' "$1" "$2" > "$SCRATCH/diff.txt" 2>&1
  else
    diff "$1" "$2" > "$SCRATCH/diff.txt" 2>&1
  fi
}

check_python() {
  local live="sdk/python/generated/valori_generated"
  local first="$SCRATCH/py-1"
  local second="$SCRATCH/py-2"

  echo "[python] regenerating twice with the pinned toolchain..."
  cp -R "$live" "$SCRATCH/py-committed" 2>/dev/null || {
    echo "      FAIL: no committed generated tree at $live"
    FAILURES=$((FAILURES + 1)); return
  }

  ./sdk/python/scripts/generate.sh > "$SCRATCH/py.log" 2>&1 || {
    echo "      FAIL: generation failed"; tail -n 20 "$SCRATCH/py.log" | sed 's/^/      /'
    FAILURES=$((FAILURES + 1)); return
  }
  cp -R "$live" "$first"

  ./sdk/python/scripts/generate.sh >> "$SCRATCH/py.log" 2>&1 || {
    echo "      FAIL: second generation failed"
    FAILURES=$((FAILURES + 1)); return
  }
  cp -R "$live" "$second"

  if compare "$first" "$second"; then
    echo "      PASS: generation is byte-stable across two runs"
  else
    echo "      FAIL: two consecutive generations differ"
    head -n 20 "$SCRATCH/diff.txt" | sed 's/^/      /'
    FAILURES=$((FAILURES + 1))
  fi

  if compare "$SCRATCH/py-committed" "$second"; then
    echo "      PASS: committed generated/ matches the generator's output"
  else
    echo "      FAIL: committed generated/ is not what the generator produces"
    echo "            (re-run sdk/python/scripts/generate.sh and commit the diff)"
    head -n 20 "$SCRATCH/diff.txt" | sed 's/^/      /'
    FAILURES=$((FAILURES + 1))
    [ "$WRITE" = "0" ] && rm -rf "$live" && cp -R "$SCRATCH/py-committed" "$live"
  fi
}

check_typescript() {
  local live="sdk/typescript/generated/valori-api.ts"
  local first="$SCRATCH/ts-1.ts"
  local second="$SCRATCH/ts-2.ts"

  echo "[typescript] regenerating twice with the pinned toolchain..."
  cp "$live" "$SCRATCH/ts-committed.ts" 2>/dev/null || {
    echo "      FAIL: no committed generated file at $live"
    FAILURES=$((FAILURES + 1)); return
  }

  ./sdk/typescript/scripts/generate.sh > "$SCRATCH/ts.log" 2>&1 || {
    echo "      FAIL: generation failed"; tail -n 20 "$SCRATCH/ts.log" | sed 's/^/      /'
    FAILURES=$((FAILURES + 1)); return
  }
  cp "$live" "$first"

  ./sdk/typescript/scripts/generate.sh >> "$SCRATCH/ts.log" 2>&1 || {
    echo "      FAIL: second generation failed"
    FAILURES=$((FAILURES + 1)); return
  }
  cp "$live" "$second"

  if compare "$first" "$second"; then
    echo "      PASS: generation is byte-stable across two runs"
  else
    echo "      FAIL: two consecutive generations differ"
    head -n 20 "$SCRATCH/diff.txt" | sed 's/^/      /'
    FAILURES=$((FAILURES + 1))
  fi

  if compare "$SCRATCH/ts-committed.ts" "$second"; then
    echo "      PASS: committed generated/ matches the generator's output"
  else
    echo "      FAIL: committed generated/ is not what the generator produces"
    echo "            (re-run sdk/typescript/scripts/generate.sh and commit the diff)"
    head -n 20 "$SCRATCH/diff.txt" | sed 's/^/      /'
    FAILURES=$((FAILURES + 1))
    [ "$WRITE" = "0" ] && cp "$SCRATCH/ts-committed.ts" "$live"
  fi
}

for target in $TARGETS; do
  case "$target" in
    python) check_python ;;
    typescript) check_typescript ;;
    *) echo "unknown SDK: $target" >&2; exit 2 ;;
  esac
  echo ""
done

echo "-------------------------------------"
if [ "$FAILURES" -eq 0 ]; then
  echo " SDK REPRODUCIBILITY: PASS"
  echo "====================================="
  exit 0
fi
echo " SDK REPRODUCIBILITY: FAIL ($FAILURES problem(s))"
echo "====================================="
exit 1
