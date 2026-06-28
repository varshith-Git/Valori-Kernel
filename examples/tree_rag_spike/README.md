# tree_rag_spike — the original offline reference

This is the **dependency-free Python prototype** that the production Tree-RAG
feature was ported from. It runs fully offline (no server, no API key, no pip
installs — uses `blake3` if present, else stdlib `blake2b`):

```bash
python3 examples/tree_rag_spike/demo.py
```

You'll see a document tree, three questions answered with **section + line**
citations, a BLAKE3 receipt per answer, the receipt chain verifying, and a
deliberate edit being **caught** by replaying a receipt.

## This is reference only — the shipped feature is in Rust

The real, supported implementation lives in the node and is exposed over HTTP:

| Spike (here, Python) | Production |
|---|---|
| `tree_rag.py` `TreeIndex` | [`crates/valori-node/src/tree_rag.rs`](../../crates/valori-node/src/tree_rag.rs) |
| `receipt.py` `ReceiptLog` | `Receipt` + `verify_chain` in the same module |
| `demo.py` | `POST /v1/tree/{build,query,verify}` (standalone + cluster) |
| run offline | `client.tree_build()` / `tree_query()` / `tree_verify()` (Python SDK) |

The Rust port is deterministic (no optional LLM hook), stateless, and works
identically in standalone and cluster mode. See
[`docs/phases/phase-I5-tree-rag.md`](../../docs/phases/phase-I5-tree-rag.md).
