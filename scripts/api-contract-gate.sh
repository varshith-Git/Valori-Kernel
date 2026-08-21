#!/usr/bin/env bash
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
#
# Valori API Contract Gate — the single entry point that proves contract
# reproducibility, three-way route equality, schema integrity, and client
# compatibility.
#
# Phase API-3 Recovery rules this gate now obeys:
#
#   * Every number printed is DISCOVERED from the current repository. Nothing
#     is hardcoded, and there is no `$X / $X` tautology.
#   * SDK readiness is COMPUTED from step outcomes and written to
#     docs/api/sdk-readiness.json. It is never read back from a file that a
#     human or an agent could have hand-edited to say "true".
#   * A failing step is reported as failing. The gate exits non-zero.
#
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# The OpenAPI dialect the canonical contract is expected to carry. Changing
# this is a deliberate, documented act — see docs/api/openapi-version-decision.md.
OPENAPI_TARGET_VERSION="3.1.0"

LOG_FILE="$(mktemp)"
GEN_1="$(mktemp)"
GEN_2="$(mktemp)"
TS_1="$(mktemp)"
TS_2="$(mktemp)"
trap 'rm -f "$LOG_FILE" "$GEN_1" "$GEN_2" "$TS_1" "$TS_2"' EXIT

echo "====================================="
echo " VALORI API CONTRACT GATE"
echo "====================================="

declare -a STEP_NAME=()
declare -a STEP_RESULT=()
declare -a BLOCKERS=()
GATE_FAILURES=0

run_step() {
  local label="$1"
  local title="$2"
  local blocker="$3"
  shift 3

  echo "[$label] $title..."
  if "$@" > "$LOG_FILE" 2>&1; then
    echo "      PASS: $title"
    STEP_NAME+=("$title")
    STEP_RESULT+=("PASS")
  else
    echo "      FAIL: $title"
    echo "      -- failure log tail (last 25 lines) --"
    tail -n 25 "$LOG_FILE" | sed 's/^/      /'
    echo "      --------------------------------------"
    STEP_NAME+=("$title")
    STEP_RESULT+=("FAIL")
    BLOCKERS+=("$blocker")
    GATE_FAILURES=$((GATE_FAILURES + 1))
  fi
}

echo ""
echo "CONTRACT GENERATION"
echo "-------------------"

run_step "1/9" "Route discovery from Rust router source" \
  "Route discovery is incomplete — the Rust route inventory cannot be established." \
  python3 scripts/generate-route-manifest.py

run_step "2/9" "Utoipa OpenAPI generation" \
  "The utoipa OpenAPI generator does not build or does not render." \
  cargo run -q -p valori-node --features utoipa --bin valori-openapi -- \
    --output api/openapi/valori-v1.yaml

echo ""
echo "CONTRACT INTEGRITY"
echo "------------------"

# Covers three-way route equality, operationId agreement, the public/non-public
# boundary, 4xx coverage, and response-body typing. The step reports which of
# those failed; the discovered counts printed in the summary below distinguish a
# route-equality failure from a response-typing one.
run_step "3/9" "Contract integrity (routes, operationIds, response typing)" \
  "Contract integrity check failed — run scripts/verify-api-route-contract.py for the itemised list." \
  python3 scripts/verify-api-route-contract.py

# Phase API-3.3. Step 3 proves the right operations are PRESENT; this proves
# their HTTP contracts are COMPLETE. It cross-checks each operation against its
# Rust handler signature, so a declared `requestBody?: never` on an endpoint
# that plainly takes JSON is a failure, not a silent hole.
run_step "3b/9" "Public operation completeness" \
  "One or more public operations carry an incomplete HTTP contract — run scripts/audit-public-api-operations.py for the itemised list." \
  python3 scripts/audit-public-api-operations.py

run_step "4/9" "Generated schema conformance" \
  "Generated schema set does not conform to the committed contract." \
  cargo test -q -p valori-node --features utoipa --test openapi_generated

run_step "5/9" "OpenAPI lint" \
  "api/openapi/valori-v1.yaml does not lint clean." \
  npx --yes @redocly/cli@latest lint api/openapi/valori-v1.yaml

echo ""
echo "RUNTIME CONFORMANCE"
echo "-------------------"

run_step "6a/9" "API contract integration suite" \
  "API contract integration tests fail." \
  cargo test -q -p valori-node --features utoipa --test api_contract

run_step "6b/9" "Standalone/cluster route parity" \
  "Standalone and cluster routers expose different route sets." \
  cargo test -q -p valori-node --test route_parity

echo ""
echo "CLIENT COMPATIBILITY"
echo "--------------------"

run_step "7a/9" "TypeScript wire types generation" \
  "TypeScript wire types do not generate from the contract." \
  ./scripts/generate-api-types.sh

echo "[7b/9] Generator reproducibility (two consecutive runs)..."
REPRO_OK=1
cargo run -q -p valori-node --features utoipa --bin valori-openapi > "$GEN_1" 2>/dev/null || REPRO_OK=0
cargo run -q -p valori-node --features utoipa --bin valori-openapi > "$GEN_2" 2>/dev/null || REPRO_OK=0
if [ -f ui/api-types/src/valori-v1.ts ]; then
  ./scripts/generate-api-types.sh > /dev/null 2>&1 && cp ui/api-types/src/valori-v1.ts "$TS_1"
  ./scripts/generate-api-types.sh > /dev/null 2>&1 && cp ui/api-types/src/valori-v1.ts "$TS_2"
else
  REPRO_OK=0
fi
if [ "$REPRO_OK" = "1" ] && cmp -s "$GEN_1" "$GEN_2" && cmp -s "$TS_1" "$TS_2"; then
  echo "      PASS: Generator reproducibility"
  STEP_NAME+=("Generator reproducibility"); STEP_RESULT+=("PASS")
else
  echo "      FAIL: Generator reproducibility"
  STEP_NAME+=("Generator reproducibility"); STEP_RESULT+=("FAIL")
  BLOCKERS+=("Contract generation is not byte-reproducible across runs.")
  GATE_FAILURES=$((GATE_FAILURES + 1))
fi

run_step "7c/9" "TypeScript wire types compile" \
  "ui/api-types does not typecheck against the generated contract — the contract is missing schemas the client references." \
  bash -c 'cd ui && npx tsc --noEmit'

run_step "8/9" "Python remote API compatibility" \
  "Python remote client contract tests fail." \
  python3 -m pytest -q python/tests/test_protocol_remote.py python/tests/test_python_remote.py \
                       python/tests/test_create_collection_contract.py

# ── Discovered coverage numbers ─────────────────────────────────────────────
#
# Every figure below is read out of artifacts produced by this run. If any
# source is unavailable the gate says UNKNOWN — it never substitutes a default.

COVERAGE="$(python3 - <<'PY'
import json, subprocess, sys
try:
    import yaml
except ImportError:
    print("UNKNOWN"); sys.exit(0)

try:
    man = json.load(open("docs/api/phase-api-3-route-manifest.json"))
    rust_public = man["totals"]["public_sdk_routes"]
    rust_total = man["totals"]["routes"]

    gen = yaml.safe_load(subprocess.check_output(
        ["cargo", "run", "-q", "-p", "valori-node", "--features", "utoipa",
         "--bin", "valori-openapi"], stderr=subprocess.DEVNULL).decode()) or {}
    methods = {"get","post","put","delete","patch","head","options","trace"}
    gen_ops = sum(1 for p, i in (gen.get("paths") or {}).items()
                    for m in (i or {}) if m.lower() in methods)
    gen_schemas = len((gen.get("components") or {}).get("schemas") or {})
    gen_version = gen.get("openapi", "?")

    doc = yaml.safe_load(open("api/openapi/valori-v1.yaml")) or {}
    doc_ops = sum(1 for p, i in (doc.get("paths") or {}).items()
                    for m in (i or {}) if m.lower() in methods)
    doc_schemas = len((doc.get("components") or {}).get("schemas") or {})

    # §24: the three-way diff counts, computed here from the same three
    # sources the verifier uses. Nothing below is a constant.
    methods_l = methods
    def ops_of(doc):
        out = {}
        for pth, item in (doc.get("paths") or {}).items():
            for meth, o in (item or {}).items():
                if meth.lower() in methods_l:
                    out[(meth.lower(), pth)] = o or {}
        return out

    import re as _re
    def to_openapi_path(s):
        return _re.sub(r":([A-Za-z_][A-Za-z0-9_]*)", r"{\1}", s)

    rust_ops = {
        (r["method"].lower(), to_openapi_path(r["path"])): r
        for r in man["routes"] if r["public_sdk_export"]
    }
    nonpublic = {
        (r["method"].lower(), to_openapi_path(r["path"]))
        for r in man["routes"] if not r["public_sdk_export"]
    }
    g = ops_of(gen)
    c = ops_of(doc)
    missing = len(set(rust_ops) - set(g)) + len(set(rust_ops) - set(c))
    unexpected = len(set(g) - set(rust_ops)) + len(set(c) - set(rust_ops) - nonpublic)
    opid = sum(
        1 for k in set(rust_ops) & set(g) & set(c)
        if g[k].get("operationId") != rust_ops[k]["operation_id"]
        or c[k].get("operationId") != rust_ops[k]["operation_id"]
    )
    leaks = len(set(c) & nonpublic)

    print(f"{rust_total}:{rust_public}:{gen_ops}:{doc_ops}:{gen_schemas}:{doc_schemas}:"
          f"{gen_version}:{missing}:{unexpected}:{opid}:{leaks}")
except Exception as e:
    print("UNKNOWN")
PY
)"

if [ "$COVERAGE" = "UNKNOWN" ]; then
  RUST_TOTAL="?"; RUST_PUBLIC="?"; GEN_OPS="?"; DOC_OPS="?"
  GEN_SCHEMAS="?"; DOC_SCHEMAS="?"; GEN_VERSION="?"
  DIFF_MISSING="?"; DIFF_UNEXPECTED="?"; DIFF_OPID="?"; DIFF_LEAKS="?"
  BLOCKERS+=("Coverage statistics could not be discovered from the repository.")
  GATE_FAILURES=$((GATE_FAILURES + 1))
else
  IFS=':' read -r RUST_TOTAL RUST_PUBLIC GEN_OPS DOC_OPS GEN_SCHEMAS DOC_SCHEMAS \
                  GEN_VERSION DIFF_MISSING DIFF_UNEXPECTED DIFF_OPID DIFF_LEAKS <<< "$COVERAGE"
  if [ "${DIFF_OPID:-0}" != "0" ]; then
    BLOCKERS+=("$DIFF_OPID operation(s) disagree on operationId across Rust, utoipa, and the contract.")
  fi
  if [ "${DIFF_LEAKS:-0}" != "0" ]; then
    BLOCKERS+=("$DIFF_LEAKS non-public route(s) appear in the public SDK contract.")
  fi
  if [ "$GEN_OPS" != "$RUST_PUBLIC" ]; then
    BLOCKERS+=("Utoipa generates $GEN_OPS of $RUST_PUBLIC public operations; $((RUST_PUBLIC - GEN_OPS)) public routes still have no #[utoipa::path].")
  fi
  if [ "$GEN_OPS" != "$DOC_OPS" ]; then
    BLOCKERS+=("api/openapi/valori-v1.yaml carries $DOC_OPS operations but utoipa emits $GEN_OPS; the committed contract is not the generator's output.")
  fi
  if [ "$GEN_VERSION" != "$OPENAPI_TARGET_VERSION" ]; then
    BLOCKERS+=("Generated document is OpenAPI $GEN_VERSION; the contract target is $OPENAPI_TARGET_VERSION.")
  fi
fi

# Response-body typing (§9): how many documented responses claim no body while
# the handler actually sends JSON. Computed by the same allowlist the verifier
# uses, so the two can never disagree.
UNTYPED_RESPONSES="$(python3 - <<'PY'
import sys
sys.path.insert(0, "scripts")
try:
    import importlib.util, yaml
    spec = importlib.util.spec_from_file_location(
        "vc", "scripts/verify-api-route-contract.py")
    vc = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(vc)
    doc = yaml.safe_load(open("api/openapi/valori-v1.yaml")) or {}
    print(len(vc.check_empty_bodies(vc.extract_ops(doc))))
except Exception:
    print("?")
PY
)"
if [ "$UNTYPED_RESPONSES" != "0" ] && [ "$UNTYPED_RESPONSES" != "?" ]; then
  BLOCKERS+=("$UNTYPED_RESPONSES documented response(s) declare no body while the handler returns JSON; a generated SDK sees \`never\` and cannot surface the message.")
fi

# ── Operation completeness (§17) ────────────────────────────────────────────
#
# Read out of the audit artifact this run produced. Every figure is discovered;
# none is a constant, and a missing artifact reports UNKNOWN rather than zero.
OPS="$(python3 - <<'PY'
import json
try:
    t = json.load(open("docs/api/public-operation-audit.json"))["totals"]
    print(":".join(str(t[k]) for k in (
        "public_operations", "complete", "incomplete",
        "complete_requests", "incomplete_requests",
        "complete_responses", "incomplete_responses",
        "untyped_parameters", "parameter_mismatches",
        "errors_without_body", "errors_not_api_error",
        "security_mismatches", "untyped_schema_properties",
    )))
except Exception:
    print("UNKNOWN")
PY
)"
if [ "$OPS" = "UNKNOWN" ]; then
  OP_TOTAL="?"; OP_COMPLETE="?"; OP_INCOMPLETE="?"
  REQ_OK="?"; REQ_BAD="?"; RESP_OK="?"; RESP_BAD="?"
  PARAM_UNTYPED="?"; PARAM_MISMATCH="?"; ERR_NOBODY="?"; ERR_OTHER="?"
  SEC_MISMATCH="?"; SCHEMA_HOLES="?"
  BLOCKERS+=("Operation-completeness statistics could not be discovered — docs/api/public-operation-audit.json is missing or unreadable.")
  GATE_FAILURES=$((GATE_FAILURES + 1))
else
  IFS=':' read -r OP_TOTAL OP_COMPLETE OP_INCOMPLETE REQ_OK REQ_BAD \
                  RESP_OK RESP_BAD PARAM_UNTYPED PARAM_MISMATCH \
                  ERR_NOBODY ERR_OTHER SEC_MISMATCH SCHEMA_HOLES <<< "$OPS"
  [ "${OP_INCOMPLETE:-0}" != "0" ] && \
    BLOCKERS+=("$OP_INCOMPLETE of $OP_TOTAL public operation(s) have an incomplete HTTP contract.")
  [ "${REQ_BAD:-0}" != "0" ] && \
    BLOCKERS+=("$REQ_BAD operation(s) have an incomplete request contract.")
  [ "${RESP_BAD:-0}" != "0" ] && \
    BLOCKERS+=("$RESP_BAD operation(s) have an incomplete response contract.")
  [ "${PARAM_UNTYPED:-0}" != "0" ] && \
    BLOCKERS+=("$PARAM_UNTYPED declared parameter(s) carry no schema.")
  [ "${PARAM_MISMATCH:-0}" != "0" ] && \
    BLOCKERS+=("$PARAM_MISMATCH operation(s) declare parameters that disagree with the handler's extractors.")
  [ "${ERR_NOBODY:-0}" != "0" ] && \
    BLOCKERS+=("$ERR_NOBODY documented error response(s) declare no body while the runtime sends ApiError.")
  [ "${SEC_MISMATCH:-0}" != "0" ] && \
    BLOCKERS+=("$SEC_MISMATCH operation(s) document security that disagrees with what the server enforces.")
  [ "${SCHEMA_HOLES:-0}" != "0" ] && \
    BLOCKERS+=("$SCHEMA_HOLES schema property/properties carry no type — an SDK sees \`unknown\`/\`Any\` there.")
fi

# ── Generated-client representation quality (§13) ────────────────────────────
#
# `unknown`, `any` and `never` are not banned outright — openapi-typescript
# emits `[name: string]: unknown` as the response-HEADERS index signature on
# every operation, and a genuinely empty 204 must be `content?: never`. What
# must be zero is the UNEXPECTED kind: an `unknown` standing in for a request
# or response body, or an untyped-bag schema with no real field.
TSQ="$(python3 - <<'PY'
import re, json
try:
    src = open("ui/api-types/src/valori-v1.ts").read()
    # Header index signatures and prose in doc comments are expected.
    lines = src.splitlines()
    unexpected = 0
    for ln in lines:
        s = ln.strip()
        if s.startswith(("*", "/*", "//")):
            continue                      # documentation prose
        if "[name: string]: unknown" in s or "[key: string]: unknown" in s:
            continue                      # response-headers map from the generator
        if re.search(r"\bunknown\b|(?<![A-Za-z])any\b", s):
            unexpected += 1
    # Untyped bags: a components.schemas entry whose only member is an index
    # signature. Those are the ones that give an SDK user nothing.
    bags = 0
    m = re.search(r"schemas:\s*\{(.*?)\n    \};", src, re.S)
    if m:
        for mm in re.finditer(r"\n        (\w+):\s*\{(.*?)\n        \};", m.group(1), re.S):
            fields = [l.strip() for l in mm.group(2).splitlines()
                      if l.strip() and not l.strip().startswith(("/*", "*", "//"))]
            if fields and not [f for f in fields if not f.startswith("[key: string]")]:
                bags += 1
    never_content = len(re.findall(r"content\?: never", src))
    never_body = len(re.findall(r"requestBody\?: never", src))
    print(f"{unexpected}:{bags}:{never_content}:{never_body}")
except Exception:
    print("UNKNOWN")
PY
)"
if [ "$TSQ" = "UNKNOWN" ]; then
  TS_UNEXPECTED="?"; TS_BAGS="?"; TS_NEVER_CONTENT="?"; TS_NEVER_BODY="?"
else
  IFS=':' read -r TS_UNEXPECTED TS_BAGS TS_NEVER_CONTENT TS_NEVER_BODY <<< "$TSQ"
  [ "${TS_UNEXPECTED:-0}" != "0" ] && \
    BLOCKERS+=("$TS_UNEXPECTED unexpected \`unknown\`/\`any\` occurrence(s) in the generated TypeScript API surface.")
  [ "${TS_BAGS:-0}" != "0" ] && \
    BLOCKERS+=("$TS_BAGS generated schema(s) have no typed field at all — an SDK user gets an untyped bag.")
fi

GATE_RESULT="PASS"
[ "$GATE_FAILURES" -gt 0 ] && GATE_RESULT="FAIL"

# ── Compute SDK readiness from the results above (never hand-written) ────────
SDK_READY="NO"
[ "${#BLOCKERS[@]}" -eq 0 ] && [ "$GATE_RESULT" = "PASS" ] && SDK_READY="YES"

python3 - "$SDK_READY" "$GATE_RESULT" "${BLOCKERS[@]+"${BLOCKERS[@]}"}" <<'PY'
import json, sys, datetime
ready = sys.argv[1] == "YES"
gate = sys.argv[2]
blockers = sys.argv[3:]
json.dump({
    "sdk_ready": ready,
    "computed_by": "scripts/api-contract-gate.sh",
    "computed_at": datetime.datetime.now(datetime.timezone.utc)
                     .replace(microsecond=0).isoformat(),
    "gate_result": gate,
    "blocker_count": len(blockers),
    "blockers": blockers,
}, open("docs/api/sdk-readiness.json", "w"), indent=2)
open("docs/api/sdk-readiness.json", "a").write("\n")
PY

echo ""
echo "-------------------------------------"
echo " VALORI API CONTRACT GATE SUMMARY"
echo "-------------------------------------"
for i in "${!STEP_NAME[@]}"; do
  printf "   %-46s %s\n" "${STEP_NAME[$i]}" "${STEP_RESULT[$i]}"
done
echo "-------------------------------------"
echo " ROUTE CONTRACT (discovered):"
echo "   Rust routes registered:      $RUST_TOTAL"
echo "   Rust public routes:          $RUST_PUBLIC"
echo "   Utoipa operations:           $GEN_OPS"
echo "   OpenAPI operations:          $DOC_OPS"
echo "   Missing (Rust -> utoipa/doc): $DIFF_MISSING"
echo "   Unexpected (not in Rust):     $DIFF_UNEXPECTED"
echo "   operationId mismatches:       $DIFF_OPID"
echo "   Classification leaks:         $DIFF_LEAKS"
echo ""
echo " SCHEMA COVERAGE (discovered):"
echo "   Utoipa schemas:              $GEN_SCHEMAS"
echo "   OpenAPI schemas:             $DOC_SCHEMAS"
echo "   Untyped JSON responses:      $UNTYPED_RESPONSES"
echo ""
echo " OPERATION COMPLETENESS (discovered):"
echo "   Public operations:           $OP_TOTAL"
echo "   Complete:                    $OP_COMPLETE"
echo "   Incomplete:                  $OP_INCOMPLETE"
echo "   Complete requests:           $REQ_OK"
echo "   Incomplete requests:         $REQ_BAD"
echo "   Complete responses:          $RESP_OK"
echo "   Incomplete responses:        $RESP_BAD"
echo "   Untyped parameters:          $PARAM_UNTYPED"
echo "   Parameter mismatches:        $PARAM_MISMATCH"
echo "   Errors with no body:         $ERR_NOBODY"
echo "   Errors not ApiError:         $ERR_OTHER (deliberate status-report bodies)"
echo "   Security mismatches:         $SEC_MISMATCH"
echo "   Untyped schema properties:   $SCHEMA_HOLES"
echo ""
echo " GENERATED CLIENT QUALITY (discovered):"
echo "   Unexpected unknown/any:      $TS_UNEXPECTED"
echo "   Untyped bag schemas:         $TS_BAGS"
echo "   content?: never:             $TS_NEVER_CONTENT (expected: genuinely empty bodies)"
echo "   requestBody?: never:         $TS_NEVER_BODY (expected: operations taking no body)"
echo ""
echo " OPENAPI VERSION EMITTED:       $GEN_VERSION (target $OPENAPI_TARGET_VERSION)"
echo "-------------------------------------"
echo " CONTRACT GATE: $GATE_RESULT"
echo ""
echo " SDK READINESS: $SDK_READY"
echo " SDK BLOCKERS (${#BLOCKERS[@]}):"
for b in ${BLOCKERS[@]+"${BLOCKERS[@]}"}; do
  echo "   - $b"
done
echo " (written to docs/api/sdk-readiness.json)"
echo "====================================="

if [ "$GATE_RESULT" = "PASS" ] && [ "${#BLOCKERS[@]}" -eq 0 ]; then
  exit 0
fi
exit 1
