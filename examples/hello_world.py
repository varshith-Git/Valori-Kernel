"""
Valori hello-world — copy-paste runnable, no server, no API key.

    pip install valoricore
    python examples/hello_world.py

What this demonstrates:
  - insert two memories with deterministic fake embeddings
  - search for the closest one
  - print the BLAKE3 state hash

Run it twice — you get the exact same hash both times, on any machine.
That's the core Valori guarantee: bit-identical, cryptographically provable.
"""

import math
import os
import shutil
from valoricore import MemoryClient

DIM = 16
DB_PATH = "./hello_valori_db"

# Deterministic fake embedder — swap for a real one (SentenceTransformer,
# OpenAI, Ollama) once you've seen it work.
def embed(text: str) -> list:
    seed = sum(ord(c) for c in text)
    return [math.sin(seed + i * 0.3) for i in range(DIM)]

# Always start clean so the hash is reproducible.
if os.path.exists(DB_PATH):
    shutil.rmtree(DB_PATH)

db = MemoryClient(path=DB_PATH, dim=DIM)

db.add_document(text="Valori stores memories with cryptographic proof.", embed=embed)
db.add_document(text="Fixed-point math gives bit-identical results on every machine.", embed=embed)
db.add_document(text="BLAKE3 chains every event into an offline-verifiable audit log.", embed=embed)

query = "how does Valori prove it didn't lose data?"
hits  = db.semantic_search(query, embed=embed, k=2)

print(f"Query: {query!r}\n")
for i, h in enumerate(hits, 1):
    print(f"  {i}. score={h['score']:.4f}  — {h.get('metadata','')[:70]}")

state_hash = db.get_state_hash()
print(f"\nState hash: {state_hash}")
print("Run this again — you get the exact same hash.")
print("Run it on a different machine — same hash.")
print("That's the guarantee.\n")

# Cleanup
shutil.rmtree(DB_PATH)
