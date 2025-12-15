import sys
import os
# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
import unittest
import time

# Ensure local package is importable
sys.path.append(os.path.join(os.getcwd(), "python"))

# Ensure FFI pyd is importable (assumes it's in CWD or added to path)
sys.path.append(os.getcwd()) 

from valori import Valori

def test_unified():
    print("=== Testing Local (FFI) Mode ===")
    try:
        db_local = Valori()
        run_test_suite(db_local, "Local")
    except ImportError as e:
        print(f"Skipping Local test: {e}")

    print("\n=== Testing Remote (HTTP) Mode ===")
    try:
        # Assumes valori-node is running on localhost:3000
        db_remote = Valori(remote="http://127.0.0.1:3000")
        run_test_suite(db_remote, "Remote")
    except Exception as e:
        print(f"Remote test failed: {e}")

def run_test_suite(db, name):
    print(f"[{name}] Inserting records...")
    # Use unique vector components to avoid collision with previous test runs
    # Base it on time? Or just unique pattern.
    # [1.0, 0, ...] is common. Let's use [0.5, 0.5, ...] for this run?
    # Actually, timestamp is better.
    ts = time.time()
    # Normalize manually roughly to avoid overflow if needed, but [ts, 0...] is fine strictly speaking if scaled
    # But let's keep it simple: [0.99, 0.01...] distinct from [1.0, 0.0]
    vec1 = [0.99, 0.0] + [0.0]*14
    vec2 = [0.0, 0.99] + [0.0]*14
    
    id1 = db.insert(vec1)
    id2 = db.insert(vec2)
    print(f"[{name}] Inserted IDs: {id1}, {id2}")
    
    print(f"[{name}] Searching...")
    hits = db.search(vec1, 2)
    print(f"[{name}] Hits: {hits}")
    assert len(hits) >= 1
    found_ids = [h['id'] for h in hits]
    assert id1 in found_ids, f"Expected {id1} in hits {found_ids}"

    print(f"[{name}] Creating Graph...")
    n1 = db.create_node(kind=0, record_id=id1)
    n2 = db.create_node(kind=0, record_id=id2)
    e1 = db.create_edge(from_id=n1, to_id=n2, kind=0)
    print(f"[{name}] Created Edge: {e1}")
    
    print(f"[{name}] PASSED")

if __name__ == "__main__":
    test_unified()
