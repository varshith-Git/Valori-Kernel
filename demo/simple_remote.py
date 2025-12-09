
"""
Simple example of connecting to a remote Valori Node.
Requires values-node running on localhost:3000.
"""
from valori.protocol import ProtocolClient
import json
import sys

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
    try:
        pc = ProtocolClient(embed=dummy_embed, remote="http://127.0.0.1:3000")
        
        print("Upserting text...")
        res = pc.upsert_text("Hello remote world! This is Valori.")
        print("Upsert response:", json.dumps(res, indent=2))
        
        print("Searching...")
        hits = pc.search_text("Hello", k=3)
        print("Search hits:", json.dumps(hits, indent=2))
    except Exception as e:
        print(f"Error connecting to Valori Node: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()
