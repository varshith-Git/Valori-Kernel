# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
Valori + Sentence Transformers Demo

This script demonstrates using the `SentenceTransformerAdapter` to seamlessly
integrate huggingface models with Valori.

Requirements:
    pip install sentence-transformers
    
Usage:
    Ensure Valori Node is running on localhost:3000
    python3 python/examples/demo_sentence_transformers.py
"""
import sys
import valori
from valori.protocol import ProtocolClient
from valori.adapters import SentenceTransformerAdapter

def main():
    print("--- Valori + Sentence Transformers Demo ---")
    
    # 1. Initialize Adapter
    # This downloads the model if not present (approx 80MB for all-MiniLM-L6-v2)
    try:
        print("Loading Model 'all-MiniLM-L6-v2'...")
        # Reduce 384-dim vector to 16-dim to fit default Valori Kernel
        adapter = SentenceTransformerAdapter("all-MiniLM-L6-v2", output_dim=16)
    except ImportError:
        print("❌ Error: sentence-transformers not installed.")
        print("Run: pip install sentence-transformers")
        sys.exit(1)
    except Exception as e:
        print(f"❌ Error loading model: {e}")
        sys.exit(1)

    # 2. Initialize Client
    try:
        client = ProtocolClient(
            embed=adapter.embed, # Pass the adapter's embed function
            remote="http://localhost:3000"
        )
        print("✅ Client initialized with SentenceTransformer backend.")
    except Exception as e:
        print(f"❌ Failed to init client: {e}")
        sys.exit(1)

    # 3. Upsert Text
    text = "The quick brown fox jumps over the lazy dog."
    print(f"\n[Action] Upserting text: '{text}'")
    try:
        # Note: Embedding happens client-side inside this call
        res = client.upsert_text(text)
        print(f"✅ Upsert Success! Response: {res}")
        rec_id = res['record_ids'][0]
    except Exception as e:
         # Valori default dim is 16. MiniLM is 384. 
         # This WILL fail if the server expects 16.
         # For this demo to work, the server must be started with DIM=384 or check handling.
        print(f"❌ Upsert Failed: {e}")
        print("Note: If error is 'Embedding mismatch', you need to restart Valori Node with VALORI_DIM=384")
        return

    # 4. Search
    query = "lazy animal"
    print(f"\n[Action] Searching for: '{query}'")
    hits = client.search_text(query, k=1)
    
    print(f"Results: {hits['results']}")
    if hits['results'] and hits['results'][0]['record_id'] == rec_id:
         print(f"🎉 Validation Passed: Found the fox!")
    else:
         print(f"⚠️ Search result mismatch.")

if __name__ == "__main__":
    main()
