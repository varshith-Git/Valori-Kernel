"""
Retrieval receipts — the Valori-differentiating primitive.

PageIndex gives you *explainable* retrieval (you see page/section numbers).
Valori's moat is to make retrieval *provable and replayable*: every answer
carries a BLAKE3-chained receipt recording exactly which nodes were visited,
which text ranges were read, and a hash of the evidence and the answer.

This mirrors the kernel's audit chain (`crates/valori-kernel/src/crypto/`):
each receipt seals the previous receipt's hash, so the whole retrieval history
is tamper-evident. `verify()` re-reads the logged ranges from the index and
recomputes the evidence hash — if a stored section was altered, the hashes no
longer match and the tampering is proven.

Hashing prefers the `blake3` package (same as the kernel); if it is not
installed it falls back to the stdlib `hashlib.blake2b`, so this module runs
with zero external dependencies.
"""
from __future__ import annotations

import time
from dataclasses import dataclass, field, asdict
from typing import List, Optional

try:
    import blake3 as _blake3

    def _h(data: bytes) -> str:
        return _blake3.blake3(data).hexdigest()

    HASH_NAME = "blake3"
except ImportError:  # pragma: no cover - environment dependent
    import hashlib

    def _h(data: bytes) -> str:
        return hashlib.blake2b(data, digest_size=32).hexdigest()

    HASH_NAME = "blake2b"


GENESIS = "0" * 64


def hash_text(text: str) -> str:
    """Stable content hash of a piece of text."""
    return _h(text.encode("utf-8"))


def _join(*parts: str) -> str:
    return _h("\x1f".join(parts).encode("utf-8"))


@dataclass
class Receipt:
    """One tamper-evident record of a single retrieval."""

    query: str
    query_hash: str
    visited_node_ids: List[str]
    fetched_ranges: List[List[int]]      # [[start_line, end_line], ...]
    evidence_hash: str                   # hash of the verbatim text that was read
    answer_hash: str
    prev_hash: str
    receipt_hash: str
    hash_algo: str = HASH_NAME
    timestamp: float = field(default_factory=lambda: round(time.time(), 3))

    def to_dict(self) -> dict:
        return asdict(self)


def make_receipt(
    *,
    query: str,
    visited_node_ids: List[str],
    fetched_ranges: List[List[int]],
    evidence_text: str,
    answer: str,
    prev_hash: str = GENESIS,
) -> Receipt:
    """Build a receipt sealing the previous receipt's hash (the chain link)."""
    query_hash = hash_text(query)
    evidence_hash = hash_text(evidence_text)
    answer_hash = hash_text(answer)
    ranges_str = ";".join(f"{a}-{b}" for a, b in fetched_ranges)
    receipt_hash = _join(
        prev_hash,
        query_hash,
        ",".join(visited_node_ids),
        ranges_str,
        evidence_hash,
        answer_hash,
    )
    return Receipt(
        query=query,
        query_hash=query_hash,
        visited_node_ids=visited_node_ids,
        fetched_ranges=fetched_ranges,
        evidence_hash=evidence_hash,
        answer_hash=answer_hash,
        prev_hash=prev_hash,
        receipt_hash=receipt_hash,
    )


class ReceiptLog:
    """An append-only, BLAKE3-chained log of retrieval receipts.

    This is the off-kernel shadow of Valori's `events.log`: in the real
    integration each receipt would become an audit entry committed through the
    kernel and folded into the running state hash.
    """

    def __init__(self, receipts: Optional[List[Receipt]] = None):
        self.receipts: List[Receipt] = list(receipts or [])

    @property
    def head(self) -> str:
        """Hash of the most recent receipt (or GENESIS if empty)."""
        return self.receipts[-1].receipt_hash if self.receipts else GENESIS

    def append(
        self,
        *,
        query: str,
        visited_node_ids: List[str],
        fetched_ranges: List[List[int]],
        evidence_text: str,
        answer: str,
    ) -> Receipt:
        r = make_receipt(
            query=query,
            visited_node_ids=visited_node_ids,
            fetched_ranges=fetched_ranges,
            evidence_text=evidence_text,
            answer=answer,
            prev_hash=self.head,
        )
        self.receipts.append(r)
        return r

    def verify_chain(self) -> bool:
        """Check that every link seals the one before it (no entries removed/reordered)."""
        prev = GENESIS
        for r in self.receipts:
            expected = _join(
                prev,
                r.query_hash,
                ",".join(r.visited_node_ids),
                ";".join(f"{a}-{b}" for a, b in r.fetched_ranges),
                r.evidence_hash,
                r.answer_hash,
            )
            if expected != r.receipt_hash or r.prev_hash != prev:
                return False
            prev = r.receipt_hash
        return True

    def to_dict(self) -> dict:
        return {"hash_algo": HASH_NAME, "receipts": [r.to_dict() for r in self.receipts]}

    @classmethod
    def from_dict(cls, data: dict) -> "ReceiptLog":
        return cls([Receipt(**r) for r in data.get("receipts", [])])
