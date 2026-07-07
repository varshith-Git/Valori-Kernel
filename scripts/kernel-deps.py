#!/usr/bin/env python3
"""
kernel-deps.py — dependency-order guard for the Valori workspace.

Why this exists
---------------
When you (or an AI assistant) edit `valori-kernel`, the Rust compiler forces a
rebuild of every crate that *compile-depends* on it — you cannot miss those.
But several important consumers are NOT compile-coupled and the compiler will
NEVER warn you if you forget them:

  * valori-node's TWO routers (server.rs must stay in sync with cluster_server.rs)
  * python/valoricore/remote.py  (HTTP client — pure wire contract)
  * valori-mcp                   (talks to the node over HTTP; node is a dev-dep only)

Nothing about the project is hardcoded: the dependency graph comes from
`cargo metadata`, the per-crate file checklist and post-phase steps are parsed
live from `CLAUDE.md`, and the wire-coupled consumers are derived by scanning
the filesystem (dual routers, pyo3/maturin crates, Python SDK imports) and the
cargo dev-dependency edges. Update the docs or the code and the tool follows.

It then provides:

  * `order`      — prints the full topological build order.
  * `impact`     — given changed crates (auto-detected from git, or passed as
                   args), prints every downstream crate IN ORDER plus the exact
                   files to touch, and loudly flags the wire-coupled consumers
                   the compiler can't protect.
  * `api-diff`   — diffs the public API of a crate against a git ref (new/removed
                   `pub fn`, `pub enum` variants, `pub struct`) and checks whether
                   each new symbol is referenced downstream yet.
  * `callers`    — reverse call-map for ONE function: every Rust + Python SDK
                   site that references it, grouped by crate, def vs call.
  * `fn-impact`  — derive which functions CHANGED vs a git ref (from diff hunk
                   headers) and list every caller of each, flagging Python SDK
                   callers the compiler cannot protect.
  * `rebuild`    — `cargo clean -p` the impacted crates, rebuild the workspace,
                   and run the wasm/no_std guard on the kernel.

Usage
-----
  scripts/kernel-deps.py order
  scripts/kernel-deps.py impact                 # auto-detect from git
  scripts/kernel-deps.py impact valori-kernel   # explicit
  scripts/kernel-deps.py api-diff valori-kernel [git-ref]
  scripts/kernel-deps.py callers apply_event_ns
  scripts/kernel-deps.py fn-impact [crate] [git-ref]   # auto-detect changed fns
  scripts/kernel-deps.py explain <file-or-dir> [max_fns] [--model=llama3.2:3b]
  scripts/kernel-deps.py rebuild                # clean+build impacted crates

`explain` uses a local Ollama model (default llama3.2:3b) to summarize each
function + note its dependency ripple. One function per request (token-safe);
override with OLLAMA_MODEL / OLLAMA_URL env vars. Needs `ollama serve` running.

No Python dependencies beyond the stdlib; needs cargo, git (and ollama for `explain`).
"""

import json
import os
import re
import subprocess
import sys
import urllib.request

def sh(cmd):
    return subprocess.run(cmd, capture_output=True, text=True)


def repo_root():
    r = sh(["git", "rev-parse", "--show-toplevel"])
    if r.returncode != 0:
        sys.exit("error: not inside a git repo")
    return r.stdout.strip()


def load_graph():
    """Return (members, deps, dirs).
    members: list of workspace package names.
    deps:    {pkg: [(dep_pkg, kind)]}  kind in {'normal','dev','build'}.
    dirs:    {pkg: manifest_dir_abspath}.
    """
    r = sh(["cargo", "metadata", "--format-version", "1"])
    if r.returncode != 0:
        sys.exit("error: cargo metadata failed\n" + r.stderr)
    d = json.loads(r.stdout)
    ws = set(d["workspace_members"])
    byid = {p["id"]: p for p in d["packages"]}
    members, dirs = [], {}
    for pid in ws:
        p = byid[pid]
        members.append(p["name"])
        dirs[p["name"]] = os.path.dirname(p["manifest_path"])
    deps = {m: [] for m in members}
    names_in_ws = set(members)
    for node in d["resolve"]["nodes"]:
        if node["id"] not in ws:
            continue
        name = byid[node["id"]]["name"]
        for dep in node["deps"]:
            dn = byid.get(dep["pkg"], {}).get("name")
            if dn in names_in_ws:
                kinds = {k.get("kind") or "normal" for k in dep.get("dep_kinds", [])}
                kind = "normal" if "normal" in kinds else sorted(kinds)[0]
                deps[name].append((dn, kind))
    return members, deps, dirs


def topo_order(members, deps):
    """Kahn topological sort: dependencies before dependents. Ignores dev edges
    for ordering (dev deps are test-only and can create cycles)."""
    edges = {m: {dp for dp, k in deps[m] if k == "normal"} for m in members}
    order, ready = [], sorted([m for m in members if not edges[m]])
    remaining = {m: set(edges[m]) for m in members}
    while ready:
        n = ready.pop(0)
        order.append(n)
        newly = []
        for m in members:
            if n in remaining[m]:
                remaining[m].discard(n)
                if not remaining[m] and m not in order and m not in ready:
                    newly.append(m)
        ready = sorted(set(ready) | set(newly))
    # append anything left (shouldn't happen in an acyclic normal graph)
    for m in members:
        if m not in order:
            order.append(m)
    return order


def reverse_dependents(target, members, deps, include_dev=True):
    """All crates that transitively depend on `target` (normal + optionally dev)."""
    rdeps = {m: set() for m in members}
    for m in members:
        for dp, k in deps[m]:
            if include_dev or k == "normal":
                rdeps[dp].add((m, k))
    seen, stack = {}, [target]
    while stack:
        cur = stack.pop()
        for m, k in rdeps.get(cur, ()):
            if m not in seen:
                seen[m] = k
                stack.append(m)
            elif k == "normal":
                seen[m] = "normal"  # promote to compile-coupled
    return seen  # {crate: kind}


def changed_crates(dirs):
    """Map git working-tree changes to owning workspace crate."""
    root = repo_root()
    r = sh(["git", "-C", root, "status", "--porcelain", "--untracked-files=all"])
    files = []
    for line in r.stdout.splitlines():
        path = line[3:].strip()
        if "->" in path:  # rename
            path = path.split("->")[-1].strip()
        files.append(os.path.join(root, path))
    hit = set()
    also_py = False
    for f in files:
        for pkg, d in dirs.items():
            if f.startswith(d + os.sep):
                hit.add(pkg)
        if "/python/" in f or f.endswith(".py"):
            also_py = True
    return hit, also_py


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------
def cmd_order(members, deps, dirs):
    order = topo_order(members, deps)
    print("Topological build order (dependencies first):\n")
    for i, m in enumerate(order, 1):
        d = deps[m]
        norm = [dp for dp, k in d if k == "normal"]
        dev = [dp for dp, k in d if k == "dev"]
        tail = ""
        if norm:
            tail += "  needs: " + ", ".join(sorted(norm))
        if dev:
            tail += "   [dev: " + ", ".join(sorted(dev)) + "]"
        print(f"  {i}. {m}{tail}")
    print("\nvalori-kernel has zero internal deps -> it is the root/center.")


# ---------------------------------------------------------------------------
# Live scans — everything below is derived from CLAUDE.md + the filesystem +
# cargo metadata at runtime. Nothing about the project's files/consumers is
# hardcoded, so the guidance can't silently go stale.
# ---------------------------------------------------------------------------
def parse_claude_md():
    """Parse CLAUDE.md for (key_files, post_phase).
    key_files: {crate: [(file, desc)]} from the '## Key files' tables.
    post_phase: [step, ...] from the '## MANDATORY: after every phase' list."""
    key_files, post_phase = {}, []
    try:
        lines = open(os.path.join(repo_root(), "CLAUDE.md"), errors="replace").read().splitlines()
    except OSError:
        return key_files, post_phase
    in_key, in_phase, crate = False, False, None
    for line in lines:
        if line.startswith("## "):
            low = line.lower()
            in_key = low.startswith("## key files")
            in_phase = "after every phase" in low
            crate = None
            continue
        if in_key:
            m = re.match(r"^###\s+([A-Za-z0-9_-]+)", line)
            if m:
                crate = m.group(1)
                key_files.setdefault(crate, [])
            elif crate and line.startswith("|"):
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                if len(cells) >= 2 and cells[0] and cells[0] != "File" \
                        and not set(cells[0]) <= set("-: "):
                    key_files[crate].append((cells[0].strip("` "), cells[1]))
        elif in_phase:
            m = re.match(r"^\d+\.\s+\*\*(.+?)\*\*\s*(.*)", line)
            if m:
                post_phase.append(re.sub(r"\s+", " ", (m.group(1) + " " + m.group(2))).strip())
    return key_files, post_phase


def scan_crate_files(crate, dirs):
    """Fallback for crates with no CLAUDE.md table: list real .rs modules."""
    src = os.path.join(dirs.get(crate, ""), "src")
    out = []
    for root, _, files in os.walk(src):
        for f in sorted(files):
            if f.endswith(".rs"):
                out.append((os.path.relpath(os.path.join(root, f), dirs[crate]), ""))
    return out


def crate_has(crate, dirs, relpath):
    return os.path.exists(os.path.join(dirs.get(crate, ""), relpath))


def crate_is_no_std(crate, dirs):
    lib = os.path.join(dirs.get(crate, ""), "src", "lib.rs")
    try:
        return "no_std" in open(lib, errors="replace").read()
    except OSError:
        return False


def crate_uses_pyo3(crate, dirs):
    cargo = os.path.join(dirs.get(crate, ""), "Cargo.toml")
    try:
        return bool(re.search(r"(?m)^\s*pyo3\b|\bpyo3\s*=", open(cargo, errors="replace").read()))
    except OSError:
        return False


def python_sdk_files():
    """Classify python SDK modules by REAL imports: HTTP-client vs FFI-embedded."""
    base = os.path.join(repo_root(), "python", "valoricore")
    http, ffi = [], []
    if not os.path.isdir(base):
        return http, ffi
    for f in sorted(os.listdir(base)):
        if not f.endswith(".py"):
            continue
        try:
            txt = open(os.path.join(base, f), errors="replace").read()
        except OSError:
            continue
        rel = f"python/valoricore/{f}"
        if re.search(r"\b(import|from)\s+(requests|httpx|aiohttp|urllib)", txt):
            http.append(rel)
        if re.search(r"valoricore_ffi|from\s+\.local|import\s+local", txt):
            ffi.append(rel)
    return http, ffi


def derive_invisible(affected, members, deps, dirs):
    """Compiler-invisible consumers, derived from real signals:
      * cargo dev-only reverse-deps (e.g. mcp -> node)
      * dual-router crates (server.rs + cluster_server.rs both present)
      * pyo3/maturin crates (need .so rebuild)
      * python SDK files classified by import (HTTP wire vs FFI embedded)."""
    out = {}
    def add(text):
        out.setdefault(text.split("(")[0].strip(), text)

    aset = set(affected)
    # dev-only reverse dependents: depend on a changed crate but NOT compile-coupled
    normal = set()
    for c in aset:
        normal |= set(reverse_dependents(c, members, deps, include_dev=False))
    for m in members:
        if m in aset or m in normal:
            continue
        for dp, k in deps[m]:
            if dp in aset and k == "dev":
                add(f"{m}  (depends on {dp} via DEV-dep only — HTTP/test-coupled, no recompile)")

    router_hit = any(crate_has(c, dirs, "src/server.rs")
                     and crate_has(c, dirs, "src/cluster_server.rs") for c in aset)
    if router_hit:
        add("server.rs <-> cluster_server.rs MUST stay in sync (dual-path rule)")
    pyo3_hit = any(crate_uses_pyo3(c, dirs) for c in aset)
    for c in aset:
        if crate_uses_pyo3(c, dirs):
            add(f"{c}: run maturin to rebuild the .abi3.so (embedded Python path)")

    http, ffi = python_sdk_files()
    wire_hit = router_hit or any("wire" in c for c in aset)
    base = lambda fs: ", ".join(os.path.basename(f) for f in fs)
    if wire_hit and http:
        add(f"python SDK HTTP wire clients — no recompile, verify: {base(http)}")
    if pyo3_hit and ffi:
        add(f"python SDK embedded FFI modules — need maturin rebuild, verify: {base(ffi)}")
    return out


def print_impact(targets, members, deps, dirs, also_py):
    order = topo_order(members, deps)
    pos = {m: i for i, m in enumerate(order)}
    # union of targets + their reverse dependents
    affected = {t: "changed" for t in targets}
    for t in targets:
        for m, k in reverse_dependents(t, members, deps).items():
            if m not in affected:
                affected[m] = k
    ordered = sorted(affected, key=lambda m: pos.get(m, 1e9))
    key_files, post_phase = parse_claude_md()

    print("Changed crate(s): " + ", ".join(sorted(targets)) + "\n")
    print("=" * 70)
    print("UPDATE IN THIS ORDER (dependencies -> dependents):")
    print("=" * 70)
    for m in ordered:
        kind = affected[m]
        if kind == "changed":
            tag = "[EDITED]"
        elif kind == "normal":
            tag = "[compile-coupled — cargo forces rebuild]"
        else:
            tag = "[DEV/WIRE — compiler will NOT warn you!]"
        print(f"\n>> {m}  {tag}")
        files = key_files.get(m) or scan_crate_files(m, dirs)
        if not files:
            print("     - (no source files found)")
        for fname, desc in files:
            desc = (desc[:66] + "…") if len(desc) > 67 else desc
            print(f"     - {fname:<26} {desc}".rstrip())
        if crate_is_no_std(m, dirs):
            print("     ! INVARIANT: stays no_std — verify "
                  "`cargo build -p {} --target wasm32-unknown-unknown`".format(m))

    print("\n" + "=" * 70)
    print("COMPILER-INVISIBLE CONSUMERS — verify these by hand + end-to-end test:")
    print("=" * 70)
    invisible = derive_invisible(ordered, members, deps, dirs)
    if also_py:
        invisible.setdefault("python/ uncommitted",
                             "python/ has uncommitted changes — keep the SDK in sync")
    if not invisible:
        print("  (none derived for this change set)")
    for w in sorted(invisible.values()):
        print(f"  !! {w}")

    print("\n" + "=" * 70)
    print("POST-PHASE CHECKLIST (parsed from CLAUDE.md):")
    print("=" * 70)
    if not post_phase:
        print("  (could not parse CLAUDE.md 'after every phase' section)")
    for p in post_phase:
        print(f"  [ ] {p}")


def cmd_impact(args, members, deps, dirs):
    if args:
        targets = set()
        for a in args:
            # accept dir name valori-ffi as alias for pkg valoricore-ffi
            if a in members:
                targets.add(a)
            elif a == "valori-ffi":
                targets.add("valoricore-ffi")
            else:
                sys.exit(f"unknown crate: {a}\nknown: {', '.join(sorted(members))}")
        also_py = False
    else:
        targets, also_py = changed_crates(dirs)
        if not targets and not also_py:
            print("No changed crates detected in the working tree.")
            print("Pass a crate explicitly, e.g.:  kernel-deps.py impact valori-kernel")
            return
        if not targets:
            targets = set()
    if not targets:
        print("Only python/ changed — no Rust crate impact.\n")
    print_impact(targets, members, deps, dirs, also_py)


PUB_RE = re.compile(r"^\+\s*(pub(?:\([^)]*\))?\s+(fn|struct|enum|trait|const|type)\s+([A-Za-z0-9_]+))")
VARIANT_RE = re.compile(r"^\+\s+([A-Z][A-Za-z0-9_]+)\s*[\({,]")


def cmd_api_diff(args, members, deps, dirs):
    target = args[0] if args else "valori-kernel"
    if target == "valori-ffi":
        target = "valoricore-ffi"
    if target not in members:
        sys.exit(f"unknown crate: {target}")
    ref = args[1] if len(args) > 1 else "HEAD"
    cdir = dirs[target]
    diff = sh(["git", "diff", ref, "--", cdir + "/src"]).stdout
    added_syms, added_variants = [], []
    for line in diff.splitlines():
        m = PUB_RE.match(line)
        if m:
            added_syms.append((m.group(2), m.group(3)))
            continue
        v = VARIANT_RE.match(line)
        if v and not line.startswith("+++"):
            added_variants.append(v.group(1))

    print(f"Public-API diff for {target} vs {ref}:\n")
    if not added_syms and not added_variants:
        print("  no new pub items / variants detected (or nothing changed).")
        return
    for kind, name in added_syms:
        print(f"  + pub {kind} {name}")
    for v in sorted(set(added_variants)):
        print(f"  + variant/ident {v}")

    # Check downstream references for each new symbol.
    downstream = sorted(reverse_dependents(target, members, deps).keys())
    print("\nReference check across downstream crates:")
    for _, name in added_syms:
        hits = []
        for dpkg in downstream:
            r = sh(["grep", "-rl", "--include=*.rs", name, dirs[dpkg] + "/src"])
            if r.returncode == 0:
                hits.append(dpkg)
        status = "referenced in " + ", ".join(hits) if hits else "NOT referenced downstream yet"
        flag = "" if hits else "   <-- likely needs wiring"
        print(f"  {name}: {status}{flag}")


def cmd_rebuild(members, deps, dirs):
    targets, _ = changed_crates(dirs)
    if not targets:
        print("No changed crates; running plain workspace build.")
        affected = []
    else:
        affected = set(targets)
        for t in targets:
            affected.update(reverse_dependents(t, members, deps).keys())
        affected = sorted(affected)
        print("Cleaning stale artifacts for: " + ", ".join(affected))
        for pkg in affected:
            sh(["cargo", "clean", "-p", pkg])
    print("\n$ cargo build --workspace")
    r = subprocess.run(["cargo", "build", "--workspace"])
    if r.returncode != 0:
        sys.exit("build failed")
    if "valori-kernel" in (affected or members):
        print("\n$ cargo build -p valori-kernel --target wasm32-unknown-unknown  (no_std guard)")
        subprocess.run(["cargo", "build", "-p", "valori-kernel",
                        "--target", "wasm32-unknown-unknown"])


# ---------------------------------------------------------------------------
# Function-level dependency tracking.
#
# The compiler catches broken *Rust* callers, but NOT the Python SDK or any
# other wire-coupled consumer. These commands build a reverse call-map so that
# when you change/rename/delete a function you can see every place — Rust AND
# Python — that touches it.
# ---------------------------------------------------------------------------
FN_DEF_RE = re.compile(r"\bfn\s+([A-Za-z0-9_]+)")


def search_roots(dirs):
    """All source dirs to grep: every crate's src/ plus the python SDK."""
    roots = [d + "/src" for d in dirs.values() if os.path.isdir(d + "/src")]
    py = os.path.join(repo_root(), "python")
    if os.path.isdir(py):
        roots.append(py)
    return roots


def label_for_path(path, dirs):
    """Human label for a file path: crate name, or 'python-sdk'."""
    for pkg, d in dirs.items():
        if path.startswith(d + os.sep):
            return pkg
    if os.sep + "python" + os.sep in path:
        return "python-sdk"
    return "?"


def classify_hit(name, text):
    """Classify a grep hit for `name` so comments/imports don't masquerade as
    real call sites. Returns one of: def, use, call, comment, ref."""
    s = text.strip()
    if s.startswith(("//", "///", "//!", "*", "#")) and not s.startswith("#["):
        return "comment"          # doc/line comment or python comment
    if re.search(r"\bfn\s+" + re.escape(name) + r"\b", s) or \
       re.search(r"\bdef\s+" + re.escape(name) + r"\b", s):
        return "def"
    if re.match(r"(pub\s+)?use\s", s) or s.startswith(("import ", "from ")):
        return "use"              # Rust `use` path or Python import
    if re.search(re.escape(name) + r"\s*\(", s):
        return "call"             # real invocation: name(
    return "ref"                  # mention (macro arg, value, string, etc.)


# Vendored / generated dirs that must never count as first-party references.
EXCLUDE_DIRS = [".venv", "venv", "site-packages", "__pycache__", "node_modules",
                "target", ".git", "dist", "build", ".next", ".mypy_cache",
                ".pytest_cache", ".eggs"]


def find_refs(name, dirs):
    """Return {label: [(relpath, lineno, kind, text)]} for whole-word `name`,
    each hit classified by classify_hit(). Vendored dirs are excluded so only
    first-party code counts."""
    roots = search_roots(dirs)
    excludes = [f"--exclude-dir={d}" for d in EXCLUDE_DIRS]
    r = sh(["grep", "-rnw", "--include=*.rs", "--include=*.py"] + excludes + [name] + roots)
    out = {}
    for line in r.stdout.splitlines():
        try:
            path, lineno, text = line.split(":", 2)
        except ValueError:
            continue
        kind = classify_hit(name, text)
        out.setdefault(label_for_path(path, dirs), []).append(
            (os.path.relpath(path, repo_root()), lineno, kind, text.strip()))
    return out


MARK = {"def": "def ", "use": "use ", "call": "call", "comment": "note", "ref": "ref "}


def print_callers(name, dirs, members, deps, defining_crate=None):
    refs = find_refs(name, dirs)
    if not refs:
        print(f"  {name}: no references found anywhere.")
        return
    order_key = topo_order(members, deps)
    posn = {m: i for i, m in enumerate(order_key)}
    for label in sorted(refs, key=lambda l: posn.get(l, 99)):
        hits = refs[label]
        ncall = sum(1 for _, _, k, _ in hits if k == "call")
        nreal = sum(1 for _, _, k, _ in hits if k in ("call", "use", "def"))
        tag = f"  ({ncall} call-site(s))"
        if label == "python-sdk" and ncall:
            tag += "  <-- WIRE: compiler will NOT catch a break here"
        elif defining_crate and label == defining_crate:
            tag += "  (defining crate)"
        elif nreal == 0:
            tag += "  (comments/mentions only)"
        print(f"  [{label}]{tag}")
        # real usages first, comments/mentions last
        for relpath, lineno, kind, text in sorted(hits, key=lambda h: h[2] == "comment"):
            print(f"      {MARK[kind]}  {relpath}:{lineno}   {text[:78]}")


def cmd_callers(args, members, deps, dirs):
    if not args:
        sys.exit("usage: kernel-deps.py callers <function_name>")
    name = args[0]
    print(f"Reverse call-map for `{name}` (Rust crates + Python SDK):\n")
    print_callers(name, dirs, members, deps)


def cmd_fn_impact(args, members, deps, dirs):
    """Derive which functions changed vs a git ref (from diff hunk headers +
    added/removed `fn` lines), then show every caller of each — flagging Python
    SDK callers the compiler can't protect."""
    crate = None
    ref = "HEAD"
    for a in args:
        if a in members or a == "valori-ffi":
            crate = "valoricore-ffi" if a == "valori-ffi" else a
        else:
            ref = a
    scope = dirs[crate] + "/src" if crate else "."
    diff = sh(["git", "diff", ref, "--", scope]).stdout

    changed, removed = set(), set()
    for line in diff.splitlines():
        if line.startswith("@@"):
            ctx = line.split("@@")[-1]  # hunk-header function context
            for nm in FN_DEF_RE.findall(ctx):
                changed.add(nm)
        elif line.startswith("-") and not line.startswith("---"):
            for nm in FN_DEF_RE.findall(line):
                changed.add(nm); removed.add(nm)
        elif line.startswith("+") and not line.startswith("+++"):
            for nm in FN_DEF_RE.findall(line):
                changed.add(nm); removed.discard(nm)  # re-added => renamed/modified, not gone

    where = "in " + crate if crate else "across the workspace"
    if not changed:
        print(f"No changed functions detected {where} vs {ref}.")
        return
    print(f"Functions changed {where} vs {ref}: {len(changed)}\n")
    for nm in sorted(changed):
        flag = "  *** signature line removed/renamed — high risk for Python callers ***" \
            if nm in removed else ""
        print("=" * 70)
        print(f"fn {nm}(){flag}")
        print("=" * 70)
        print_callers(nm, dirs, members, deps, defining_crate=crate)
        print()
    print("Reminder: [python-sdk] callers do NOT fail to compile — verify by hand.")


def enclosing_fn(relpath, lineno):
    """Nearest `fn NAME`/`def NAME` at or above lineno — the function that
    contains a given call site. Heuristic but accurate for normal code."""
    path = os.path.join(repo_root(), relpath)
    try:
        with open(path, errors="replace") as f:
            lines = f.readlines()
    except OSError:
        return None
    for i in range(min(int(lineno), len(lines)) - 1, -1, -1):
        m = re.search(r"\bfn\s+([A-Za-z0-9_]+)", lines[i]) or \
            re.search(r"\bdef\s+([A-Za-z0-9_]+)", lines[i])
        if m:
            return m.group(1)
    return None


def real_callers(name, dirs):
    """Distinct (crate, enclosing_fn) pairs that actually CALL `name`."""
    refs = find_refs(name, dirs)
    seen, out = set(), []
    for label, hits in refs.items():
        for relpath, lineno, kind, _ in hits:
            if kind != "call":
                continue
            enc = enclosing_fn(relpath, lineno)
            if enc and enc != name and (label, enc) not in seen:
                seen.add((label, enc))
                out.append((label, enc, relpath, lineno))
    return out


def cmd_calltree(args, members, deps, dirs):
    """Transitive caller tree: `name` <- callers <- their callers ... showing
    the full blast radius out to the edge (HTTP handlers, SDK, etc.)."""
    if not args:
        sys.exit("usage: kernel-deps.py calltree <function_name> [max_depth]")
    name = args[0]
    maxdepth = int(args[1]) if len(args) > 1 else 4
    print(f"Caller tree for `{name}` (who calls it, transitively; depth {maxdepth}):\n")
    print(name)
    visited = set()

    CAP = 25  # guard against generic names (run/main) exploding the tree
    # Names so common that grep can't tell which one is meant — show them but
    # don't recurse (that's where false edges multiply).
    GENERIC = {"run", "main", "new", "apply", "node", "edge", "build", "setup",
               "health", "restore", "delete", "insert", "get", "handle", "call",
               "default", "from", "into", "next", "start", "stop", "test"}

    def walk(fn, depth, prefix):
        callers = sorted(real_callers(fn, dirs))
        truncated = len(callers) > CAP
        shown = callers[:CAP]
        for i, (label, enc, relpath, lineno) in enumerate(shown):
            last = (i == len(shown) - 1) and not truncated
            branch = "└── " if last else "├── "
            wire = "  << WIRE" if label == "python-sdk" else ""
            key = f"{label}:{enc}"
            if key in visited:
                print(f"{prefix}{branch}{enc}  [{label}]  (…already shown)")
                continue
            visited.add(key)
            generic = enc in GENERIC
            note = "  [generic name — not expanded]" if generic else ""
            print(f"{prefix}{branch}{enc}  [{label}]  {relpath}:{lineno}{wire}{note}")
            if depth + 1 < maxdepth and not generic:
                walk(enc, depth + 1, prefix + ("    " if last else "│   "))
        if truncated:
            print(f"{prefix}└── (+{len(callers) - CAP} more callers — name too "
                  f"generic to expand; use `callers {fn}`)")

    walk(name, 0, "")
    print("\n(name-matched: same-named fns in different crates may appear; "
          "verify the file paths.)")


def cmd_tree(args, members, deps, dirs):
    """Internal crate dependency tree rooted at a crate (default valori-node)."""
    root = args[0] if args else "valori-node"
    if root == "valori-ffi":
        root = "valoricore-ffi"
    if root not in members:
        sys.exit(f"unknown crate: {root}")
    print(f"Internal dependency tree for `{root}` (compile-coupled only):\n")
    print(root)

    def walk(crate, prefix, seen):
        norm = sorted({dp for dp, k in deps.get(crate, []) if k == "normal"})
        for i, dp in enumerate(norm):
            last = i == len(norm) - 1
            branch = "└── " if last else "├── "
            if dp in seen:
                print(f"{prefix}{branch}{dp}  (…)")
                continue
            print(f"{prefix}{branch}{dp}")
            walk(dp, prefix + ("    " if last else "│   "), seen | {dp})

    walk(root, "", {root})


# ---------------------------------------------------------------------------
# Ollama integration — `explain <path>`: send each function (one at a time, so
# the context window is never blown) to a local model for a plain-English
# summary + a dependency ripple note. Token-safe: bodies are truncated and one
# function == one request.
# ---------------------------------------------------------------------------
OLLAMA_URL = os.environ.get("OLLAMA_URL", "http://localhost:11434")
OLLAMA_MODEL = os.environ.get("OLLAMA_MODEL", "llama3.2:3b")
MAX_BODY_CHARS = 6000     # ~1.5k tokens; keeps prompt < num_ctx
DEFAULT_FN_CAP = 25       # avoid firing hundreds of requests unintentionally

PROMPT_TMPL = """You are analyzing one {lang} function `{name}` from the Valori \
codebase (a Rust vector database with a Python SDK). Be terse and concrete.

Its signature:
  takes: {inputs}
  gives: {output}

Respond in EXACTLY this format, nothing else:
SUMMARY: <2-3 sentences on what it does>
OUTPUT: <what the returned value represents / its format / error cases>
DEPENDS: <functions, HTTP endpoints, wire/serde types, or Python SDK a change \
here could ripple to; or none>

```{lang}
{body}
```
"""

RUST_FN = re.compile(r"^\s*(pub(?:\([^)]*\))?\s+)?(async\s+)?(unsafe\s+)?fn\s+([A-Za-z0-9_]+)")
PY_DEF = re.compile(r"^(\s*)(async\s+)?def\s+([A-Za-z0-9_]+)")


def ollama_up(model):
    """Return (ok, message). Checks the daemon and that `model` is present."""
    try:
        with urllib.request.urlopen(OLLAMA_URL + "/api/tags", timeout=5) as r:
            tags = json.loads(r.read())
    except Exception as e:
        return False, f"Ollama not reachable at {OLLAMA_URL} ({e}). Is `ollama serve` running?"
    names = [m.get("name", "") for m in tags.get("models", [])]
    if model not in names and model.split(":")[0] not in [n.split(":")[0] for n in names]:
        return False, f"model '{model}' not installed. Have: {', '.join(names)}\nTry: ollama pull {model}"
    return True, ""


def ollama_generate(prompt, model):
    body = json.dumps({
        "model": model, "prompt": prompt, "stream": False,
        "options": {"num_ctx": 4096, "num_predict": 300, "temperature": 0.1},
    }).encode()
    req = urllib.request.Request(OLLAMA_URL + "/api/generate", data=body,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=300) as r:
            return json.loads(r.read()).get("response", "").strip()
    except Exception as e:
        return f"[ollama error: {e}]"


def _extract_rust(rel, lines):
    i, n = 0, len(lines)
    while i < n:
        m = RUST_FN.match(lines[i])
        if not m:
            i += 1
            continue
        name = m.group(4)
        depth, started, body, j = 0, False, [], i
        while j < n:
            line = lines[j]
            body.append(line)
            if not started and ";" in line and "{" not in line:
                break  # trait-method / fn-pointer declaration, no body
            for ch in line:
                if ch == "{":
                    depth += 1
                    started = True
                elif ch == "}":
                    depth -= 1
            if started and depth <= 0:
                break
            j += 1
        if started:
            yield (rel, i + 1, name, "rust", "\n".join(body))
        i = j + 1


def _extract_python(rel, lines):
    i, n = 0, len(lines)
    while i < n:
        m = PY_DEF.match(lines[i])
        if not m:
            i += 1
            continue
        indent, name = len(m.group(1)), m.group(3)
        body, j = [lines[i]], i + 1
        while j < n:
            ln = lines[j]
            if ln.strip() == "":
                body.append(ln)
                j += 1
                continue
            if len(ln) - len(ln.lstrip()) <= indent:
                break
            body.append(ln)
            j += 1
        yield (rel, i + 1, name, "python", "\n".join(body))
        i = j


def _match_parens(s, start):
    """Index of the ')' matching the '(' at s[start], or -1."""
    depth = 0
    for i in range(start, len(s)):
        if s[i] == "(":
            depth += 1
        elif s[i] == ")":
            depth -= 1
            if depth == 0:
                return i
    return -1


def _split_top_commas(s):
    """Split on commas not nested in (), [], <>, {}."""
    parts, depth, cur = [], 0, ""
    for c in s:
        if c in "([<{":
            depth += 1
        elif c in ")]>}":
            depth -= 1
        if c == "," and depth == 0:
            parts.append(cur.strip())
            cur = ""
        else:
            cur += c
    if cur.strip():
        parts.append(cur.strip())
    return parts


def parse_signature(name, lang, body):
    """Return (inputs, output) parsed straight from the signature — exact, no LLM.
    inputs: list of 'name: Type' strings (self/cls dropped for python).
    output: return-type string ('()' / '' when none)."""
    kw = "fn " if lang == "rust" else "def "
    idx = body.find(kw + name)
    if idx == -1:
        idx = body.find(kw)
    p = body.find("(", idx)
    if p == -1:
        return [], ""
    q = _match_parens(body, p)
    params = _split_top_commas(body[p + 1:q]) if q != -1 else []
    rest = body[q + 1:] if q != -1 else ""
    output = ""
    if lang == "rust":
        arrow = rest.find("->")
        if arrow != -1:
            tail = rest[arrow + 2:]
            cut = len(tail)
            for stop in ("{", " where ", ";"):
                k = tail.find(stop)
                if k != -1:
                    cut = min(cut, k)
            output = re.sub(r"\s+", " ", tail[:cut]).strip()
        else:
            output = "()"
    else:  # python
        arrow, colon = rest.find("->"), rest.find(":")
        if arrow != -1 and (colon == -1 or arrow < colon):
            tail = rest[arrow + 2:]
            c = tail.find(":")
            output = re.sub(r"\s+", " ", (tail[:c] if c != -1 else tail)).strip()
        if not output:
            output = "(unannotated)"
        params = [x for x in params if x.split(":")[0].strip() not in ("self", "cls")]
    params = [re.sub(r"\s+", " ", x).strip() for x in params if x.strip()]
    return params, output


def sig_str(inputs):
    return ", ".join(inputs) if inputs else "(nothing)"


def extract_functions(path):
    files = []
    if os.path.isfile(path):
        files = [path]
    else:
        for root, subdirs, fs in os.walk(path):
            subdirs[:] = [d for d in subdirs if d not in EXCLUDE_DIRS]
            for f in sorted(fs):
                if f.endswith((".rs", ".py")):
                    files.append(os.path.join(root, f))
    for fp in sorted(files):
        try:
            lines = open(fp, errors="replace").read().splitlines()
        except OSError:
            continue
        rel = os.path.relpath(fp, repo_root())
        yield from (_extract_rust if fp.endswith(".rs") else _extract_python)(rel, lines)


BATCH_CHAR_BUDGET = 3500   # total function-body chars packed per request
BATCH_MAX_FNS = 6          # hard cap on functions per request


def group_functions(fns):
    """Pack functions into token-budgeted groups for --batch mode."""
    groups, cur, size = [], [], 0
    for f in fns:
        blen = min(len(f[4]), MAX_BODY_CHARS)
        if cur and (size + blen > BATCH_CHAR_BUDGET or len(cur) >= BATCH_MAX_FNS):
            groups.append(cur)
            cur, size = [], 0
        cur.append(f)
        size += blen
    if cur:
        groups.append(cur)
    return groups


def build_batch_prompt(group):
    head = ("You are analyzing functions from the Valori codebase (a Rust vector "
            "database with a Python SDK). For EACH function below, output EXACTLY "
            "this block and nothing else between blocks:\n"
            "<<<NAME>>>\nSUMMARY: <2-3 sentences>\nOUTPUT: <what it returns / its "
            "format / error cases>\nDEPENDS: <functions/HTTP endpoints/wire types/"
            "Python SDK it could ripple to, or none>\n\nFunctions:\n")
    parts = [head]
    for rel, ln, name, lang, body in group:
        inputs, output = parse_signature(name, lang, body)
        parts.append(f"\n[FUNCTION {name} ({lang})  takes: {sig_str(inputs)}  "
                     f"gives: {output or '()'}]\n```{lang}\n{body[:MAX_BODY_CHARS]}\n```\n")
    return "".join(parts)


def print_batch_result(group, text):
    sections = {}
    for m in re.finditer(r"<<<\s*([^>]+?)\s*>>>\s*(.*?)(?=<<<|\Z)", text, re.S):
        sections[m.group(1).strip()] = m.group(2).strip()
    for rel, ln, name, lang, body in group:
        inputs, output = parse_signature(name, lang, body)
        print("=" * 70)
        print(f"{name}()  —  {rel}:{ln}")
        print("=" * 70)
        print(f"TAKES:  {sig_str(inputs)}")
        print(f"GIVES:  {output or '()'}")
        sec = sections.get(name)
        if sec is None:  # loose match (model may reformat the name)
            sec = next((v for k, v in sections.items() if name in k), None)
        print((sec or "[no per-function block parsed — see raw output below]") + "\n")
    if not sections:
        print("--- raw model output (format not followed) ---\n" + text + "\n")


def cmd_explain(args, members, deps, dirs):
    if not args:
        sys.exit("usage: kernel-deps.py explain <file-or-dir> [max_fns] "
                 "[--batch] [--model=llama3.2:3b]\n"
                 "(set OLLAMA_MODEL / OLLAMA_URL to override)")
    model, batch, positional = OLLAMA_MODEL, False, []
    for a in args:
        if a.startswith("--model="):
            model = a.split("=", 1)[1]
        elif a in ("--batch", "--multi"):
            batch = True
        else:
            positional.append(a)
    raw = positional[0]
    path = raw if os.path.isabs(raw) else os.path.abspath(raw)
    if not os.path.exists(path):
        sys.exit(f"path not found: {raw}")
    limit = int(positional[1]) if len(positional) > 1 else None

    ok, msg = ollama_up(model)
    if not ok:
        sys.exit("error: " + msg)

    fns = list(extract_functions(path))
    total = len(fns)
    if limit is None and total > DEFAULT_FN_CAP:
        print(f"Found {total} functions; processing the first {DEFAULT_FN_CAP} "
              f"(pass a number to raise the cap, e.g. `explain {raw} {total}`).\n")
        fns = fns[:DEFAULT_FN_CAP]
    elif limit is not None:
        fns = fns[:limit]

    if batch:
        groups = group_functions(fns)
        print(f"Explaining {len(fns)}/{total} function(s) from {raw} via {model} "
              f"in {len(groups)} batched request(s)\n")
        for g in groups:
            print_batch_result(g, ollama_generate(build_batch_prompt(g), model))
    else:
        print(f"Explaining {len(fns)}/{total} function(s) from {raw} via {model} "
              f"(one request each)\n")
        for rel, lineno, name, lang, body in fns:
            inputs, output = parse_signature(name, lang, body)
            note = ""
            if len(body) > MAX_BODY_CHARS:
                body, note = body[:MAX_BODY_CHARS], "  (body truncated for token budget)"
            ans = ollama_generate(PROMPT_TMPL.format(
                lang=lang, name=name, body=body,
                inputs=sig_str(inputs), output=output or "()"), model)
            print("=" * 70)
            print(f"{name}()  —  {rel}:{lineno}{note}")
            print("=" * 70)
            print(f"TAKES:  {sig_str(inputs)}")
            print(f"GIVES:  {output or '()'}")
            print(ans + "\n")


def main():
    args = sys.argv[1:]
    cmd = args[0] if args else "impact"
    rest = args[1:]
    members, deps, dirs = load_graph()
    if cmd == "order":
        cmd_order(members, deps, dirs)
    elif cmd == "impact":
        cmd_impact(rest, members, deps, dirs)
    elif cmd == "api-diff":
        cmd_api_diff(rest, members, deps, dirs)
    elif cmd == "callers":
        cmd_callers(rest, members, deps, dirs)
    elif cmd == "fn-impact":
        cmd_fn_impact(rest, members, deps, dirs)
    elif cmd == "calltree":
        cmd_calltree(rest, members, deps, dirs)
    elif cmd == "tree":
        cmd_tree(rest, members, deps, dirs)
    elif cmd == "explain":
        cmd_explain(rest, members, deps, dirs)
    elif cmd == "rebuild":
        cmd_rebuild(members, deps, dirs)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    # Play nice when piped to `head`/`less` (avoid BrokenPipeError tracebacks).
    try:
        import signal
        signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    except (ImportError, AttributeError, ValueError):
        pass  # SIGPIPE not available (e.g. Windows)
    try:
        main()
    except BrokenPipeError:
        try:
            sys.stdout.close()
        except Exception:
            pass
