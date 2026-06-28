"""
Storage for the tree index + receipt log.

`LocalStore` (default) persists to JSON so the spike runs with no services.

`ValoriStore` documents — and partially sketches — how this maps onto a real
Valori node. It is intentionally not wired to a live node here (that is the
next phase); the docstring is the contract.
"""
from __future__ import annotations

import json
import os
from typing import Tuple

from tree_rag import TreeIndex
from receipt import ReceiptLog


class LocalStore:
    """Persist an index and its receipt log to a workspace directory as JSON."""

    def __init__(self, workspace: str):
        self.workspace = workspace
        os.makedirs(workspace, exist_ok=True)

    def _paths(self, doc_id: str) -> Tuple[str, str]:
        return (
            os.path.join(self.workspace, f"{doc_id}.index.json"),
            os.path.join(self.workspace, f"{doc_id}.receipts.json"),
        )

    def save(self, doc_id: str, index: TreeIndex, log: ReceiptLog) -> None:
        ip, rp = self._paths(doc_id)
        with open(ip, "w", encoding="utf-8") as f:
            json.dump(index.to_dict(), f, ensure_ascii=False, indent=2)
        with open(rp, "w", encoding="utf-8") as f:
            json.dump(log.to_dict(), f, ensure_ascii=False, indent=2)

    def load(self, doc_id: str) -> Tuple[TreeIndex, ReceiptLog]:
        ip, rp = self._paths(doc_id)
        with open(ip, encoding="utf-8") as f:
            index = TreeIndex.from_dict(json.load(f))
        log = ReceiptLog()
        if os.path.exists(rp):
            with open(rp, encoding="utf-8") as f:
                log = ReceiptLog.from_dict(json.load(f))
        return index, log


class ValoriStore:
    """Sketch of the real integration (next phase). Mapping:

        TreeIndex node      -> graph node           (POST /v1/graph or memory_upsert)
        node.own_text       -> Valori record(s)     (insert; addressable, audited)
        parent/child links  -> graph edges          (Contains edges)
        ReceiptLog          -> kernel audit chain   (each receipt -> events.log entry)

    Why off-kernel for now: the tree *builder* and the *reasoner* call an LLM, so
    by invariant #7 they cannot live in `valori-kernel` (no_std). Only the stored
    tree + receipts belong in the kernel, and that is the phase that follows this
    spike once the snapshot-bug fix + eval harness gates are cleared.
    """

    def __init__(self, base_url: str = "http://localhost:3000"):
        self.base_url = base_url

    def save(self, *args, **kwargs):  # pragma: no cover - intentionally unbuilt
        raise NotImplementedError(
            "ValoriStore is the next phase. Use LocalStore for the spike. "
            "See the class docstring for the kernel mapping."
        )
