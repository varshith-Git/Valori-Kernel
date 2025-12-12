import sys
import random
from typing import List
import valori
from valori.protocol import ProtocolClient

def dummy_embed(text: str) -> List[float]:
    raise Exception("Embedder should NOT be called when vector is provided!")

def main():
    remote_url = "http://localhost:3000"
    print(f"--- Valori Pre-computed Embeddings Test ---")
    
    client = ProtocolClient(
        embed=dummy_embed, # Passing a broken embedder to prove it's not used
        remote=remote_url,
    )

    # 1. Prepare data
    my_text = "This is a pre-embedded text."
    my_vector = [0.5] * 16 # Distinctive vector
    
    print(f"[Action] Upserting text with explicit vector: {my_vector[:3]}...")
    
    try:
        # Upsert with explicit vector
        res = client.upsert_text(text=my_text, vector=my_vector)
        print(f"✅ Upsert Success! Response: {res}")
        rec_id = res['record_ids'][0]
    except Exception as e:
        print(f"❌ Upsert Failed: {e}")
        sys.exit(1)

    # 2. Search to verify
    print(f"\n[Action] Searching for vector: {my_vector[:3]}...")
    hits = client.search_vector(my_vector, k=1)
    
    if hits['results'] and hits['results'][0]['record_id'] == rec_id:
        print(f"🎉 Validation Passed: Found record {rec_id} with exact match.")
    else:
        print(f"❌ Verification Failed: Record not found or ID mismatch.")

if __name__ == "__main__":
    main()
