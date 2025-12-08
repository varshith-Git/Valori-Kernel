"""
Small Valori demo:
 - loads a short text (or PDF if path ends with .pdf)
 - chunks -> embeds (deterministic dummy embed)
 - upserts text into ProtocolClient (local FFI by default)
 - runs semantic search
 - snapshots state to a file
 - restores snapshot into a fresh kernel and re-runs search to assert same results
Usage:
  # local FFI (must have built & installed valori_ffi via maturin)
  python demo/demo_run.py

  # remote node
  REMOTE=http://127.0.0.1:3000 python demo/demo_run.py
"""
import os
import sys
import json
import time
from typing import List

# ensure local python package is importable (adjust path if needed)
repo_root = os.path.dirname(os.path.dirname(__file__))
sys.path.insert(0, repo_root)

# Import your Python API
from python.valori.protocol import ProtocolClient

# Demo embedding: deterministic hash -> 16-dim vector in [-1,1]
def dummy_embed(text: str) -> List[float]:
    import hashlib
    h = hashlib.sha256(text.encode("utf-8")).hexdigest()
    out = []
    for i in range(16):
        byte = int(h[i*2:(i*2)+2], 16)
        # map 0..255 -> -1..1
        out.append((byte / 255.0) * 2.0 - 1.0)
    return out

# helper: load text or pdf via existing ingest API
from python.valori.ingest import load_text_from_file, chunk_text

SAMPLE_TEXT = "This is a tiny demo document. It contains a few sentences. Valori demo."

def main():
    # remote override from env
    remote = os.environ.get("REMOTE", None)
    print("Running demo. Remote:", remote or "local (FFI)")

    # create client
    proto = ProtocolClient(embed=dummy_embed, remote=remote)

    # 1) Upsert text
    text = SAMPLE_TEXT
    print("Upserting text:", text[:80])
    resp = proto.upsert_text(text, doc_id="demo-doc-1", chunk_size=64)
    print("Upsert response:", json.dumps(resp, indent=2))

    # choose first memory id to search for
    mem_ids = resp["memory_ids"]
    rec_ids = resp["record_ids"]
    print("Inserted memory ids:", mem_ids)

    # 2) Search by text (client embeds)
    query = "tiny demo"
    print("Search for:", query)
    search_res = proto.search_text(query, k=3)
    print("Search results:", json.dumps(search_res, indent=2))

    # 3) Snapshot the state
    snap_bytes = proto.snapshot()
    snap_path = os.path.join("demo", f"snapshot_{int(time.time())}.bin")
    with open(snap_path, "wb") as f:
        f.write(snap_bytes)
    print("Snapshot saved to", snap_path, "size:", len(snap_bytes))

    # 4) Restore into a fresh ProtocolClient (new kernel)
    #    For local FFI: we must instantiate a fresh ProtocolClient (it constructs a new MemoryClient which constructs a new kernel)
    #    Then restore snapshot into that kernel and run the same search to verify determinism
    fresh = ProtocolClient(embed=dummy_embed, remote=remote)
    print("Restoring snapshot into fresh client...")
    fresh.restore(snap_bytes)

    # 5) Re-run search and compare
    fresh_res = fresh.search_text(query, k=3)
    print("Fresh search results:", json.dumps(fresh_res, indent=2))

    # 6) Compare top-k memory ids from original vs restored
    orig_ids = [h["memory_id"] for h in search_res["results"]]
    new_ids = [h["memory_id"] for h in fresh_res["results"]]
    print("Orig top-k:", orig_ids)
    print("New  top-k:", new_ids)
    if orig_ids == new_ids:
        print("SUCCESS: restored search results match original.")
    else:
        print("WARN: results differ after restore. That's unexpected for determinism.")

if __name__ == "__main__":
    main()
