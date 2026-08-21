#!/usr/bin/env python3
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""
Valori route manifest generator — Phase API-3 Recovery.

Discovers the HTTP surface of `valori-node` by parsing the **actual Rust axum
router construction**. This is the only sanctioned source of route truth.

ARCHITECTURAL CONSTRAINT (Phase API-3 Recovery, sections 1/4/21):

    This script MUST NOT read api/openapi/valori-v1.yaml.
    This script MUST NOT emit OpenAPI paths, request bodies, or responses.

    It is a *discovery* tool. It produces an inventory of what Rust registers.
    The OpenAPI contract is produced exclusively by utoipa from
    `#[utoipa::path(...)]` annotations via the `valori-openapi` binary.

HONESTY CONTRACT (section 5):

    If any router construct cannot be resolved with confidence, this script
    reports it and exits non-zero. "Route discovery incomplete" is the correct
    answer when the parser missed something; a confident wrong count is not.

Outputs:
    docs/api/phase-api-3-route-manifest.json
    docs/api/phase-api-3-route-manifest.md
"""

from __future__ import annotations

import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
NODE_SRC = ROOT / "crates" / "valori-node" / "src"

# Files that construct routers. routes/**/*.rs is scanned too so that a future
# router builder added there is discovered rather than silently missed.
ROUTER_SOURCES = [
    NODE_SRC / "server.rs",
    NODE_SRC / "cluster_server.rs",
    NODE_SRC / "cluster_api.rs",
] + sorted((NODE_SRC / "routes").rglob("*.rs"))

# Top-level router builders. Each is walked transitively through .merge()/.nest().
ENTRY_POINTS = [
    ("standalone", "crates/valori-node/src/server.rs", "build_router_with_keys"),
    (
        "cluster",
        "crates/valori-node/src/cluster_server.rs",
        "build_cluster_router_with_keys",
    ),
]

HTTP_METHODS = ("get", "post", "put", "delete", "patch", "head", "options", "trace")

# ── Classification policy (section 6) ────────────────────────────────────────
#
# Not every axum-registered route belongs in the public SDK contract. These
# rules are declared here, in one reviewable place, rather than being inferred.
#
#   PUBLIC_UNAUTH     — reachable without credentials; part of the SDK surface.
#   PUBLIC_SDK        — authenticated data-plane API; the SDK surface proper.
#   ADMIN             — admin-scope operations (key management, membership).
#   OPERATOR_INTERNAL — operator/observability/node-to-node; NOT in the SDK.
#   DEPRECATED        — legacy alias retained for compatibility; NOT in the SDK.

PUBLIC_UNAUTH_PATHS = {"/health"}

OPERATOR_INTERNAL_PATHS = {
    "/metrics",
    "/v1/replication/wal",
    "/v1/replication/events",
    "/v1/replication/state",
    "/v1/cluster/read-index",
}

OPERATOR_INTERNAL_PREFIXES = ()

ADMIN_PATHS = {
    "/v1/keys",
    "/v1/keys/:id",
    "/v1/cluster/add-node",
    "/v1/cluster/remove-node",
    "/v1/cluster/snapshot",
    "/v1/crypto/shred/:key_id",
}

# Routers whose every route is a deprecated legacy alias. Identified by the
# Rust binding name inside the router source, so renaming the block in Rust
# surfaces here as an unresolved-classification error rather than silently
# reclassifying 13 routes.
DEPRECATED_ROUTER_BINDINGS = {"legacy"}


@dataclass
class Route:
    method: str
    path: str
    handler: str
    source_file: str
    source_line: int
    router_binding: str
    mode: str = ""
    classification: str = ""

    def key(self) -> tuple[str, str]:
        return (self.method, self.path)


@dataclass
class RouterUnit:
    """One `Router::new()...` chain, keyed by its binding or function name.

    Units are scoped **per source file**. `server.rs` and `cluster_server.rs`
    both declare bindings named `v1`, `legacy`, `public`, and `protected`;
    resolving those in a flat namespace silently unions two different routers.
    """

    name: str
    source_file: str
    source_line: int
    is_fn: bool = False
    routes: list[Route] = field(default_factory=list)
    merges: list[tuple[str, int]] = field(default_factory=list)

    @property
    def qualified(self) -> str:
        return f"{self.source_file}::{self.name}"


@dataclass
class Problem:
    file: str
    line: int
    reason: str
    excerpt: str


PROBLEMS: list[Problem] = []


def problem(file: str, line: int, reason: str, excerpt: str = "") -> None:
    PROBLEMS.append(Problem(file, line, reason, excerpt.strip()[:160]))


# ── Rust chain slicing ───────────────────────────────────────────────────────


def strip_comments(src: str) -> str:
    """Blank out // comments and string-literal-free /* */ blocks.

    Replaces with spaces so byte offsets and line numbers are preserved.
    """
    out = list(src)
    i, n = 0, len(src)
    in_str = False
    while i < n:
        c = src[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
            i += 1
            continue
        if c == '"':
            in_str = True
            i += 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j == -1 else j
            for k in range(i, j):
                out[k] = " "
            i = j
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            j = src.find("*/", i)
            j = n if j == -1 else j + 2
            for k in range(i, j):
                if out[k] != "\n":
                    out[k] = " "
            i = j
            continue
        i += 1
    return "".join(out)


def chain_end(src: str, start: int) -> int:
    """Return the offset of the `;` terminating the statement beginning at `start`."""
    depth = 0
    i, n = start, len(src)
    in_str = False
    while i < n:
        c = src[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
            i += 1
            continue
        if c == '"':
            in_str = True
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                # Unbalanced close before a `;` — the chain is a tail expression
                # (function return value). That is the unit's end.
                return i
            depth -= 1
        elif c == ";" and depth == 0:
            return i
        i += 1
    return n


def parse_method_chain(chunk: str) -> list[tuple[str, str]]:
    """Extract (method, handler) pairs from a `.route()` second argument.

    Handles `get(h)`, `axum::routing::post(h)`, and chains such as
    `get(a).delete(b)`.
    """
    pairs = []
    for m in re.finditer(
        r"(?:axum::routing::|routing::)?\b(" + "|".join(HTTP_METHODS) + r")\s*\(",
        chunk,
    ):
        method = m.group(1)
        # Slice out the balanced handler argument.
        i = m.end()
        depth = 1
        start = i
        while i < len(chunk) and depth:
            if chunk[i] == "(":
                depth += 1
            elif chunk[i] == ")":
                depth -= 1
            i += 1
        handler = chunk[start : i - 1].strip()
        # Reduce `crate::ingest::ingest` / `valori_rag::tree::tree_verify` to a
        # readable handler identity while keeping the qualifying path.
        handler = re.sub(r"\s+", "", handler)
        pairs.append((method.upper(), handler))
    return pairs


def split_route_args(chunk: str) -> tuple[str, str] | None:
    """Split a `.route(A, B)` argument list at the top-level comma."""
    depth = 0
    in_str = False
    i = 0
    while i < len(chunk):
        c = chunk[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
            i += 1
            continue
        if c == '"':
            in_str = True
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == "," and depth == 0:
            return chunk[:i], chunk[i + 1 :]
        i += 1
    return None


def parse_unit(
    src: str, name: str, file_label: str, start: int, end: int, is_fn: bool = False
) -> RouterUnit:
    line0 = src.count("\n", 0, start) + 1
    unit = RouterUnit(name=name, source_file=file_label, source_line=line0, is_fn=is_fn)
    body = src[start:end]

    for m in re.finditer(r"\.route\s*\(", body):
        i = m.end()
        depth = 1
        s = i
        while i < len(body) and depth:
            if body[i] == "(":
                depth += 1
            elif body[i] == ")":
                depth -= 1
            i += 1
        args = body[s : i - 1]
        line = src.count("\n", 0, start + m.start()) + 1

        split = split_route_args(args)
        if split is None:
            problem(file_label, line, ".route() call has no top-level comma", args)
            continue
        path_arg, method_arg = split
        path_m = re.fullmatch(r'\s*"([^"]*)"\s*', path_arg)
        if not path_m:
            problem(
                file_label,
                line,
                ".route() path is not a plain string literal — cannot resolve statically",
                path_arg,
            )
            continue
        path = path_m.group(1)

        pairs = parse_method_chain(method_arg)
        if not pairs:
            problem(
                file_label,
                line,
                f".route(\"{path}\", ...) has no recognisable HTTP method handler",
                method_arg,
            )
            continue
        for method, handler in pairs:
            unit.routes.append(
                Route(
                    method=method,
                    path=path,
                    handler=handler,
                    source_file=file_label,
                    source_line=line,
                    router_binding=name,
                )
            )

    for m in re.finditer(r"\.(merge|nest)\s*\(", body):
        kind = m.group(1)
        i = m.end()
        depth = 1
        s = i
        while i < len(body) and depth:
            if body[i] == "(":
                depth += 1
            elif body[i] == ")":
                depth -= 1
            i += 1
        arg = body[s : i - 1].strip()
        line = src.count("\n", 0, start + m.start()) + 1

        if kind == "nest":
            problem(
                file_label,
                line,
                ".nest() is not supported by this parser; prefixed routes would be missed",
                arg,
            )
            continue

        ident = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*)", arg)
        call = re.match(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(", arg)
        if ident:
            unit.merges.append((ident.group(1), line))
        elif call:
            unit.merges.append((call.group(1), line))
        else:
            problem(
                file_label,
                line,
                ".merge() argument is neither a plain binding nor a plain function call",
                arg,
            )

    return unit


def collect_units(path: Path) -> dict[str, RouterUnit]:
    raw = path.read_text()
    src = strip_comments(raw)
    label = str(path.relative_to(ROOT))
    units: dict[str, RouterUnit] = {}

    # 1. `let NAME = Router::new()...;` — also catches rebinding
    #    (`let protected = protected.layer(..)`), which is why later bindings
    #    are merged into the earlier unit rather than replacing it.
    let_spans: list[tuple[int, int]] = []
    for m in re.finditer(r"\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*", src):
        name = m.group(1)
        rest = src[m.end() :]
        if not (rest.lstrip().startswith("Router::new") or rest.lstrip().startswith(name)):
            continue
        start = m.end()
        end = chain_end(src, start)
        unit = parse_unit(src, name, label, start, end)
        if not unit.routes and not unit.merges:
            continue
        let_spans.append((start, end))
        if name in units:
            units[name].routes.extend(unit.routes)
            units[name].merges.extend(unit.merges)
        else:
            units[name] = unit

    # A function-shaped builder's body *contains* the `let` chains above. Parsing
    # the raw body would attribute the same routes to both the function and the
    # binding, double-counting them and giving the function's copy the wrong
    # classification. Blank the already-claimed spans (preserving newlines so
    # line numbers stay accurate) before parsing function bodies.
    masked = list(src)
    for s, e in let_spans:
        for k in range(s, e):
            if masked[k] != "\n":
                masked[k] = " "
    masked_src = "".join(masked)

    # 2. `fn NAME(..) -> Router { ... }` — function-shaped builders.
    for m in re.finditer(
        r"\b(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\([^;{]*?->\s*Router\s*\{", src, re.S
    ):
        name = m.group(1)
        start = m.end()
        # Function body end: matching close brace.
        depth = 1
        i = start
        while i < len(src) and depth:
            if src[i] == "{":
                depth += 1
            elif src[i] == "}":
                depth -= 1
            i += 1
        body_end = i - 1
        unit = parse_unit(masked_src, name, label, start, body_end, is_fn=True)

        # The function's value is its tail expression. When the body assembled a
        # router into a local binding and returned it, follow that binding —
        # otherwise the entry point resolves to zero routes.
        # Only chase the tail when the function body itself contributed nothing:
        # a body that already registered routes *is* its own tail expression, and
        # following it again would resolve to a method name like `with_state`.
        tail = "" if (unit.routes or unit.merges) else masked_src[start:body_end].rstrip()
        tail_target = None
        tail_m = re.search(r"(?:^|[;}])\s*([A-Za-z_][A-Za-z0-9_]*)\s*$", tail)
        if tail_m and tail_m.group(1) != "return":
            # Tail is a bare binding: `router`
            tail_target = tail_m.group(1)
        elif tail.endswith(")"):
            # Tail is a delegating call: `build_router_with_keys(state, ...)`
            depth = 0
            j = len(tail) - 1
            while j >= 0:
                if tail[j] == ")":
                    depth += 1
                elif tail[j] == "(":
                    depth -= 1
                    if depth == 0:
                        break
                j -= 1
            head = re.search(r"([A-Za-z_][A-Za-z0-9_]*)\s*$", tail[:j]) if j >= 0 else None
            if head:
                tail_target = head.group(1)

        if tail_target:
            unit.merges.append((tail_target, src.count("\n", 0, body_end) + 1))
        elif not unit.routes and not unit.merges:
            problem(
                label,
                src.count("\n", 0, m.start()) + 1,
                f"fn '{name}' returns Router but neither registers routes nor "
                "returns a resolvable binding",
                tail[-120:],
            )

        if name in units:
            units[name].routes.extend(unit.routes)
            units[name].merges.extend(unit.merges)
            units[name].is_fn = True
        else:
            units[name] = unit

    return units


def resolve(
    entry_file: str, entry: str, units: dict[str, dict[str, RouterUnit]], mode: str
) -> list[Route]:
    """Walk an entry point transitively through `.merge()`.

    Name lookup is file-scoped first (local `let` bindings), then falls back to
    any file-level function-shaped router builder — which is how
    `cluster_server.rs` reaches `cluster_api.rs::cluster_router`.
    """
    seen: set[str] = set()
    out: list[Route] = []

    def lookup(name: str, in_file: str) -> RouterUnit | None:
        local = units.get(in_file, {}).get(name)
        if local is not None:
            return local
        for f, by_name in units.items():
            u = by_name.get(name)
            if u is not None and u.is_fn:
                return u
        return None

    def walk(name: str, in_file: str, from_line: int) -> None:
        unit = lookup(name, in_file)
        if unit is None:
            problem(
                in_file,
                from_line,
                f"unresolved router merge target '{name}' — route inventory is incomplete",
            )
            return
        if unit.qualified in seen:
            return
        seen.add(unit.qualified)
        for r in unit.routes:
            r.mode = mode
            out.append(r)
        for target, line in unit.merges:
            walk(target, unit.source_file, line)

    walk(entry, entry_file, 0)
    return out


def classify(route: Route) -> str:
    if route.router_binding in DEPRECATED_ROUTER_BINDINGS:
        return "DEPRECATED"
    if route.path in ADMIN_PATHS:
        return "ADMIN"
    if route.path in OPERATOR_INTERNAL_PATHS or route.path.startswith(
        OPERATOR_INTERNAL_PREFIXES
    ):
        return "OPERATOR_INTERNAL"
    if route.path in PUBLIC_UNAUTH_PATHS:
        return "PUBLIC_UNAUTH"
    return "PUBLIC_SDK"


PUBLIC_CLASSES = {"PUBLIC_UNAUTH", "PUBLIC_SDK"}


def operation_id_for(route: Route, annotations: dict[str, dict]) -> str:
    """Report the canonical operationId, read out of Rust source.

    The manifest invents nothing. The canonical id is declared exactly once,
    in the handler's own `#[utoipa::path(operation_id = "...")]`, and that is
    what this function reports when the annotation exists. An unannotated
    route has no declared id yet, so the handler name stands in — which is
    also what utoipa would default to once someone annotates it.

    Reading the id from the annotation is what lets one identifier serve both
    the standalone and cluster registration of the same path: the two routers
    have differently named handler functions, but only one of them carries the
    `#[utoipa::path]`, and that one names the operation.
    """
    ann = annotations.get(f"{route.method} {axum_to_openapi_path(route.path)}")
    if ann and ann.get("operation_id"):
        return ann["operation_id"]
    return route.handler.split("::")[-1]


def axum_to_openapi_path(p: str) -> str:
    """`/v1/records/:id` -> `/v1/records/{id}`. Representation only."""
    return re.sub(r":([A-Za-z_][A-Za-z0-9_]*)", r"{\1}", p)


def registered_in_valori_api() -> set[str]:
    """The handler function names listed in `ValoriApi`'s `paths(...)` block.

    Section 12: utoipa coverage is a two-link chain — the handler carries an
    annotation AND the annotated handler is registered on the `OpenApi`
    derive. Coverage is never inferred from a schema, a route count, or the
    presence of a path in the OpenAPI file.
    """
    src = strip_comments((NODE_SRC / "openapi.rs").read_text())
    m = re.search(r"\n\s*paths\(", src)
    if not m:
        problem("crates/valori-node/src/openapi.rs", 0,
                "ValoriApi has no paths(...) block — no operation can be generated")
        return set()
    i = m.end()
    depth = 1
    start = i
    while i < len(src) and depth:
        if src[i] == "(":
            depth += 1
        elif src[i] == ")":
            depth -= 1
        i += 1
    block = src[start : i - 1]
    return {
        seg.strip().split("::")[-1]
        for seg in block.split(",")
        if seg.strip()
    }


def find_utoipa_annotations() -> dict[str, dict]:
    """Scan for `#[utoipa::path(...)]` and record (method, path, operation_id).

    Used only to populate the `utoipa_registered` link field (section 20).
    This reads Rust source, never OpenAPI.
    """
    found: dict[str, dict] = {}
    scan_roots = [NODE_SRC]
    for extra in ("valori-rag", "valori-ingest"):
        d = ROOT / "crates" / extra / "src"
        if d.exists():
            scan_roots.append(d)
    for rs in sorted(f for root in scan_roots for f in root.rglob("*.rs")):
        src = strip_comments(rs.read_text())
        for m in re.finditer(
            r"#\[(?:cfg_attr\s*\([^)]*?,\s*)?utoipa::path\s*\(", src
        ):
            i = m.end()
            depth = 1
            s = i
            while i < len(src) and depth:
                if src[i] == "(":
                    depth += 1
                elif src[i] == ")":
                    depth -= 1
                i += 1
            attr = src[s : i - 1]
            meth = re.search(
                r"^\s*(" + "|".join(HTTP_METHODS) + r")\s*,", attr, re.M | re.I
            ) or re.search(r"\b(" + "|".join(HTTP_METHODS) + r")\s*,", attr, re.I)
            path_m = re.search(r'path\s*=\s*"([^"]*)"', attr)
            op_m = re.search(r'operation_id\s*=\s*"([^"]*)"', attr)
            # The annotated fn follows the attribute block.
            # The annotated fn is the first `fn` after the attribute. Doc
            # comments between the two are already blanked out by
            # strip_comments, so nothing inside prose can be mistaken for it.
            # The window is generous because a handler's doc comment can be
            # long; the search still stops at the first real `fn`.
            fn_m = re.search(
                r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)", src[i : i + 4000]
            )
            if not (meth and path_m):
                problem(
                    str(rs.relative_to(ROOT)),
                    src.count("\n", 0, m.start()) + 1,
                    "#[utoipa::path] is missing a method or path",
                    attr,
                )
                continue
            fn_name = fn_m.group(1) if fn_m else ""
            key = f"{meth.group(1).upper()} {path_m.group(1)}"
            found[key] = {
                "operation_id": op_m.group(1) if op_m else fn_name,
                "fn": fn_name,
                "source_file": str(rs.relative_to(ROOT)),
            }
    return found


def main() -> int:
    missing = [p for p in ROUTER_SOURCES if not p.exists()]
    if missing:
        for p in missing:
            problem(str(p), 0, "router source file not found")

    units: dict[str, dict[str, RouterUnit]] = {}
    for p in ROUTER_SOURCES:
        if not p.exists():
            continue
        units[str(p.relative_to(ROOT))] = collect_units(p)

    all_routes: dict[tuple[str, str], Route] = {}
    per_mode: dict[str, set[tuple[str, str]]] = {}

    for mode, filename, entry in ENTRY_POINTS:
        routes = resolve(filename, entry, units, mode)
        if not routes:
            problem(filename, 0, f"entry point '{entry}' resolved to zero routes")
        per_mode[mode] = {r.key() for r in routes}
        for r in routes:
            r.classification = classify(r)
            existing = all_routes.get(r.key())
            if existing is None:
                all_routes[r.key()] = r
            elif existing.classification != r.classification:
                problem(
                    r.source_file,
                    r.source_line,
                    f"{r.method} {r.path} classified differently in standalone vs cluster",
                )

    utoipa = find_utoipa_annotations()
    registered_fns = registered_in_valori_api()

    if PROBLEMS:
        print("ROUTE DISCOVERY INCOMPLETE", file=sys.stderr)
        print("=" * 60, file=sys.stderr)
        for p in PROBLEMS:
            print(f"  {p.file}:{p.line}: {p.reason}", file=sys.stderr)
            if p.excerpt:
                print(f"      {p.excerpt}", file=sys.stderr)
        print("=" * 60, file=sys.stderr)
        print(
            f"{len(PROBLEMS)} unresolved construct(s). Refusing to emit a manifest "
            "that would understate the route surface.",
            file=sys.stderr,
        )
        return 1

    ordered = sorted(all_routes.values(), key=lambda r: (r.path, r.method))

    entries = []
    for r in ordered:
        op_id = operation_id_for(r, utoipa)
        key = f"{r.method} {axum_to_openapi_path(r.path)}"
        ann = utoipa.get(key)
        # Section 6/12: an annotation alone does not make an operation appear.
        # The handler must ALSO be listed in `ValoriApi`'s `paths(...)`, or
        # utoipa emits nothing for it. Both links are checked here.
        linked = ann is not None and ann["fn"] in registered_fns
        entries.append(
            {
                "method": r.method,
                "path": r.path,
                "handler": r.handler,
                "operation_id": op_id,
                "source_file": r.source_file,
                "source_line": r.source_line,
                "router_binding": r.router_binding,
                "classification": r.classification,
                "public_sdk_export": r.classification in PUBLIC_CLASSES,
                "modes": sorted(m for m, keys in per_mode.items() if r.key() in keys),
                "utoipa_registered": linked,
                "utoipa_annotated": ann is not None,
                "utoipa_registered_in_api": bool(ann and ann["fn"] in registered_fns),
                "utoipa_operation_id": ann["operation_id"] if ann else None,
                "utoipa_source_file": ann["source_file"] if ann else None,
            }
        )

    counts: dict[str, int] = {}
    for e in entries:
        counts[e["classification"]] = counts.get(e["classification"], 0) + 1

    public = [e for e in entries if e["public_sdk_export"]]
    manifest = {
        "generated_by": "scripts/generate-route-manifest.py",
        "source_of_truth": "Rust axum router registrations",
        "reads_openapi": False,
        "entry_points": [
            {"mode": m, "file": f, "function": e} for m, f, e in ENTRY_POINTS
        ],
        "totals": {
            "routes": len(entries),
            "public_sdk_routes": len(public),
            "utoipa_annotated_public_routes": sum(
                1 for e in public if e["utoipa_registered"]
            ),
            "by_classification": dict(sorted(counts.items())),
            "standalone_routes": len(per_mode.get("standalone", ())),
            "cluster_routes": len(per_mode.get("cluster", ())),
        },
        "routes": entries,
    }

    out_json = ROOT / "docs" / "api" / "phase-api-3-route-manifest.json"
    out_json.write_text(json.dumps(manifest, indent=2) + "\n")

    lines = [
        "# Valori Route Manifest — discovered from Rust router source",
        "",
        "Generated by `scripts/generate-route-manifest.py`. **Do not hand-edit.**",
        "",
        "This manifest is derived exclusively from axum router registrations in",
        "`crates/valori-node/src/{server,cluster_server,cluster_api}.rs`. It never",
        "reads `api/openapi/valori-v1.yaml`, and it never emits OpenAPI.",
        "",
        "## Totals",
        "",
        f"- Routes discovered: **{len(entries)}**",
        f"- Public SDK routes: **{len(public)}**",
        f"- Public routes with `#[utoipa::path]`: "
        f"**{manifest['totals']['utoipa_annotated_public_routes']}**",
        f"- Standalone-registered: {manifest['totals']['standalone_routes']}",
        f"- Cluster-registered: {manifest['totals']['cluster_routes']}",
        "",
        "## By classification",
        "",
        "| Classification | Count | In SDK |",
        "|---|---|---|",
    ]
    for cls, n in sorted(counts.items()):
        lines.append(f"| `{cls}` | {n} | {'yes' if cls in PUBLIC_CLASSES else 'no'} |")
    lines += [
        "",
        "## Routes",
        "",
        "| Method | Path | Handler | Classification | Modes | Utoipa |",
        "|---|---|---|---|---|---|",
    ]
    for e in entries:
        lines.append(
            f"| `{e['method']}` | `{e['path']}` | `{e['handler']}` | "
            f"{e['classification']} | {', '.join(e['modes'])} | "
            f"{'yes' if e['utoipa_registered'] else '—'} |"
        )
    lines.append("")
    (ROOT / "docs" / "api" / "phase-api-3-route-manifest.md").write_text("\n".join(lines))

    print(f"Route discovery complete: {len(entries)} routes from Rust router source.")
    print(f"  public SDK:            {len(public)}")
    print(f"  utoipa-annotated:      {manifest['totals']['utoipa_annotated_public_routes']}")
    for cls, n in sorted(counts.items()):
        print(f"  {cls:<20} {n}")
    print(f"Wrote {out_json.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
