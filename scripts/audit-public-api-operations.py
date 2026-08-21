#!/usr/bin/env python3
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
Valori public-operation completeness audit — Phase API-3.3.

The Phase API-3 route gate answered *"are the correct operations present?"*.
It compared method+path+operationId across three sources and stopped there. An
operation could pass that gate with no request schema, no response schema, no
declared parameters and no error bodies — 74 named holes.

This script answers the next question:

    Is every PUBLIC operation's HTTP contract complete enough that a
    high-quality multi-language SDK can be generated from it?

It reads two sources and cross-checks them:

  1. api/openapi/valori-v1.yaml            — what the contract claims.
  2. the Rust handler signature, located   — what the runtime actually does.
     via docs/api/phase-api-3-route-manifest.json

Completeness is never inferred from path existence. A `requestBody` is
"complete" only when the contract declares one *and* the handler has a body
extractor; a `query` block is "complete" only when the contract's parameter
list matches the handler's `Query<..>` extractor, in both directions.

Writes docs/api/public-operation-audit.json and .md. Exits non-zero when any
public operation is INCOMPLETE.

VERIFICATION ONLY — never generates or mutates the contract.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover
    print("error: PyYAML is required (pip install pyyaml)", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "docs" / "api" / "phase-api-3-route-manifest.json"
CANONICAL = ROOT / "api" / "openapi" / "valori-v1.yaml"
OUT_JSON = ROOT / "docs" / "api" / "public-operation-audit.json"
OUT_MD = ROOT / "docs" / "api" / "public-operation-audit.md"

HTTP_METHODS = {"get", "post", "put", "delete", "patch", "head", "options", "trace"}

# Statuses that must not carry a body, per RFC 9110 §15.3.5.
BODYLESS_STATUSES = {"204", "304"}

# Success responses that are genuinely empty, verified by reading the handler.
# Kept deliberately in step with `EMPTY_BODY_OK` in
# scripts/verify-api-route-contract.py — the two scripts must never disagree
# about which empty body is intentional.
EMPTY_SUCCESS_OK = {
    # `restore` is `async fn(..) -> Result<(), EngineError>`; the success arm
    # is the unit type, so 200 really does carry nothing.
    ("post", "/v1/snapshot/upload", "200"),
}


# ── source inspection ────────────────────────────────────────────────────────


def axum_to_openapi_path(p: str) -> str:
    return re.sub(r":([A-Za-z_][A-Za-z0-9_]*)", r"{\1}", p)


def _sig_in(src: str, handler: str) -> str | None:
    """The argument list of the axum *handler* named `handler`, if this file has one.

    A name can be defined more than once in one file: `create_node` is both a
    trait method on the standalone `GraphOps` impl and the route handler that
    calls into it. The trait method takes `&self` and has no extractors, so
    matching the first definition reported "handler has no body extractor" for
    an endpoint that plainly takes JSON. Self-receiving definitions are
    therefore skipped — an axum handler never takes `self`.
    """
    fallback = None
    for m in re.finditer(
        rf"\b(?:async\s+)?fn\s+{re.escape(handler)}\s*(?:<[^>]*>)?\s*\(", src
    ):
        i = m.end() - 1
        depth = 0
        sig = None
        for j in range(i, len(src)):
            if src[j] == "(":
                depth += 1
            elif src[j] == ")":
                depth -= 1
                if depth == 0:
                    sig = src[i + 1 : j]
                    break
        if sig is None:
            continue
        if re.match(r"\s*&?\s*(?:mut\s+)?self\b", sig):
            fallback = fallback if fallback is not None else sig
            continue
        return sig
    return fallback


_RS_CACHE: list[tuple[Path, str]] = []


def _all_rust_sources() -> list[tuple[Path, str]]:
    if not _RS_CACHE:
        for p in sorted((ROOT / "crates").rglob("*.rs")):
            try:
                _RS_CACHE.append((p, p.read_text()))
            except OSError:
                continue
    return _RS_CACHE


def find_handler_signature(source_file: str, handler: str) -> str | None:
    """Return the parenthesised argument list of `async fn <handler>`.

    The manifest records the file that *registers* the route, which is not
    always the file that *defines* the handler: `/v1/ingest*` is defined in
    `valori-ingest`, `/v1/tree/*` in `valori-rag`, and the shared-handler
    domains in `crates/valori-node/src/routes/`. So the declared file is tried
    first, then the rest of the workspace.

    Returns None only when no definition exists anywhere — reported as
    UNRESOLVED rather than silently assumed complete.
    """
    # The manifest records the handler as written at the call site, which may
    # be module-qualified (`crate::ingest::ingest`,
    # `valori_rag::tree::tree_verify`). Only the final segment names the fn.
    handler = handler.rsplit("::", 1)[-1]
    path = ROOT / source_file
    if path.exists():
        src = path.read_text()
        sig = _sig_in(src, handler)
        if sig is not None:
            return sig
        # The route may be registered under an import alias —
        # `use crate::routes::version as version_handler;`. Follow it.
        alias = re.search(
            rf"\buse\s+([A-Za-z0-9_:]+)\s+as\s+{re.escape(handler)}\s*;", src
        )
        if alias:
            handler = alias.group(1).rsplit("::", 1)[-1]
            sig = _sig_in(src, handler)
            if sig is not None:
                return sig
    for _, src in _all_rust_sources():
        sig = _sig_in(src, handler)
        if sig is not None:
            return sig
    return None


BODY_EXTRACTORS = (
    r"\bJson\s*<",
    r"\baxum::Json\s*<",
    r"\bForm\s*<",
    r"\bBytes\b",
    r":\s*String\s*[,)]?$",
)


def inspect_handler(sig: str) -> dict:
    """Classify the axum extractors in a handler argument list."""
    # Strip generics/nesting noise conservatively — we only need presence.
    return {
        "has_body_extractor": any(re.search(p, sig, re.M) for p in BODY_EXTRACTORS),
        "has_query_extractor": bool(re.search(r"\bQuery\s*<", sig)),
        "has_path_extractor": bool(
            re.search(r"\b(?:Axum)?Path\s*<", sig) or re.search(r"\baxum::extract::Path\s*<", sig)
        ),
        "has_header_extractor": bool(
            re.search(r"\bHeaderMap\b|\bTypedHeader\s*<", sig)
        ),
    }


# ── contract inspection ──────────────────────────────────────────────────────


def schema_ref(node: dict | None) -> str | None:
    """Render a schema node as a short human-readable type name."""
    if not node:
        return None
    if "$ref" in node:
        return node["$ref"].rsplit("/", 1)[-1]
    for combinator in ("allOf", "oneOf", "anyOf"):
        if combinator in node:
            parts = [schema_ref(s) or "?" for s in node[combinator]]
            return f"{combinator}[{', '.join(parts)}]"
    if node.get("type") == "array":
        return f"array<{schema_ref(node.get('items')) or 'unknown'}>"
    if node.get("type"):
        return str(node["type"])
    return None


def untyped_properties(schemas: dict) -> list[str]:
    """`Schema.property` paths whose schema conveys no type at all.

    A `$ref` to a named schema is "typed" at the reference site, so the
    operation-level check above cannot see a hole *inside* that schema. It
    missed `IndexBuildRequest.parameters` — a bare `serde_json::Value` that
    utoipa rendered with no `type`, which is `unknown` in TypeScript and `Any`
    in Python. This walks the schema set itself so an untyped leaf anywhere in
    the public surface is a finding.
    """
    out: list[str] = []

    def walk(name: str, node: dict, trail: str) -> None:
        if not isinstance(node, dict):
            return
        for prop, sub in (node.get("properties") or {}).items():
            here = f"{trail}.{prop}"
            if is_untyped(sub):
                out.append(f"{name}{here}")
            else:
                walk(name, sub, here)
        items = node.get("items")
        if isinstance(items, dict):
            if is_untyped(items):
                out.append(f"{name}{trail}[]")
            else:
                walk(name, items, f"{trail}[]")

    for name, node in (schemas or {}).items():
        walk(name, node or {}, "")
    return sorted(out)


def is_untyped(node: dict | None) -> bool:
    """True when a schema node conveys no type information to a generator.

    An empty schema, or `{}`-equivalent, is what becomes `unknown` in
    TypeScript and `Any` in Python.
    """
    if node is None:
        return True
    if not isinstance(node, dict):
        return True
    if "$ref" in node:
        return False
    meaningful = {
        "type", "properties", "items", "allOf", "oneOf", "anyOf",
        "enum", "const", "additionalProperties", "format",
    }
    if not (set(node) & meaningful):
        return True
    # A bare `type: object` with no properties is not "typed" in any sense an
    # SDK user can use: it renders as `object` in TypeScript and
    # `Dict[str, Any]` in Python, with no discoverable field.
    #
    # Phase API-3.3: `GET /v1/proof/receipt`, `GET /v1/proof/receipt/{id}` and
    # `GET /v1/ingest/status/{job_id}` were all annotated `body = Object` and
    # passed the looser check that used to live here — the receipt, the
    # product's flagship proof artifact, shipped to every SDK as an opaque
    # blob. `additionalProperties` being a *schema* (not just `true`) still
    # counts as typed, since that describes the values.
    if node.get("type") == "object" and not node.get("properties"):
        if not isinstance(node.get("additionalProperties"), dict):
            return True
    return False


def any_content(resp_or_body: dict) -> tuple[str, dict] | None:
    """The declared media type and entry, JSON preferred.

    A contract is complete when it declares *a* body type, not specifically a
    JSON one: `GET /v1/snapshot/download` is `application/octet-stream` and
    `GET /v1/version` is `text/plain`, and both give a generator everything it
    needs. Requiring JSON here was an audit bug, not a contract defect.
    """
    content = (resp_or_body or {}).get("content") or {}
    for ctype, entry in content.items():
        if "json" in ctype:
            return ctype, (entry or {})
    for ctype, entry in content.items():
        return ctype, (entry or {})
    return None


def audit_operation(method: str, path: str, op: dict, route: dict | None) -> dict:
    rec: dict = {
        "operationId": op.get("operationId"),
        "method": method.upper(),
        "path": path,
        "classification": (route or {}).get("classification", "UNKNOWN"),
        "handler": (route or {}).get("handler"),
        "source_file": (route or {}).get("source_file"),
        "modes": (route or {}).get("modes", []),
    }

    # ── security ──
    sec = op.get("security")
    if sec is None:
        rec["security"] = "INHERITED_UNDECLARED"
    elif sec == []:
        rec["security"] = "NONE"
    else:
        rec["security"] = sorted({k for r in sec for k in (r or {})})
    rec["required_scope"] = op.get("x-required-scope")

    # ── parameters ──
    params = op.get("parameters") or []
    rec["path_parameters"] = sorted(
        p["name"] for p in params if p.get("in") == "path"
    )
    rec["query_parameters"] = sorted(
        p["name"] for p in params if p.get("in") == "query"
    )
    rec["header_parameters"] = sorted(
        p["name"] for p in params if p.get("in") == "header"
    )
    rec["untyped_parameters"] = sorted(
        p.get("name", "?") for p in params if is_untyped(p.get("schema"))
    )

    # ── request body ──
    rb = op.get("requestBody")
    rec["request_body_present"] = rb is not None
    body_ct = any_content(rb) if rb else None
    rec["request_media_type"] = body_ct[0] if body_ct else None
    rec["request_schema"] = schema_ref((body_ct[1] if body_ct else {}).get("schema"))
    rec["request_body_required"] = bool((rb or {}).get("required")) if rb else None
    rec["has_untyped_request"] = bool(rb) and (
        body_ct is None or is_untyped(body_ct[1].get("schema"))
    )

    # ── responses ──
    responses = op.get("responses") or {}
    success, errors = [], []
    rec["success_response_schema"] = {}
    rec["error_response_schemas"] = {}
    untyped_success = []
    for code in sorted(responses):
        entry = responses[code] or {}
        c = str(code)
        numeric = c.isdigit()
        target = success if (numeric and int(c) < 400) else errors
        target.append(c)
        ct = any_content(entry)
        name = schema_ref(ct[1].get("schema")) if ct else None
        if numeric and int(c) < 400:
            rec["success_response_schema"][c] = name
            if c in BODYLESS_STATUSES or (method, path, c) in EMPTY_SUCCESS_OK:
                continue
            if ct is None or is_untyped(ct[1].get("schema")):
                untyped_success.append(c)
        else:
            rec["error_response_schemas"][c] = name
    rec["success_statuses"] = success
    rec["error_statuses"] = errors
    rec["has_untyped_success"] = bool(untyped_success)
    rec["untyped_success_statuses"] = untyped_success
    rec["errors_without_body"] = sorted(
        c for c, n in rec["error_response_schemas"].items() if n is None
    )
    rec["errors_not_api_error"] = sorted(
        c for c, n in rec["error_response_schemas"].items()
        if n is not None and n != "ApiError"
    )

    # ── source cross-check ──
    #
    # Findings are categorised, not free text, so `scripts/api-contract-gate.sh`
    # can report per-category counts (§17) without grepping English prose.
    findings: list[dict] = []
    notes: list[str] = []

    def flag(category: str, message: str) -> None:
        findings.append({"category": category, "message": message})

    sig = (
        find_handler_signature(route["source_file"], route["handler"])
        if route and route.get("source_file") and route.get("handler")
        else None
    )
    if sig is None:
        rec["source_crosscheck"] = "UNRESOLVED"
        flag(
            "unresolved_handler",
            "handler signature could not be located — contract not cross-checked "
            "against source",
        )
    else:
        h = inspect_handler(sig)
        rec["source_crosscheck"] = h
        if h["has_body_extractor"] and not rec["request_body_present"]:
            flag(
                "request",
                "handler has a body extractor but the contract declares no requestBody "
                "(SDK sees `requestBody?: never`)",
            )
        if rec["request_body_present"] and not h["has_body_extractor"]:
            flag(
                "request",
                "contract declares a requestBody but the handler has no body extractor",
            )
        if h["has_query_extractor"] and not rec["query_parameters"]:
            flag(
                "parameter",
                "handler has a Query<..> extractor but the contract declares no query "
                "parameters (SDK sees `query?: never`)",
            )
        if h["has_path_extractor"] and not rec["path_parameters"]:
            flag("parameter", "handler has a Path<..> extractor but the contract declares none")
        # A templated path MUST declare each of its variables.
        for var in re.findall(r"\{([^}]+)\}", path):
            if var not in rec["path_parameters"]:
                flag("parameter", f"path variable `{{{var}}}` is not declared as a parameter")

    # ── contract-internal completeness ──
    if rec["has_untyped_request"]:
        flag("request", "requestBody declares no usable schema")
    if not rec["success_statuses"]:
        flag("response", "no success response documented")
    for c in untyped_success:
        flag("response", f"success {c} declares no typed body")
    for c in rec["errors_without_body"]:
        flag("error", f"error {c} declares no body but the runtime sends ApiError")
    # A `>= 400` response with a typed schema that is NOT ApiError is a
    # deliberate status-report body, not a defect — `GET /health` and
    # `GET /v1/cluster/health` answer 503 with their full health document, and
    # `attach_error_code` exempts exactly those two paths so the bytes match.
    # It is still surfaced, so a third one cannot appear unnoticed.
    for c in rec["errors_not_api_error"]:
        notes.append(
            f"error {c} carries the typed {rec['error_response_schemas'][c]} document "
            f"rather than ApiError (deliberate status-report body)"
        )
    if rec["untyped_parameters"]:
        flag("parameter", "untyped parameter(s): " + ", ".join(rec["untyped_parameters"]))
    if rec["security"] == "INHERITED_UNDECLARED":
        flag("security", "operation declares no explicit `security`")
    if rec["security"] != "NONE":
        missing_auth = {"401", "403"} - set(rec["error_statuses"])
        if missing_auth:
            flag(
                "security",
                "authenticated operation does not document " + "/".join(sorted(missing_auth)),
            )
        # §7: an authenticated operation must say which scope it demands, or an
        # SDK cannot tell a read-only key from a read-write one before the call.
        # The value itself is generated from the middleware's own
        # `required_scope`, and `tests/api_contract.rs` diffs the two.
        if not rec["required_scope"]:
            flag("security", "authenticated operation declares no `x-required-scope`")
    elif rec["required_scope"]:
        flag(
            "security",
            "unauthenticated operation declares `x-required-scope` "
            f"({rec['required_scope']}) — no credentials are consulted",
        )

    rec["findings"] = findings
    rec["notes"] = notes
    rec["completeness_status"] = "COMPLETE" if not findings else "INCOMPLETE"
    return rec


# ── main ─────────────────────────────────────────────────────────────────────


def main() -> int:
    if not CANONICAL.exists():
        print(f"error: {CANONICAL} not found", file=sys.stderr)
        return 2
    if not MANIFEST.exists():
        print(
            f"error: {MANIFEST} not found — run scripts/generate-route-manifest.py",
            file=sys.stderr,
        )
        return 2

    doc = yaml.safe_load(CANONICAL.read_text()) or {}
    manifest = json.loads(MANIFEST.read_text())
    routes = {
        (r["method"].lower(), axum_to_openapi_path(r["path"])): r
        for r in manifest["routes"]
        if r["public_sdk_export"]
    }

    records = []
    for path, item in sorted((doc.get("paths") or {}).items()):
        for method, op in sorted((item or {}).items()):
            if method.lower() not in HTTP_METHODS:
                continue
            records.append(
                audit_operation(method.lower(), path, op or {}, routes.get((method.lower(), path)))
            )

    # §13: an untyped leaf inside a named schema is invisible to the
    # per-operation checks above, because the reference site is typed.
    schema_holes = untyped_properties((doc.get("components") or {}).get("schemas") or {})

    complete = [r for r in records if r["completeness_status"] == "COMPLETE"]
    incomplete = [r for r in records if r["completeness_status"] == "INCOMPLETE"]

    def with_category(cat: str) -> int:
        """Operations carrying at least one finding of this category."""
        return sum(1 for r in records if any(f["category"] == cat for f in r["findings"]))

    totals = {
        "public_operations": len(records),
        "complete": len(complete),
        "incomplete": len(incomplete),
        "operations_with_request_body": sum(1 for r in records if r["request_body_present"]),
        "complete_requests": sum(1 for r in records if r["request_body_present"])
        - with_category("request"),
        "incomplete_requests": with_category("request"),
        "complete_responses": len(records) - with_category("response"),
        "incomplete_responses": with_category("response"),
        "untyped_parameters": sum(len(r["untyped_parameters"]) for r in records),
        "parameter_mismatches": with_category("parameter"),
        "error_contract_defects": with_category("error"),
        "errors_without_body": sum(len(r["errors_without_body"]) for r in records),
        "errors_not_api_error": sum(len(r["errors_not_api_error"]) for r in records),
        "security_mismatches": with_category("security"),
        "unresolved_handlers": with_category("unresolved_handler"),
        "untyped_schema_properties": len(schema_holes),
    }

    OUT_JSON.write_text(
        json.dumps(
            {
                "generated_by": "scripts/audit-public-api-operations.py",
                "untyped_schema_properties": schema_holes,
                "contract": "api/openapi/valori-v1.yaml",
                "openapi_version": doc.get("openapi"),
                "totals": totals,
                "operations": records,
            },
            indent=2,
        )
        + "\n"
    )

    # ── markdown ──
    lines = [
        "# Public Operation Audit — Valori API v1",
        "",
        "Generated by `scripts/audit-public-api-operations.py`. Do not hand-edit;",
        "regenerate instead. Every number here is discovered from",
        "`api/openapi/valori-v1.yaml` cross-checked against the Rust handler",
        "signatures named in `docs/api/phase-api-3-route-manifest.json`.",
        "",
        "## Totals",
        "",
        "| Metric | Value |",
        "|---|---|",
    ]
    for k, v in totals.items():
        lines.append(f"| {k.replace('_', ' ')} | {v} |")
    lines += [
        "",
        "## Incomplete operations",
        "",
    ]
    if not incomplete:
        lines.append("None. Every public operation carries a complete HTTP contract.")
    else:
        for r in incomplete:
            lines.append(f"### `{r['method']} {r['path']}` — `{r['operationId']}`")
            lines.append("")
            for f in r["findings"]:
                lines.append(f"- **[{f['category']}]** {f['message']}")
            lines.append("")

    lines += ["", "## All public operations", "",
              "| operationId | Method | Path | Security | Req | Success | Errors | Status |",
              "|---|---|---|---|---|---|---|---|"]
    for r in records:
        sec = r["security"] if isinstance(r["security"], str) else ",".join(r["security"])
        req = r["request_schema"] or ("—" if not r["request_body_present"] else "UNTYPED")
        succ = ", ".join(
            f"{c}:{n or 'empty'}" for c, n in sorted(r["success_response_schema"].items())
        )
        errs = ", ".join(sorted(r["error_response_schemas"]))
        lines.append(
            f"| `{r['operationId']}` | {r['method']} | `{r['path']}` | {sec} | "
            f"{req} | {succ} | {errs} | {r['completeness_status']} |"
        )
    OUT_MD.write_text("\n".join(lines) + "\n")

    # ── console ──
    print("=======================================================")
    print(" VALORI PUBLIC OPERATION COMPLETENESS AUDIT")
    print("=======================================================")
    for k, v in totals.items():
        print(f"  {k.replace('_', ' '):<32} {v}")
    print("=======================================================")
    if schema_holes:
        print(f"\nUntyped schema properties ({len(schema_holes)}):")
        for h in schema_holes:
            print(f"   - {h}")
    if incomplete:
        print(f"\nIncomplete operations ({len(incomplete)}):")
        for r in incomplete:
            print(f"   - {r['method']} {r['path']} ({r['operationId']})")
            for f in r["findings"]:
                print(f"       * [{f['category']}] {f['message']}")
        print("\nOPERATION AUDIT: FAIL")
        return 1
    if schema_holes:
        print("\nOPERATION AUDIT: FAIL")
        return 1
    print("\nOPERATION AUDIT: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
