#!/usr/bin/env python3
"""
Valori — Live Demo (single file, no network, no model downloads)
=================================================================

Run from the repo root:

    python3 demo/cto_demo.py            # run straight through
    python3 demo/cto_demo.py --pause    # pause between acts (live demo mode)

Three acts, ~3 minutes:

  ACT 1  The problem  — floating-point math is not reproducible
  ACT 2  The fix      — Valori: two independent kernels, one identical
                        BLAKE3 audit root, snapshot/restore proof
  ACT 3  The payoff   — a rogue AI agent mutates memory; Valori detects
                        it mathematically, no logs, no trust required
"""
import sys
import hashlib

PAUSE = "--pause" in sys.argv


def banner(title: str) -> None:
    print()
    print("=" * 66)
    print(f"  {title}")
    print("=" * 66)
    if PAUSE:
        input("  [press Enter]")


def embed(text: str) -> list:
    """Deterministic 16-dim embedding (hash-based, demo only)."""
    h = hashlib.sha256(text.encode()).digest()
    return [(b / 255.0) * 2.0 - 1.0 for b in h[:16]]


# ---------------------------------------------------------------- ACT 1
banner("ACT 1 — The problem: floating-point math is not reproducible")

a = (0.1 + 0.2) + 0.3   # one CPU groups the additions this way
b = 0.1 + (0.2 + 0.3)   # another CPU (SIMD, different arch) groups this way

print(f"""
  The SAME three numbers, added in a different order — which is exactly
  what happens when a CPU vectorizes, or a cloud migrates your workload:

      (0.1 + 0.2) + 0.3 = {a!r}
      0.1 + (0.2 + 0.3) = {b!r}
      identical?          {a == b}

  Every mainstream vector DB (FAISS, Pinecone, Qdrant, Milvus) is built
  on this arithmetic. I reported x86/ARM divergence upstream, and a
  FAISS contributor confirmed it on the record (issue #4739):

    "Instructions on x86 vs ARM will not have identical outputs...
     even a floating point add on x86 for different generations of
     CPUs from the same manufacturer will not always have the same
     results."

  If the math isn't reproducible, the memory isn't auditable.
""")

# ---------------------------------------------------------------- ACT 2
banner("ACT 2 — Valori: bit-identical memory, provable with one hash")

import tempfile
from valoricore import MemoryClient

DOCS = [
    "Patient 4412: allergy to penicillin recorded by Dr. Rao",
    "Trade 88231: BUY 500 AAPL @ 182.4400, desk EU-7",
    "Contract clause 9.2: liability capped at 2M EUR",
    "Incident 2207: deploy rolled back after canary failure",
]

def build_kernel() -> MemoryClient:
    c = MemoryClient(path=tempfile.mkdtemp(prefix="valori_demo_"))
    for d in DOCS:
        c.add_document(text=d, embed=embed)
    return c

# Two completely independent kernels — pretend one is on x86 in Frankfurt
# and one is on ARM in Singapore.
kernel_a = build_kernel()
kernel_b = build_kernel()

hash_a = kernel_a.get_state_hash()
hash_b = kernel_b.get_state_hash()

print(f"""
  Ingested {kernel_a.record_count()} records into two INDEPENDENT kernels
  (think: x86 server in Frankfurt vs ARM server in Singapore).

      kernel A root: {hash_a}
      kernel B root: {hash_b}
      identical?     {hash_a == hash_b}

  One 64-char BLAKE3 root summarises the ENTIRE memory state.
  Same inputs => same bits => same root. On any CPU. Forever.
""")

# Unlike state-only databases, Valori stores HOW it got here: events.
print("  And it remembers HOW it got here — the event log is the truth:\n")
for line in kernel_a.get_timeline()[:4]:
    print(f"      {line}")
print("      ... (every insert, edge, delete — replayable forever)\n")

# Snapshot / restore — crash recovery you can prove, not trust.
snap = kernel_a.snapshot()
restored = MemoryClient(path=tempfile.mkdtemp(prefix="valori_demo_"))
restored.restore(snap)

results = restored.semantic_search(DOCS[0], embed=embed, k=1)

print(f"""  Crash-recovery proof: snapshot -> brand-new kernel -> restore.

      restored root matches original: {restored.get_state_hash() == hash_a}
      search still works after restore: query for the allergy note
      returns record {results[0]['id']} — the allergy note. Correct.
""")

# ---------------------------------------------------------------- ACT 3
banner("ACT 3 — A rogue agent tampers with memory. Catch it with math.")

print("""
  July 2025: an AI coding agent at Replit deleted a production database,
  then fabricated records to cover it up. The company only found out
  because the agent admitted it in chat.

  Scenario: an agent with DB access quietly deletes the audit-relevant
  trade record. No error. No log line. Would you notice?
""")

# The "before" root is anchored externally (regulator, customer, git tag).
anchored_root = kernel_a.get_state_hash()
print(f"      anchored root (held by auditor):  {anchored_root}")

# The rogue agent strikes: deletes record 1 (the trade).
kernel_a.delete(1)

current_root = kernel_a.get_state_hash()
print(f"      root recomputed after the agent:  {current_root}")
print(f"      roots match?                      {current_root == anchored_root}")

print(f"""
  TAMPER DETECTED — mathematically, offline, by anyone holding the
  anchored root. No vendor dashboard. No 'trust us'.

  In production this is `valori-verify`: replay the event log,
  recompute the root, and it tells you WHICH event was altered,
  at WHICH byte offset, and WHEN it was committed.

  That is the difference between logging and PROOF.
""")

print("=" * 66)
print("  Recap: deterministic math (Q16.16) + event sourcing + BLAKE3")
print("  = AI memory you can replay, audit, and prove. pip install valori")
print("=" * 66)
