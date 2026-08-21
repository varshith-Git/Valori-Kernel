#!/usr/bin/env python3
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
Valori three-way API route contract verifier — Phase API-3 Recovery §7 / §20 / §23.

Proves the invariant:

    Rust registered public routes
        ==
    Utoipa generated public operations
        ==
    canonical OpenAPI public operations

VERIFICATION ONLY. This script MUST NOT generate or modify OpenAPI. It reads
three sources and diffs them:

  1. Rust      — docs/api/phase-api-3-route-manifest.json, which
                 scripts/generate-route-manifest.py derives from axum router
                 source. Regenerated here so a stale manifest cannot pass.
  2. Utoipa    — the live output of `cargo run -p valori-node --features utoipa
                 --bin valori-openapi`, rendered in-memory.
  3. OpenAPI   — the committed api/openapi/valori-v1.yaml.

Exit code 0 only when all three agree on method, path, operationId, and
classification.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

try:
    import yaml
except ImportError:
    print("error: PyYAML is required (pip install pyyaml)", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "docs" / "api" / "phase-api-3-route-manifest.json"
CANONICAL = ROOT / "api" / "openapi" / "valori-v1.yaml"

HTTP_METHODS = {"get", "post", "put", "delete", "patch", "head", "options", "trace"}


def axum_to_openapi_path(p: str) -> str:
    """`/v1/records/:id` -> `/v1/records/{id}`.

    Representation only. This changes no semantics and invents nothing.
    """
    return re.sub(r":([A-Za-z_][A-Za-z0-9_]*)", r"{\1}", p)


def load_rust_routes() -> tuple[dict[tuple[str, str], dict], list[str]]:
    """Regenerate and load the Rust-derived manifest."""
    notes = []
    proc = subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "generate-route-manifest.py")],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    if proc.returncode != 0:
        print("ROUTE DISCOVERY FAILED — cannot verify a contract against an", file=sys.stderr)
        print("incomplete route inventory.\n", file=sys.stderr)
        print(proc.stderr, file=sys.stderr)
        sys.exit(1)
    notes.append(proc.stdout.strip().splitlines()[0] if proc.stdout.strip() else "")

    data = json.loads(MANIFEST.read_text())
    out = {}
    for r in data["routes"]:
        if not r["public_sdk_export"]:
            continue
        out[(r["method"].lower(), axum_to_openapi_path(r["path"]))] = r
    return out, data["totals"]


def load_utoipa_ops() -> dict[tuple[str, str], dict]:
    proc = subprocess.run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "valori-node",
            "--features",
            "utoipa",
            "--bin",
            "valori-openapi",
        ],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    if proc.returncode != 0:
        print("UTOIPA GENERATION FAILED", file=sys.stderr)
        print(proc.stderr[-3000:], file=sys.stderr)
        sys.exit(1)
    doc = yaml.safe_load(proc.stdout) or {}
    return extract_ops(doc)


def load_canonical_ops() -> dict[tuple[str, str], dict]:
    if not CANONICAL.exists():
        print(f"error: {CANONICAL} not found", file=sys.stderr)
        sys.exit(1)
    return extract_ops(yaml.safe_load(CANONICAL.read_text()) or {})


def extract_ops(doc: dict) -> dict[tuple[str, str], dict]:
    ops = {}
    for path, item in (doc.get("paths") or {}).items():
        for method, op in (item or {}).items():
            if method.lower() in HTTP_METHODS:
                ops[(method.lower(), path)] = op or {}
    return ops


def fmt(keys) -> list[str]:
    return [f"{m.upper()} {p}" for m, p in sorted(keys, key=lambda k: (k[1], k[0]))]


# ── Empty-body allowlist (Phase API-3.2 §9) ──────────────────────────────────
#
# A response declared with no `content` becomes `content?: never` in the
# generated TypeScript, and its equivalent in every other generated SDK: the
# client is told there is nothing to read. That is correct only when the
# handler really does send an empty body.
#
# Each entry below was verified by reading the handler. Anything NOT listed
# here that declares no content is reported as a defect, because the default
# assumption is that a Valori endpoint answers in JSON.
#
# Phase API-3.3 removed the blanket 401/403 exemption that used to live here.
# It rested on "auth_guard_v2 returns a bare StatusCode, which axum renders
# with an empty body" — true of the guard alone, false of the router. Both
# routers install `attach_error_code` as their OUTERMOST layer, and it
# synthesises a full `{"error","code"}` object for an empty error body.
# `tests/api_contract.rs::unauthorized_has_a_parseable_json_body_with_a_code`
# proves it. 401 and 403 are now checked like every other error status.
EMPTY_BODY_ALWAYS_OK: set[str] = set()

EMPTY_BODY_OK = {
    # `drop_collection` returns `StatusCode::NO_CONTENT`. 204 must not carry a
    # body at all.
    ("delete", "/v1/namespaces/{name}", "204"),
    # `restore` is `async fn(...) -> Result<(), EngineError>`; the success arm
    # is the unit type, so 200 is genuinely empty.
    ("post", "/v1/snapshot/upload", "200"),
}


def check_empty_bodies(canonical: dict) -> list[str]:
    out = []
    for (method, path), op in sorted(canonical.items()):
        for code, resp in sorted((op.get("responses") or {}).items()):
            code = str(code)
            if code in EMPTY_BODY_ALWAYS_OK:
                continue
            if (method, path, code) in EMPTY_BODY_OK:
                continue
            if not (resp or {}).get("content"):
                out.append(
                    f"{method.upper()} {path} -> {code} declares no content "
                    f"({(resp or {}).get('description', '')[:60]})"
                )
    return out


def main() -> int:
    rust, totals = load_rust_routes()
    utoipa = load_utoipa_ops()
    canonical = load_canonical_ops()

    rust_keys = set(rust)
    utoipa_keys = set(utoipa)
    canonical_keys = set(canonical)

    manifest_all = json.loads(MANIFEST.read_text())["routes"]
    nonpublic = {
        (r["method"].lower(), axum_to_openapi_path(r["path"])): r["classification"]
        for r in manifest_all
        if not r["public_sdk_export"]
    }

    missing_utoipa = rust_keys - utoipa_keys
    missing_openapi = rust_keys - canonical_keys
    unexpected_utoipa = utoipa_keys - rust_keys
    # An operation in the contract that no router registers at all is synthetic.
    # An operation that *is* registered but classified non-public is a
    # classification error, not a synthetic path — report it once, as the latter.
    unexpected_openapi = canonical_keys - rust_keys - set(nonpublic)

    # operationId agreement across all three, for routes present everywhere.
    op_mismatches = []
    for key in sorted(rust_keys & utoipa_keys & canonical_keys):
        want = rust[key]["operation_id"]
        got_u = utoipa[key].get("operationId")
        got_c = canonical[key].get("operationId")
        if got_u != want or got_c != want:
            op_mismatches.append((key, want, got_u, got_c))

    # Classification agreement: anything the manifest marks non-public must not
    # appear in the canonical SDK contract.
    classification_violations = [
        (k, nonpublic[k]) for k in sorted(canonical_keys & set(nonpublic))
    ]

    print("=" * 55)
    print(" VALORI THREE-WAY ROUTE CONTRACT")
    print("=" * 55)
    print(f"  Rust public routes:     {len(rust_keys):>5}")
    print(f"  Utoipa operations:      {len(utoipa_keys):>5}")
    print(f"  OpenAPI operations:     {len(canonical_keys):>5}")
    print()
    print(f"  Missing Utoipa:         {len(missing_utoipa):>5}")
    print(f"  Missing OpenAPI:        {len(missing_openapi):>5}")
    print(f"  Unexpected Utoipa:      {len(unexpected_utoipa):>5}")
    print(f"  Unexpected OpenAPI:     {len(unexpected_openapi):>5}")
    print(f"  OperationId mismatches: {len(op_mismatches):>5}")
    print(f"  Classification errors:  {len(classification_violations):>5}")
    print("=" * 55)

    failures = 0

    def report(title, items, limit=15):
        nonlocal failures
        if not items:
            return
        failures += len(items)
        print(f"\n{title} ({len(items)}):")
        for line in items[:limit]:
            print(f"   - {line}")
        if len(items) > limit:
            print(f"   ... and {len(items) - limit} more")

    report("Rust routes with no #[utoipa::path]", fmt(missing_utoipa))
    report("Rust routes absent from canonical OpenAPI", fmt(missing_openapi))
    report("Utoipa operations not registered in any Rust router", fmt(unexpected_utoipa))
    report(
        "OpenAPI operations not registered in any Rust router (synthetic)",
        fmt(unexpected_openapi),
    )
    report(
        "operationId mismatches",
        [
            f"{m.upper()} {p}: rust={w!r} utoipa={u!r} openapi={c!r}"
            for (m, p), w, u, c in op_mismatches
        ],
    )
    report(
        "Non-public routes leaking into the SDK contract",
        [f"{m.upper()} {p} is {cls}" for (m, p), cls in classification_violations],
    )

    # Redocly's `operation-4xx-response` is pinned to `warn` in redocly.yaml
    # only because `openapi-typescript` shares that config and aborts codegen
    # on any error-severity problem. The rule itself is enforced here at full
    # strength, where nothing else consumes the result.
    #
    # `GET /health` is the sole documented exemption: unauthenticated
    # (`security: []`, so no 401/403 is reachable) and it accepts no body, path
    # param, or query param, so no 400 is reachable either. See redocly.yaml
    # and .redocly.lint-ignore.yaml.
    missing_4xx = [
        f"{m.upper()} {p}"
        for (m, p), op in sorted(canonical.items())
        if (m, p) != ("get", "/health")
        and not any(str(c).startswith("4") for c in (op.get("responses") or {}))
    ]
    report(
        "Public operations documenting no 4xx response",
        missing_4xx,
    )
    report(
        "Responses declared with no body that the handler actually fills "
        "(SDK sees `never`)",
        check_empty_bodies(canonical),
        limit=30,
    )

    print()
    if failures:
        print(f"ROUTE CONTRACT: FAIL ({failures} discrepancies)")
        return 1
    print("ROUTE CONTRACT: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
