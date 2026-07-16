#!/usr/bin/env python3
# Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
"""
End-to-end crate timing harness.

Launches the REAL valori-node HTTP server (not the embedded FFI, so the full
HTTP → handler → engine → runner → effect → kernel → storage stack is exercised)
and times each product operation end-to-end. For every operation it reports the
measured latency AND the crates on that operation's call path.

╭─ HONESTY NOTE — read this ───────────────────────────────────────────────────╮
│ • Operation latency is MEASURED (median / p95 over N iterations).             │
│ • The crate list per operation is the KNOWN CALL PATH (static architecture    │
│   map), not individually measured wall-time. You cannot measure *exclusive*   │
│   per-crate time from outside the process — that needs Rust `#[instrument]`   │
│   spans inside each crate. The per-crate totals below are therefore           │
│   INCLUSIVE and OVERLAPPING (an op's time is counted toward every crate it    │
│   touches), useful for "which crates are on the hot path", not for a          │
│   flamegraph-style exclusive breakdown.                                       │
│ • Want true exclusive per-crate time? Ask and I'll add tracing spans +        │
│   a subscriber that aggregates by crate — that's a Rust-side change.          │
╰──────────────────────────────────────────────────────────────────────────────╯

Usage:
    python3 scripts/e2e_crate_timing.py                 # default: 30 iters, dim 8
    ITERS=100 python3 scripts/e2e_crate_timing.py       # more iterations
    PORT=4123 DIM=16 python3 scripts/e2e_crate_timing.py

No third-party dependencies — uses only the Python standard library.
"""

import json
import os
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

# ── Config ─────────────────────────────────────────────────────────────────────

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PORT = int(os.environ.get("PORT", "3999"))
DIM = int(os.environ.get("DIM", "8"))
ITERS = int(os.environ.get("ITERS", "30"))
WARMUP = int(os.environ.get("WARMUP", "5"))
BASE = f"http://127.0.0.1:{PORT}"

# ── Crate registry — role of each crate, for the summary ───────────────────────

CRATE_ROLE = {
    "valori-node":     "HTTP server (axum) + routing + handlers",
    "valori-engine":   "stateful orchestrator (Engine, commit funnel)",
    "valori-kernel":   "deterministic core: Q16.16, graph, BLAKE3 chain",
    "valori-index":    "vector index (brute / hnsw / ivf / bq)",
    "valori-search":   "reranker (BM25) + decay re-rank",
    "valori-rag":      "GraphRAG / Tree-RAG / Community",
    "valori-ingest":   "chunking + embedding client",
    "valori-storage":  "event log / WAL / object store",
    "valori-state":    "recovery orchestration (startup replay)",
    "valori-metadata": "collection registry + redb control plane",
    "valori-wire":     "serialization + event-log V4 format",
    "valori-planner":  "Op→ExecutionGraph — INLINE builder (A13); cache INACTIVE",
    "valori-effect":   "EffectBus + capability traits + runner",
    "valori-consensus":"Raft state machine (CLUSTER ONLY — not this run)",
}

# Handlers confirmed to route through run_graph_inline (planner + effect + runner).
# Verified in server.rs: snapshot_save, insert_record, graphrag,
# memory_search_vector, tree_*, community_*.
PLANNER_PATH = ["valori-planner", "valori-effect"]

# Planner evolution marker — bump this as the migration advances so every
# benchmark run doubles as an architecture changelog.
#   A13  = inline builder, planner cache INACTIVE (today)
#   A15+ = structural planner, cached ExecutionPlan + runtime bindings (future)
PLANNER_VERSION = "A13"


class Op:
    """One timed operation.

    Fields beyond timing describe the EXECUTION PIPELINE truthfully:
      • crates     — ordered call path (declared, not measured)
      • op_kind    — planner OperationKind, or None for a direct engine call
      • graph      — (task_kinds, edge_count) for graph-routed ops, else None
      • writes     — mutates KernelState (→ state hash changes, height grows)
    """
    def __init__(self, name, method, path, body_fn, crates,
                 op_kind=None, graph=None, writes=False, note=""):
        self.name = name
        self.method = method
        self.path = path
        self.body_fn = body_fn          # callable(state) -> dict|None
        self.crates = crates
        self.op_kind = op_kind
        self.graph = graph              # (["InsertRecord"], 0)  or  None
        self.writes = writes
        self.note = note
        self.samples_ms = []
        # observed proof, captured around the measured loop:
        self.hash_before = self.hash_after = None
        self.height_before = self.height_after = None
        self.exec_block = None          # observed _execution (if ?explain wired)

    @property
    def model(self):
        """Standardized execution model: DIRECT | INLINE | CACHED.
        CACHED is reserved for after the structural-planner migration."""
        return "INLINE" if self.graph else "DIRECT"


def vec(seed):
    return [round((seed + i) * 0.1, 4) for i in range(DIM)]


# ── The operation suite ────────────────────────────────────────────────────────
# `state` carries IDs discovered during the run (record ids, node ids) so later
# ops reference real data.

def build_ops():
    # crates lists are ORDERED to reflect the actual call path (top → bottom).
    # graph = (task_kinds, edge_count) is the REAL inline ExecutionGraph, read
    # from server.rs. Every routed op today is single-task / zero-edge — the
    # multi-task pipeline (Search→GraphExpand→Rerank) is the FUTURE structural
    # planner, not what ships today. The benchmark shows what is real.
    return [
        Op("health", "GET", "/health", None,
           ["valori-node", "valori-engine", "valori-kernel"]),

        Op("create_collection", "POST", "/v1/namespaces",
           lambda s: {"name": "bench"},
           ["valori-node", "valori-engine", "valori-metadata", "valori-kernel"],
           writes=True),

        Op("insert_record", "POST", "/records",
           lambda s: {"values": vec(s["i"]), "collection": "bench",
                      "text": f"benchmark chunk number {s['i']}"},
           ["valori-node"] + PLANNER_PATH + ["valori-engine", "valori-kernel",
            "valori-index", "valori-storage", "valori-wire"],
           op_kind="Ingest", graph=(["InsertRecord"], 0), writes=True),

        Op("search", "POST", "/search",
           lambda s: {"query": vec(1), "k": 5, "collection": "bench",
                      "query_text": "benchmark chunk"},
           ["valori-node", "valori-engine", "valori-kernel", "valori-index",
            "valori-search"]),

        Op("graph_node", "POST", "/graph/node",
           lambda s: {"kind": 0, "collection": "bench"},
           ["valori-node", "valori-engine", "valori-kernel", "valori-storage"],
           writes=True),

        Op("graph_edge", "POST", "/graph/edge",
           lambda s: {"from": s.get("node_a", 0), "to": s.get("node_b", 0),
                      "kind": 1, "collection": "bench"},
           ["valori-node", "valori-engine", "valori-kernel", "valori-storage"],
           writes=True),

        Op("graphrag", "POST", "/v1/graphrag",
           lambda s: {"query_vector": vec(1), "k": 5, "depth": 2,
                      "collection": "bench"},
           ["valori-node"] + PLANNER_PATH + ["valori-rag", "valori-engine",
            "valori-kernel", "valori-index"],
           op_kind="GraphRag", graph=(["GraphRag"], 0)),

        Op("memory_upsert", "POST", "/v1/memory/upsert_vector",
           lambda s: {"vector": vec(s["i"]), "collection": "bench"},
           ["valori-node", "valori-engine", "valori-kernel", "valori-index",
            "valori-storage"], writes=True),

        Op("memory_search", "POST", "/v1/memory/search_vector",
           lambda s: {"query_vector": vec(1), "k": 5, "collection": "bench"},
           ["valori-node"] + PLANNER_PATH + ["valori-engine", "valori-kernel",
            "valori-index", "valori-search"],
           op_kind="MemorySearch", graph=(["MemorySearch"], 0)),

        Op("proof_event_log", "GET", "/v1/proof/event-log", None,
           ["valori-node", "valori-storage", "valori-kernel"]),

        Op("timeline", "GET", "/v1/timeline", None,
           ["valori-node", "valori-storage", "valori-kernel"]),
    ]


# ── HTTP ───────────────────────────────────────────────────────────────────────

def http(method, path, body=None):
    url = BASE + path
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    if data is not None:
        req.add_header("Content-Type", "application/json")
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=30) as resp:
        payload = resp.read()
    dt_ms = (time.perf_counter() - t0) * 1000.0
    parsed = json.loads(payload) if payload else None
    return dt_ms, parsed


def proof_snapshot():
    """Observed audit state: (state_hash, committed_height). This is REAL —
    read from the server's proof endpoints, not declared."""
    state_hash = height = None
    try:
        _, p = http("GET", "/v1/proof/state")
        if isinstance(p, dict):
            state_hash = p.get("final_state_hash")
    except Exception:
        pass
    try:
        _, p = http("GET", "/v1/proof/event-log")
        if isinstance(p, dict):
            height = p.get("committed_height")
    except Exception:
        pass
    return state_hash, height


# ── Node lifecycle ─────────────────────────────────────────────────────────────

def ensure_binary():
    binary = os.path.join(ROOT, "target", "debug", "valori-node")
    if not os.path.exists(binary):
        print("→ building valori-node (debug)…")
        subprocess.run(["cargo", "build", "-p", "valori-node"], cwd=ROOT, check=True)
    return binary


def start_node(binary, workdir):
    env = dict(os.environ)
    env.update({
        "VALORI_DIM": str(DIM),
        "VALORI_BIND": f"127.0.0.1:{PORT}",
        "VALORI_INDEX": "brute",
        "VALORI_EVENT_LOG_PATH": os.path.join(workdir, "events.log"),
        "VALORI_SNAPSHOT_PATH": os.path.join(workdir, "snapshot.val"),
        "RUST_LOG": "warn",
    })
    log = open(os.path.join(workdir, "node.log"), "w")
    proc = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT)
    # Poll /health until ready.
    for _ in range(120):
        if proc.poll() is not None:
            log.flush()
            raise RuntimeError(f"node exited early; see {workdir}/node.log")
        try:
            http("GET", "/health")
            return proc, log
        except (urllib.error.URLError, ConnectionError, OSError):
            time.sleep(0.25)
    raise RuntimeError("node did not become healthy within 30s")


# ── Run ────────────────────────────────────────────────────────────────────────

def seed_state():
    """Create the IDs later ops depend on; return the shared state dict."""
    state = {"i": 0}
    # collection
    try:
        http("POST", "/v1/namespaces", {"name": "bench"})
    except urllib.error.HTTPError:
        pass  # already exists on a warm dir — fine
    # a couple of records so search returns hits
    for k in range(3):
        state["i"] = k
        http("POST", "/records", {"values": vec(k), "collection": "bench",
                                  "text": f"seed chunk {k}"})
    # two nodes for the edge op
    _, a = http("POST", "/graph/node", {"kind": 0, "collection": "bench"})
    _, b = http("POST", "/graph/node", {"kind": 0, "collection": "bench"})
    state["node_a"] = a.get("node_id", 0) if isinstance(a, dict) else 0
    state["node_b"] = b.get("node_id", 0) if isinstance(b, dict) else 0
    return state


def time_op(op, state):
    # warmup
    for w in range(WARMUP):
        state["i"] += 1
        body = op.body_fn(state) if op.body_fn else None
        try:
            http(op.method, op.path, body)
        except urllib.error.HTTPError as e:
            print(f"  ⚠ {op.name}: HTTP {e.code} — {e.read()[:200]!r}")
            return False
    # observed audit state BEFORE the measured loop
    op.hash_before, op.height_before = proof_snapshot()
    # measure
    for _ in range(ITERS):
        state["i"] += 1
        body = op.body_fn(state) if op.body_fn else None
        dt, _ = http(op.method, op.path, body)
        op.samples_ms.append(dt)
    # observed audit state AFTER
    op.hash_after, op.height_after = proof_snapshot()
    # probe ?explain=true once — if the handler has the block wired, we get the
    # REAL graph_hash + state_hash straight from the server (no fabrication).
    if op.graph and op.method == "POST":
        try:
            sep = "&" if "?" in op.path else "?"
            _, p = http(op.method, op.path + sep + "explain=true", op.body_fn(state))
            if isinstance(p, dict) and isinstance(p.get("_execution"), dict):
                op.exec_block = p["_execution"]
        except Exception:
            pass
    return True


def pct(vals, p):
    if not vals:
        return 0.0
    s = sorted(vals)
    idx = min(len(s) - 1, int(round((p / 100.0) * (len(s) - 1))))
    return s[idx]


def op_kind_disp(kind):
    return kind or "?"


def render_stack(crates, indent="           "):
    """Vertical-ish call path: 'a → b → c' wrapped to keep lines readable."""
    lines, cur = [], ""
    for c in crates:
        piece = c if not cur else " → " + c
        if len(cur) + len(piece) > 66:
            lines.append(cur)
            cur = c
        else:
            cur += piece
    if cur:
        lines.append(cur)
    return ("\n" + indent).join(lines)


def report(ops):
    # ── Section 1: compact timing table ────────────────────────────────────────
    print("\n" + "=" * 92)
    print(f"OPERATION TIMING   (dim={DIM}, iters={ITERS}, warmup={WARMUP}, index=brute; all times in ms)")
    print("=" * 92)
    hdr = (f"{'operation':<18}{'method':<7}{'median':>8}{'p95':>8}{'min':>8}{'max':>8}"
           f"  {'model':<7}{'graph':<8}audit")
    print(hdr)
    print("-" * 92)
    for op in ops:
        if not op.samples_ms:
            print(f"{op.name:<18}{op.method:<7}{'SKIPPED (see warning above)':>36}")
            continue
        med = statistics.median(op.samples_ms)
        graph = f"{len(op.graph[0])}t/{op.graph[1]}e" if op.graph else "—"
        audit = "hash+log" if op.writes else "read"
        print(f"{op.name:<18}{op.method:<7}"
              f"{med:>8.2f}{pct(op.samples_ms,95):>8.2f}"
              f"{min(op.samples_ms):>8.2f}{max(op.samples_ms):>8.2f}"
              f"  {op.model:<7}{graph:<8}{audit}")

    # ── Section 2: execution pipeline (the "documentation / demo" view) ─────────
    print("\n" + "=" * 92)
    print("EXECUTION PIPELINE   (per operation: call stack · real graph · observed proof)")
    print("=" * 92)
    for op in ops:
        if not op.samples_ms:
            continue
        med = statistics.median(op.samples_ms)
        print(f"\n▸ {op.name}   {op.method} {op.path}   median {med:.2f} ms   [{op.model}]")
        print(f"    stack:   {render_stack(op.crates)}")
        if op.graph:
            tasks, edges = op.graph
            # Named task-flow chain — people remember names, not "1 task / 0 edges".
            # A single task renders as [Name]; multiple render as A → B → C,
            # so this line naturally becomes the full pipeline once the planner
            # emits multi-task graphs (e.g. Embed → InsertRecord → InsertNode).
            flow = " → ".join(tasks) if len(tasks) > 1 else f"[{tasks[0]}]" if tasks else "(empty)"
            n = len(tasks)
            print(f"    plan :   OperationKind::{op_kind_disp(op.op_kind)}  ·  "
                  f"model: INLINE  ·  version: {PLANNER_VERSION}  ·  cache: INACTIVE")
            print(f"    graph:   {flow}"
                  f"   ({n} task{'' if n == 1 else 's'} · {edges} edge{'' if edges == 1 else 's'})")
            # If ?explain=true is wired for this op, print the REAL graph hash
            # observed from the server; otherwise say so truthfully.
            if op.exec_block and op.exec_block.get("graph_hash"):
                gh = op.exec_block["graph_hash"]
                print(f"    hash :   graph_hash {gh[:16]}…  (OBSERVED via ?explain=true) ✓")
            else:
                print(f"    hash :   content-addressed (ExecutionGraph.graph_hash, BLAKE3) "
                      f"— ?explain not wired for this op yet")
        else:
            print(f"    plan :   direct engine call — no ExecutionGraph "
                  f"(model: DIRECT — not yet routed through the planner)")
        # observed audit proof
        if op.writes:
            changed = (op.hash_before != op.hash_after)
            hb = (op.hash_before or "?")[:12]
            ha = (op.hash_after or "?")[:12]
            dh = ""
            if op.height_before is not None and op.height_after is not None:
                dh = f"  ·  committed_height {op.height_before} → {op.height_after}"
            verdict = "CHANGED ✓ (audited write)" if changed else "unchanged (?)"
            print(f"    proof:   state_hash {verdict}{dh}")
            print(f"             {hb}… → {ha}…")
        else:
            unchanged = (op.hash_before == op.hash_after)
            print(f"    proof:   read-only  ·  state_hash "
                  f"{'stable ✓' if unchanged else 'CHANGED (unexpected!)'}  "
                  f"·  ({(op.hash_after or '?')[:12]}…)")

    # ── Section 3: per-crate attribution (inclusive, overlapping) ──────────────
    print("\n" + "=" * 92)
    print("CRATE ATTRIBUTION   (inclusive call-path time — NOT exclusive; see honesty note)")
    print("=" * 92)
    print(f"{'crate':<18}{'ops':>4}{'Σ median ms':>13}   role  (times in ms)")
    print("-" * 92)
    crate_ms = {}
    crate_ops = {}
    for op in ops:
        if not op.samples_ms:
            continue
        med = statistics.median(op.samples_ms)
        for c in set(op.crates):
            crate_ms[c] = crate_ms.get(c, 0.0) + med
            crate_ops[c] = crate_ops.get(c, 0) + 1
    for c in sorted(crate_ms, key=lambda k: -crate_ms[k]):
        print(f"{c:<18}{crate_ops[c]:>4}{crate_ms[c]:>12.2f}m   {CRATE_ROLE.get(c,'')}")

    # crates never touched this run
    touched = set(crate_ms)
    print("\nnot exercised in this run:")
    for c in CRATE_ROLE:
        if c not in touched:
            print(f"  · {c:<16} {CRATE_ROLE[c]}")

    print("\nreminder: Σ median is INCLUSIVE (an op counts toward every crate on its")
    print("path), so columns overlap and do not sum to wall time.")

    # ── Section 4: planner truth + the future flamegraph ───────────────────────
    print("\n" + "=" * 92)
    print("PLANNER STATUS (truthful)")
    print("=" * 92)
    print("  today   :  handlers build the ExecutionGraph INLINE (A13).")
    print("             plan_with_cache + ExecutionCache exist but are NOT wired.")
    print("             every routed op is single-task / zero-edge.")
    print("  future  :  structural planner → cached ExecutionPlan + runtime bindings,")
    print("             then multi-task graphs (e.g. MemorySearch→GraphExpand→Rerank).")
    print()
    print("  next level — TRUE exclusive per-crate time (needs Rust #[instrument]")
    print("  spans + a tracing subscriber). Target shape, once wired:")
    print("      HTTP 0.06 · Planner 0.03 · Runner 0.02 · Capability 0.05")
    print("      Kernel 0.18 · Index 0.14 · Storage 0.04 · Serde 0.02  → Total 0.54")
    print("  (this harness measures op latency + observed proof; it cannot split")
    print("   exclusive per-crate time from outside the process.)")

    # ── Section 5: execution-model summary (system health at a glance) ─────────
    timed = [o for o in ops if o.samples_ms]
    routed = [o for o in timed if o.graph]
    total_tasks = sum(len(o.graph[0]) for o in routed)
    total_edges = sum(o.graph[1] for o in routed)
    writes = [o for o in timed if o.writes]
    audited = [o for o in writes if o.hash_before != o.hash_after]
    reads = [o for o in timed if not o.writes]
    # real events appended, from observed committed_height deltas
    appended = sum((o.height_after - o.height_before)
                   for o in timed
                   if isinstance(o.height_before, int) and isinstance(o.height_after, int))
    print("\n" + "=" * 92)
    print("EXECUTION MODEL SUMMARY")
    print("=" * 92)
    print(f"  Planner-routed operations : {len(routed)} / {len(timed)}   (model INLINE, version {PLANNER_VERSION})")
    print(f"  Direct engine operations  : {len(timed) - len(routed)} / {len(timed)}")
    print(f"  ExecutionGraph tasks      : {total_tasks}   (all single-task today)")
    print(f"  ExecutionGraph edges      : {total_edges}")
    print(f"  Audited writes            : {len(audited)}   (state_hash observed to change)")
    print(f"  Read-only operations      : {len(reads)}")
    print(f"  Audit events appended     : {appended}   (Σ committed_height delta, observed)")
    print(f"  Receipt-emitting ops run  : 0   (receipts today = tree/community only; not in this suite)")
    print(f"  Planner cache             : INACTIVE")


def main():
    print("=" * 92)
    print("VALORI — END-TO-END CRATE TIMING HARNESS")
    print("=" * 92)
    binary = ensure_binary()
    workdir = tempfile.mkdtemp(prefix="valori_e2e_timing_")
    proc = log = None
    try:
        print(f"→ starting node on {BASE}  (workdir: {workdir})")
        proc, log = start_node(binary, workdir)
        print("→ node healthy; seeding state…")
        state = seed_state()
        ops = build_ops()
        print(f"→ timing {len(ops)} operations × {ITERS} iterations each…")
        for op in ops:
            time_op(op, state)
        report(ops)
    finally:
        if proc is not None:
            proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
        if log is not None:
            log.close()
        shutil.rmtree(workdir, ignore_errors=True)
    print("\n✅ done.\n")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
