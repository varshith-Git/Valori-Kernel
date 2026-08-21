#!/usr/bin/env bash
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
#
# Regenerate the TypeScript wire model from the canonical OpenAPI contract.
#
#   api/openapi/valori-v1.yaml  ──openapi-typescript──▶  ui/api-types/src/valori-v1.ts
#
# `ui/api-types/src/valori-v1.ts` is machine output. Never hand-edit it — edit
# the contract and re-run this script. `ui/api-types/src/index.ts` IS
# hand-written: it only maps the generated `components["schemas"][…]` index
# types onto the short aliases the UI uses, so a renamed or deleted schema
# becomes a TypeScript error there instead of silent drift.
#
# Both `ui/src` and `ui/studio` consume `@valori/api-types`; neither keeps its
# own copy of the wire model any more.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPEC="$ROOT/api/openapi/valori-v1.yaml"
OUT="$ROOT/ui/api-types/src/valori-v1.ts"

if [ ! -f "$SPEC" ]; then
  echo "error: contract not found at $SPEC" >&2
  exit 1
fi

echo "generating $OUT from $SPEC"
npx --yes openapi-typescript@7 "$SPEC" -o "$OUT"
echo "done — review the diff before committing"
