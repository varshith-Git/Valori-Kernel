#!/usr/bin/env bash
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
#
# Regenerate sdk/python/generated/ from the canonical OpenAPI contract.
#
#   api/openapi/valori-v1.yaml ──openapi-python-client──▶ generated/valori_generated
#
# MACHINE-OWNED OUTPUT. Never hand-edit anything under generated/. Edit the
# Rust `#[utoipa::path]` annotations, re-run the contract gate, then re-run
# this script.
#
# Versions come from sdk/generator.lock.json (Phase API-4A §2) — there is no
# `latest` anywhere in this pipeline, because §13 requires that a second
# generation be byte-identical to the first.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SDK="$ROOT/sdk/python"
SPEC="$ROOT/api/openapi/valori-v1.yaml"
OUT="$SDK/generated/valori_generated"
LOCK="$ROOT/sdk/generator.lock.json"

# Where the pinned generator toolchain lives. Overridable so CI can reuse a
# cached venv instead of rebuilding it on every job.
VENV="${VALORI_SDK_GEN_VENV:-$ROOT/.sdk-toolchain/python}"

GEN_VERSION="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["python"]["version"])' "$LOCK")"
FMT_VERSION="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["python"]["formatter_version"])' "$LOCK")"

if [ ! -x "$VENV/bin/openapi-python-client" ] || \
   ! "$VENV/bin/openapi-python-client" --version 2>/dev/null | grep -q "$GEN_VERSION"; then
  echo "provisioning pinned generator toolchain in $VENV"
  rm -rf "$VENV"
  # openapi-python-client 0.26 needs >= 3.9; prefer a modern interpreter when
  # one is present so the generated code matches what CI produces.
  PY="${VALORI_SDK_GEN_PYTHON:-}"
  if [ -z "$PY" ]; then
    for cand in python3.13 python3.12 python3.11 python3.10 python3; do
      if command -v "$cand" >/dev/null 2>&1; then PY="$(command -v "$cand")"; break; fi
    done
  fi
  "$PY" -m venv "$VENV"
  "$VENV/bin/pip" install --quiet --upgrade pip
  "$VENV/bin/pip" install --quiet \
    "openapi-python-client==$GEN_VERSION" "ruff==$FMT_VERSION"
fi

echo "generating $OUT"
echo "     from $SPEC"
echo "     with openapi-python-client $GEN_VERSION + ruff $FMT_VERSION"

rm -rf "$OUT"
mkdir -p "$SDK/generated"

# `--meta none` emits only the importable package — no generated pyproject.toml,
# no generated README. Packaging is owned by sdk/python/pyproject.toml, which is
# handwritten and must not be clobbered by the generator.
PATH="$VENV/bin:$PATH" "$VENV/bin/openapi-python-client" generate \
  --path "$SPEC" \
  --config "$SDK/scripts/openapi-python-client.yaml" \
  --meta none \
  --output-path "$OUT"

# The formatter's cache is machine-local state, not generated source. Leaving it
# behind would make `generated == regenerated` fail on the cache directory alone.
rm -rf "$OUT/.ruff_cache"

# `--meta none` skips the generator's packaging files, including the PEP 561
# marker. The generated code is fully annotated, so without this an installed
# `valori_generated` would be invisible to mypy. Deterministic, empty file —
# `generated == regenerated` still holds.
: > "$OUT/py.typed"

echo "done — generated/ is machine output; review the diff, never hand-edit it"
