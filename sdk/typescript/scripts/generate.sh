#!/usr/bin/env bash
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
#
# Regenerate sdk/typescript/generated/ from the canonical OpenAPI contract.
#
#   api/openapi/valori-v1.yaml ──swagger-typescript-api──▶ generated/valori-api.ts
#
# MACHINE-OWNED OUTPUT. Never hand-edit generated/valori-api.ts.
#
# Two deliberate, deterministic post-processing steps are applied:
#
#   1. `// @ts-nocheck` is stripped. The generator emits it unconditionally;
#      leaving it in would mean the generated API surface is never typechecked,
#      which defeats the point of a typed SDK. Phase API-3.3 proved the contract
#      produces compiling TypeScript, so we hold the generated file to it.
#   2. The generator's `Api` class is exported under the stable name
#      `GeneratedApi` so the handwritten layer's import does not depend on the
#      generator's default class name.
#
# Both steps are pure `sed` on fixed strings — idempotent and byte-stable, so
# §13 (`generated == regenerated`) still holds.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SDK="$ROOT/sdk/typescript"
SPEC="$ROOT/api/openapi/valori-v1.yaml"
OUT_DIR="$SDK/generated"
OUT="$OUT_DIR/valori-api.ts"
LOCK="$ROOT/sdk/generator.lock.json"

GEN_VERSION="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["typescript"]["version"])' "$LOCK")"

echo "generating $OUT"
echo "     from $SPEC"
echo "     with swagger-typescript-api $GEN_VERSION"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

npx --yes "swagger-typescript-api@$GEN_VERSION" generate \
  --path "$SPEC" \
  --output "$OUT_DIR" \
  --name "valori-api.ts" \
  --api-class-name "GeneratedApi" \
  --single-http-client \
  --module-name-index 0 \
  --sort-routes \
  --sort-types \
  --silent

# Deterministic post-processing (see header).
python3 - "$OUT" <<'PY'
import sys
path = sys.argv[1]
src = open(path, encoding="utf-8").read()
lines = [l for l in src.splitlines() if l.strip() != "// @ts-nocheck"]
banner = (
    "//\n"
    "// GENERATED FILE — DO NOT EDIT.\n"
    "// Source of truth: api/openapi/valori-v1.yaml\n"
    "// Regenerate with: sdk/typescript/scripts/generate.sh\n"
    "//"
)
out = "\n".join(lines).rstrip("\n") + "\n"
open(path, "w", encoding="utf-8").write(banner + "\n" + out)
PY

echo "done — generated/ is machine output; review the diff, never hand-edit it"
