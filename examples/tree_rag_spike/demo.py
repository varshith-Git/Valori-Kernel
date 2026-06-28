"""
End-to-end demo:  build the map -> store it -> answer with a citation ->
prove the receipt -> catch tampering.

Run (no API key, no services needed):
    python3 pageindex/valori_tree_rag/demo.py
"""
from __future__ import annotations

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from tree_rag import TreeIndex          # noqa: E402
from receipt import ReceiptLog, HASH_NAME  # noqa: E402
from store import LocalStore            # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
DOC = os.path.join(HERE, "sample_docs", "employee_handbook.md")
WORKSPACE = os.path.join(HERE, ".workspace")

LINE = "=" * 70


def rule(title: str) -> None:
    print(f"\n{LINE}\n{title}\n{LINE}")


def print_map(nodes, indent=0):
    for n in nodes:
        print("  " * indent + f"[{n['node_id']}] {n['title']}  —  {n['summary'][:54]}")
        if n.get("nodes"):
            print_map(n["nodes"], indent + 1)


def main() -> None:
    # 1 — BUILD THE MAP (zero LLM calls for a structured doc)
    rule("1. BUILD THE MAP  (tree index — no embeddings, no chunking, no LLM)")
    with open(DOC, encoding="utf-8") as f:
        text = f.read()
    index = TreeIndex.from_markdown(text, doc_name="employee_handbook.md")
    print(f"Document: {index.doc_name}   nodes: {len(index.nodes)}   hash: {HASH_NAME}\n")
    print_map(index.structure_map())

    # 2 — STORE IT
    rule("2. STORE IT  (LocalStore JSON; maps to Valori records + graph + audit)")
    log = ReceiptLog()
    store = LocalStore(WORKSPACE)
    store.save("handbook", index, log)
    print(f"Saved index + receipt log to {WORKSPACE}/")

    # 3 — ASK QUESTIONS (reason over the tree -> cited answers + receipts)
    rule("3. ANSWER WITH A CITATION  (+ a provable receipt per answer)")
    questions = [
        "How many paid sick days do I get?",
        "Can I work from home, and how often?",
        "What is the retirement contribution match?",
    ]
    for q in questions:
        res = index.answer(q, log)
        cite = res.citations[0] if res.citations else {"breadcrumb": "—", "lines": []}
        print(f"\nQ: {q}")
        print(f"   navigator: {res.reasoning}")
        print(f"   answer:    {_one_line(res.answer)}")
        print(f"   citation:  {cite['breadcrumb']}  (lines {cite['lines']})")
        print(f"   receipt:   {res.receipt['receipt_hash'][:24]}…  "
              f"(evidence {res.receipt['evidence_hash'][:12]}…)")
    store.save("handbook", index, log)

    # 4 — PROVE IT  (replay receipts; verify the chain)
    rule("4. PROVE IT  (replay the receipt + verify the tamper-evident chain)")
    last = log.receipts[-1]
    print(f"Replay last receipt against the stored index: "
          f"{'VALID ✓' if index.verify_receipt(last.to_dict()) else 'INVALID ✗'}")
    print(f"Receipt chain intact (nothing removed/reordered): "
          f"{'YES ✓' if log.verify_chain() else 'NO ✗'}")
    print(f"Chain head: {log.head[:32]}…")

    # 5 — CATCH TAMPERING  (the moat made tangible)
    rule("5. CATCH TAMPERING  (alter a stored section -> proof breaks)")
    target = last.visited_node_ids[0]
    original = index.nodes[target].own_text
    tampered = re.sub(r"\d+%", "99%", original, count=1)
    if tampered == original:                       # node had no percentage to flip
        tampered = re.sub(r"\b\d+\b", "999", original, count=1)
    if tampered == original:                       # no number at all: append
        tampered = original + " (unauthorized edit)"
    print(f"Someone edits stored section [{target}] ({index.nodes[target].title})…")
    print(f"   before: {_one_line(original, 70)}")
    print(f"   after:  {_one_line(tampered, 70)}")
    index.nodes[target].own_text = tampered
    ok = index.verify_receipt(last.to_dict())
    print(f"Replay the SAME receipt against the altered index: "
          f"{'VALID ✓ (tamper missed!)' if ok else 'TAMPERING DETECTED ✗'}")
    index.nodes[target].own_text = original  # restore

    rule("DONE")
    print("This is the off-kernel Layer-1 spike. Next phase: store the tree as a")
    print("Valori graph + records, and commit each receipt to the kernel audit chain.")
    llm = bool(os.getenv("OPENAI_API_KEY") or os.getenv("ANTHROPIC_API_KEY"))
    print(f"\nLLM reasoning/answers: {'ON' if llm else 'OFF (deterministic)'}  "
          f"— set OPENAI_API_KEY + `pip install litellm` to enable prose answers.")


def _one_line(s: str, limit: int = 120) -> str:
    s = " ".join(s.split())
    return s if len(s) <= limit else s[:limit] + "…"


if __name__ == "__main__":
    main()
