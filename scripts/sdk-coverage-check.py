#!/usr/bin/env python3
# Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
"""SDK coverage gate — Phase API-4A §12/§21.

Proves that every public operation in the canonical contract is exposed by every
SDK, and that each claim in a coverage manifest is *true* rather than
aspirational. Three checks per SDK:

  1. **Completeness.** Every operationId in ``api/openapi/valori-v1.yaml``
     appears in the manifest. A contract that grows an operation fails the build
     until the SDK names it.
  2. **No fiction.** Every manifest entry names an operation the contract
     actually has. A removed endpoint fails the build until the SDK drops it.
  3. **Resolvability.** Every declared ``wrapper:`` resolves to a real callable
     in the SDK source. This is the check that stops the manifest from becoming
     documentation nobody maintains.

Entries may declare either ``wrapper:`` (an ergonomic human-written method) or
``generated: true`` (reachable only through the generated client). Both count as
coverage; only ``wrapper:`` is resolved.

Usage::

    python3 scripts/sdk-coverage-check.py             # both SDKs
    python3 scripts/sdk-coverage-check.py --sdk python
    python3 scripts/sdk-coverage-check.py --json      # machine-readable summary

Exits non-zero on any failure.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONTRACT = ROOT / "api" / "openapi" / "valori-v1.yaml"

HTTP_METHODS = {"get", "post", "put", "delete", "patch", "head", "options", "trace"}

SDKS = {
    "python": {
        "manifest": ROOT / "sdk" / "python" / "api-coverage.yaml",
        "sources": [ROOT / "sdk" / "python" / "handwritten"],
        "suffixes": [".py"],
    },
    "typescript": {
        "manifest": ROOT / "sdk" / "typescript" / "api-coverage.yaml",
        "sources": [ROOT / "sdk" / "typescript" / "src"],
        "suffixes": [".ts"],
    },
}


def die(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(2)


def load_yaml(path: pathlib.Path):
    try:
        import yaml
    except ImportError:
        die("PyYAML is required: pip install pyyaml")
    if not path.exists():
        die(f"not found: {path.relative_to(ROOT)}")
    return yaml.safe_load(path.read_text())


def contract_operations() -> dict:
    """``operationId -> "METHOD /path"`` for every operation in the contract."""
    doc = load_yaml(CONTRACT)
    out = {}
    for path, item in (doc.get("paths") or {}).items():
        for method, op in (item or {}).items():
            if method.lower() not in HTTP_METHODS:
                continue
            op_id = (op or {}).get("operationId")
            if not op_id:
                die(f"{method.upper()} {path} has no operationId")
            if op_id in out:
                die(f"duplicate operationId in the contract: {op_id}")
            out[op_id] = f"{method.upper()} {path}"
    return out


# ── wrapper resolution ───────────────────────────────────────────────────────
#
# A wrapper is written as a dotted call path, e.g. `collection.records.insert`
# or `client.collections.create`. Rather than importing the SDK (which would
# make this script depend on an installed package and a working Node runtime),
# resolution is a source-level search for the *final* method name declared
# somewhere in the handwritten tree. That is enough to catch the failure this
# check exists for: a manifest row naming a method nobody wrote.


def declared_methods(sdk: str) -> set:
    """Every method name defined anywhere in an SDK's handwritten sources."""
    config = SDKS[sdk]
    names: set = set()
    patterns = (
        # Python: `def name(` / `async def name(`
        [re.compile(r"^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)\s*\(", re.M)]
        if sdk == "python"
        # TypeScript: `name(args)`, `name = (`, `readonly name:`, `name: Type`
        else [
            re.compile(r"^\s{2,}(?:async\s+)?([A-Za-z_]\w*)\s*[(<]", re.M),
            re.compile(r"^\s{2,}(?:readonly\s+)?([A-Za-z_]\w*)\s*[:=]", re.M),
        ]
    )
    for root in config["sources"]:
        if not root.exists():
            die(f"SDK source tree missing: {root.relative_to(ROOT)}")
        for suffix in config["suffixes"]:
            for source in root.rglob(f"*{suffix}"):
                text = source.read_text(encoding="utf-8")
                for pattern in patterns:
                    names.update(pattern.findall(text))
    return names


def check_sdk(sdk: str, verbose: bool) -> dict:
    config = SDKS[sdk]
    manifest = load_yaml(config["manifest"])
    ops = contract_operations()

    entries = manifest.get("operations") or {}
    if not entries:
        die(f"{sdk}: manifest declares no operations")

    problems: list = []

    # 1. completeness
    missing = sorted(set(ops) - set(entries))
    for op_id in missing:
        problems.append(f"{sdk}: contract operation `{op_id}` ({ops[op_id]}) has no coverage entry")

    # 2. no fiction
    unknown = sorted(set(entries) - set(ops))
    for op_id in unknown:
        problems.append(f"{sdk}: coverage entry `{op_id}` names no operation in the contract")

    # 3. resolvability + declared HTTP agreement
    methods = declared_methods(sdk)
    wrapped = 0
    generated_only = 0
    for op_id, entry in sorted(entries.items()):
        if op_id not in ops:
            continue
        entry = entry or {}
        declared_http = entry.get("http")
        if declared_http and declared_http != ops[op_id]:
            problems.append(
                f"{sdk}: `{op_id}` declares `{declared_http}` but the contract says `{ops[op_id]}`"
            )
        wrapper = entry.get("wrapper")
        if wrapper:
            wrapped += 1
            leaf = str(wrapper).split(".")[-1].replace("()", "")
            if leaf not in methods:
                problems.append(
                    f"{sdk}: `{op_id}` claims wrapper `{wrapper}`, but no method named "
                    f"`{leaf}` is defined in the handwritten sources"
                )
        elif entry.get("generated"):
            generated_only += 1
        else:
            problems.append(
                f"{sdk}: `{op_id}` declares neither `wrapper:` nor `generated: true`"
            )

    # Declared totals must match reality, so the header cannot go stale.
    for field, actual in (("total_operations", len(entries)), ("wrapped", wrapped),
                          ("generated_only", generated_only)):
        declared = manifest.get(field)
        if declared is not None and declared != actual:
            problems.append(
                f"{sdk}: manifest header says {field}: {declared}, but the entries say {actual}"
            )

    result = {
        "sdk": sdk,
        "contract_operations": len(ops),
        "manifest_entries": len(entries),
        "wrapped": wrapped,
        "generated_only": generated_only,
        "problems": problems,
        "ok": not problems,
    }

    if verbose:
        status = "PASS" if result["ok"] else "FAIL"
        print(
            f"  {sdk:<12} {status}  "
            f"{wrapped} wrapped + {generated_only} generated-only "
            f"/ {len(ops)} contract operations"
        )
        for problem in problems:
            print(f"      - {problem}")

    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", choices=sorted(SDKS), action="append",
                        help="check only this SDK (repeatable). Default: all.")
    parser.add_argument("--json", action="store_true", help="emit a JSON summary")
    args = parser.parse_args()

    targets = args.sdk or sorted(SDKS)
    verbose = not args.json

    if verbose:
        print("=====================================")
        print(" VALORI SDK COVERAGE CHECK")
        print("=====================================")
        print(f" contract: {CONTRACT.relative_to(ROOT)}")
        print()

    results = [check_sdk(sdk, verbose) for sdk in targets]
    ok = all(r["ok"] for r in results)

    if args.json:
        print(json.dumps({"ok": ok, "results": results}, indent=2))
    else:
        print()
        print(f" SDK COVERAGE: {'PASS' if ok else 'FAIL'}")
        print("=====================================")

    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
