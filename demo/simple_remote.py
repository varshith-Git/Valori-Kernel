
"""
Simple example of connecting to a remote Valori Node.
Requires values-node running on localhost:3000.
"""
from python.valori.protocol import ProtocolClient
import json

def dummy_embed(text: str) -> list[float]:
    # Dummy deterministic embedding (16-dim)
    import hashlib
    h = hashlib.sha256(text.encode("utf-8")).hexdigest()
    out = []
    for i in range(16):
        byte = int(h[i*2:(i*2)+2], 16)
        out.append((byte / 255.0) * 2.0 - 1.0)
    return out

def main():
    print("Connecting to http://127.0.0.1:3000...")
    pc = ProtocolClient(embed=dummy_embed, remote="http://127.0.0.1:3000")
    
    print("Upserting text...")
    res = pc.upsert_text("Hello remote world! This is Valori.")
    print("Upsert response:", json.dumps(res, indent=2))
    
    print("Searching...")
    hits = pc.search_text("Hello", k=3)
    print("Search hits:", json.dumps(hits, indent=2))

if __name__ == "__main__":
    main()
