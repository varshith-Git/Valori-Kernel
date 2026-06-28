"""
Tree-RAG: PageIndex-style hierarchical retrieval, fused with Valori proof.

What this is
------------
A small, dependency-free prototype of the "build the map -> store it -> answer
with a citation -> emit a provable receipt" flow we scoped for Valori.

It mirrors PageIndex's two ideas:
  1. Build a *tree index* of a document (a table-of-contents the machine can read).
  2. *Reason over the tree* to fetch the exact relevant sections — no chunking,
     no embeddings, no vector DB.

...and adds the two things PageIndex does not have, which are Valori's moat:
  - every answer cites the exact section + line range it came from, and
  - every retrieval emits a BLAKE3-chained, replayable receipt (see receipt.py).

How it maps onto Valori (the real integration, not built here)
--------------------------------------------------------------
  - Tree node  -> a graph node (Valori already has a knowledge graph).
  - Section text / pages -> Valori records (the verbatim, addressable content).
  - Receipt chain -> the kernel audit chain (events.log / state hash).
The LLM only ever lives off-kernel (in valori-node), exactly as discussed.

LLM dependency
--------------
For a structured document, building the map and navigating it need ZERO LLM
calls — it is pure text processing. An LLM is *optional*, used only to (a) pick
nodes with reasoning and (b) compose a prose answer. If no API key is present,
a transparent deterministic navigator and verbatim-evidence answer are used,
so the whole thing runs offline.
"""
from __future__ import annotations

import os
import re
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple

from receipt import ReceiptLog, hash_text

# ── tiny stopword list for the deterministic navigator ───────────────────────
_STOP = {
    "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "is", "are",
    "do", "does", "i", "you", "my", "our", "how", "what", "when", "where",
    "which", "can", "may", "get", "much", "many", "per", "with", "at", "be",
    "if", "it", "this", "that", "as", "by", "from", "have", "has",
}

_WORD = re.compile(r"[a-z0-9]+")
_HEADER = re.compile(r"^(#{1,6})\s+(.+)$")


def _tokens(text: str) -> List[str]:
    return [w for w in _WORD.findall(text.lower()) if len(w) > 1 and w not in _STOP]


# ── the tree ─────────────────────────────────────────────────────────────────
@dataclass
class Node:
    node_id: str
    title: str
    level: int
    start_line: int          # 1-indexed line where this heading appears
    end_line: int            # last line owned by this section (excl. children)
    own_text: str            # verbatim section body, excluding sub-sections
    summary: str             # first line of body, no LLM needed
    parent: Optional[str] = None
    children: List[str] = field(default_factory=list)

    def to_dict(self) -> dict:
        return {
            "node_id": self.node_id, "title": self.title, "level": self.level,
            "start_line": self.start_line, "end_line": self.end_line,
            "own_text": self.own_text, "summary": self.summary,
            "parent": self.parent, "children": list(self.children),
        }


class TreeIndex:
    """A hierarchical, page/line-addressable index of one document."""

    def __init__(self, doc_name: str, nodes: Dict[str, Node], roots: List[str]):
        self.doc_name = doc_name
        self.nodes = nodes
        self.roots = roots

    # ---- construction (no LLM for structured docs) --------------------------
    @classmethod
    def from_markdown(cls, text: str, doc_name: str) -> "TreeIndex":
        lines = text.split("\n")
        headers: List[Tuple[int, int, str]] = []  # (line_num, level, title)
        in_code = False
        for i, line in enumerate(lines, 1):
            s = line.strip()
            if s.startswith("```"):
                in_code = not in_code
                continue
            if in_code:
                continue
            m = _HEADER.match(s)
            if m:
                headers.append((i, len(m.group(1)), m.group(2).strip()))

        nodes: Dict[str, Node] = {}
        roots: List[str] = []
        stack: List[Tuple[str, int]] = []  # (node_id, level)

        for idx, (line_num, level, title) in enumerate(headers):
            node_id = f"{idx + 1:04d}"
            # body = lines after this header up to the next header of any level
            next_header_line = headers[idx + 1][0] if idx + 1 < len(headers) else len(lines) + 1
            body_lines = lines[line_num:next_header_line - 1]
            own_text = "\n".join(body_lines).strip()
            end_line = next_header_line - 1
            summary = _first_sentence(own_text) or title

            node = Node(
                node_id=node_id, title=title, level=level,
                start_line=line_num, end_line=end_line,
                own_text=own_text, summary=summary,
            )
            nodes[node_id] = node

            while stack and stack[-1][1] >= level:
                stack.pop()
            if stack:
                parent_id = stack[-1][0]
                node.parent = parent_id
                nodes[parent_id].children.append(node_id)
            else:
                roots.append(node_id)
            stack.append((node_id, level))

        return cls(doc_name, nodes, roots)

    # ---- addressing & navigation --------------------------------------------
    def breadcrumb(self, node_id: str) -> str:
        parts, cur = [], self.nodes[node_id]
        while cur is not None:
            parts.append(cur.title)
            cur = self.nodes[cur.parent] if cur.parent else None
        return " > ".join(reversed(parts))

    def structure_map(self) -> List[dict]:
        """The compact 'table of contents' an agent reasons over (titles+summaries, no body)."""
        def build(nid: str) -> dict:
            n = self.nodes[nid]
            d = {"node_id": n.node_id, "title": n.title, "summary": n.summary}
            if n.children:
                d["nodes"] = [build(c) for c in n.children]
            return d
        return [build(r) for r in self.roots]

    def get_text(self, node_id: str) -> str:
        return self.nodes[node_id].own_text

    # ---- the deterministic navigator (zero-LLM) -----------------------------
    def rank_nodes(self, query: str) -> List[Tuple[str, float, List[str]]]:
        """Score every node against the query. Returns (node_id, score, matched_terms).

        Transparent and traceable: title matches weigh most, then summary, then
        body. This is the 'reasoning' a human does scanning a table of contents.
        """
        q_terms = set(_tokens(query))
        scored: List[Tuple[str, float, List[str]]] = []
        for nid, n in self.nodes.items():
            title_t = set(_tokens(n.title))
            sum_t = set(_tokens(n.summary))
            body_counts = _count(_tokens(n.own_text))
            matched, score = [], 0.0
            for q in q_terms:
                hit = False
                if q in title_t:
                    score += 5.0; hit = True
                if q in sum_t:
                    score += 2.0; hit = True
                if q in body_counts:
                    score += min(body_counts[q], 3) * 1.0; hit = True
                if hit:
                    matched.append(q)
            if score > 0:
                # prefer leaf/specific sections over broad parents on ties
                score += 0.1 * n.level
                scored.append((nid, round(score, 2), matched))
        scored.sort(key=lambda x: x[1], reverse=True)
        return scored

    def select_nodes(self, query: str, k: int = 2) -> Tuple[List[str], str]:
        """Pick the top-k relevant nodes. Uses an LLM if available, else deterministic."""
        llm_pick = _llm_select(self, query, k)
        if llm_pick is not None:
            return llm_pick
        ranked = self.rank_nodes(query)[:k]
        if not ranked:
            return [], "No section matched the query terms."
        reasoning = "; ".join(
            f"{nid} ({self.nodes[nid].title}) matched {mt} score={sc}"
            for nid, sc, mt in ranked
        )
        return [nid for nid, _, _ in ranked], reasoning

    # ---- answering ----------------------------------------------------------
    def answer(self, query: str, log: ReceiptLog, k: int = 2) -> "AnswerResult":
        node_ids, reasoning = self.select_nodes(query, k)
        citations, evidence_parts, ranges = [], [], []
        for nid in node_ids:
            n = self.nodes[nid]
            citations.append({
                "node_id": nid,
                "title": n.title,
                "breadcrumb": self.breadcrumb(nid),
                "lines": [n.start_line, n.end_line],
            })
            evidence_parts.append(f"[{self.breadcrumb(nid)}]\n{n.own_text}")
            ranges.append([n.start_line, n.end_line])
        evidence_text = "\n\n".join(evidence_parts)

        answer_text = _llm_answer(query, evidence_text)
        if answer_text is None:
            # offline: the honest 'vectorless' answer is the verbatim section(s)
            answer_text = evidence_text if evidence_text else "No relevant section found."

        receipt = log.append(
            query=query,
            visited_node_ids=node_ids,
            fetched_ranges=ranges,
            evidence_text=evidence_text,
            answer=answer_text,
        )
        return AnswerResult(
            query=query, answer=answer_text, citations=citations,
            visited_node_ids=node_ids, fetched_ranges=ranges,
            evidence_text=evidence_text, reasoning=reasoning,
            receipt=receipt.to_dict(),
        )

    def verify_receipt(self, receipt: dict) -> bool:
        """Replay a receipt: re-read the logged ranges from THIS index and check
        the evidence hash still matches. Detects tampering with stored content."""
        parts = []
        for nid in receipt["visited_node_ids"]:
            if nid not in self.nodes:
                return False
            n = self.nodes[nid]
            parts.append(f"[{self.breadcrumb(nid)}]\n{n.own_text}")
        return hash_text("\n\n".join(parts)) == receipt["evidence_hash"]

    # ---- persistence --------------------------------------------------------
    def to_dict(self) -> dict:
        return {
            "doc_name": self.doc_name,
            "roots": self.roots,
            "nodes": {nid: n.to_dict() for nid, n in self.nodes.items()},
        }

    @classmethod
    def from_dict(cls, data: dict) -> "TreeIndex":
        nodes = {nid: Node(**nd) for nid, nd in data["nodes"].items()}
        return cls(data["doc_name"], nodes, data["roots"])


@dataclass
class AnswerResult:
    query: str
    answer: str
    citations: List[dict]
    visited_node_ids: List[str]
    fetched_ranges: List[List[int]]
    evidence_text: str
    reasoning: str
    receipt: dict


# ── helpers ──────────────────────────────────────────────────────────────────
def _count(tokens: List[str]) -> Dict[str, int]:
    out: Dict[str, int] = {}
    for t in tokens:
        out[t] = out.get(t, 0) + 1
    return out


def _first_sentence(text: str, limit: int = 160) -> str:
    body = " ".join(line for line in text.split("\n") if line.strip())
    if not body:
        return ""
    m = re.search(r"(.+?[.!?])(\s|$)", body)
    s = m.group(1) if m else body
    return s[:limit].strip()


# ── optional LLM hooks (used only when an API key is configured) ──────────────
def _llm_available() -> bool:
    return bool(os.getenv("OPENAI_API_KEY") or os.getenv("ANTHROPIC_API_KEY"))


def _llm_complete(prompt: str) -> Optional[str]:
    if not _llm_available():
        return None
    try:
        import litellm  # lazy import; optional dependency
        model = os.getenv("VALORI_TREE_RAG_MODEL", "gpt-4o-mini")
        resp = litellm.completion(
            model=model,
            messages=[{"role": "user", "content": prompt}],
            temperature=0,
        )
        return resp.choices[0].message.content
    except Exception:
        return None


def _llm_select(index: "TreeIndex", query: str, k: int) -> Optional[Tuple[List[str], str]]:
    import json
    prompt = (
        "You are navigating a document by its table of contents.\n"
        f"Question: {query}\n\n"
        f"Tree (node_id, title, summary):\n{json.dumps(index.structure_map(), indent=2)}\n\n"
        f"Return JSON only: {{\"thinking\": \"...\", \"node_ids\": [\"0007\"]}} "
        f"with at most {k} node_ids most likely to contain the answer."
    )
    raw = _llm_complete(prompt)
    if raw is None:
        return None
    try:
        import json as _j
        s = raw[raw.find("{"): raw.rfind("}") + 1]
        data = _j.loads(s)
        ids = [i for i in data.get("node_ids", []) if i in index.nodes][:k]
        return (ids, data.get("thinking", "")) if ids else None
    except Exception:
        return None


def _llm_answer(query: str, evidence_text: str) -> Optional[str]:
    if not evidence_text:
        return None
    prompt = (
        "Answer the question using ONLY the provided sections. Be concise and "
        "cite the section breadcrumb in brackets.\n\n"
        f"Question: {query}\n\nSections:\n{evidence_text}"
    )
    return _llm_complete(prompt)
