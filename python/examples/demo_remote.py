"""
Valori Remote Mode Verification Script

This script attempts to connect to a running Valori node at http://localhost:3000.
It demonstrates:
1. Connecting via ProtocolClient (the higher-level SDK).
2. Inserting a vector.
3. Searching for a vector.

Usage:
1. Open Terminal 1: Run the Valori server
   $ cd node
   $ cargo run --release

2. Open Terminal 2: Run this script
   $ python3 python/examples/demo_remote.py
"""

# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import sys
import random
import time
from typing import List

# Ensure we can import `valori` from the local checkout if installed or in pythonpath
# You may need to run `pip install .` in the `python/` folder first.
import valori
from valori.protocol import ProtocolClient

def dummy_embed(text: str) -> List[float]:
    """Generates a random deterministic 16-dim vector for testing."""
    # Seed with length to be deterministic per string
    random.seed(len(text)) 
    return [random.uniform(-0.5, 0.5) for _ in range(16)]

def main():
    remote_url = "http://localhost:3000"
    print(f"--- Valori Remote Mode Test ---")
    print(f"Target: {remote_url}")
    
    # 1. Initialize Client
    try:
        client = ProtocolClient(
            embed=dummy_embed,
            remote=remote_url,
            # api_key="test-key" # Uncomment if you enabled auth
        )
        print("✅ Client initialized.")
    except Exception as e:
        print(f"❌ Failed to init client: {e}")
        sys.exit(1)

    # 2. Upsert a test vector
    vector = [0.1] * 16
    print(f"\n[Action] Upserting vector: {vector[:3]}...")
    try:
        res = client.upsert_vector(vector)
        print(f"✅ Upsert Success! Response: {res}")
        rec_id = res['record_id']
    except Exception as e:
        print(f"❌ Upsert Failed: {e}")
        print("Check if the server is running on port 3000.")
        sys.exit(1)

    # 3. Search
    print(f"\n[Action] Searching for vector: {vector[:3]}...")
    try:
        hits = client.search_vector(vector, k=3)
        print(f"✅ Search Success!")
        print(f"Results: {hits['results']}")
        
        # Verify
        found = any(h['record_id'] == rec_id for h in hits['results'])
        if found:
            print("🎉 Validation Passed: Inserted record was found.")
        else:
            print("⚠️ Warning: Inserted record not in top 3 results.")
    except Exception as e:
        print(f"❌ Search Failed: {e}")
        sys.exit(1)

    print("\n--- Test Complete ---")

if __name__ == "__main__":
    main()
