import sys
import os

# Ensure we can import the build
# This might need `pip install .` or `PYTHONPATH` manipulation if built in-place.
# For simplicity, we assume the user might run `python setup.py build_ext --inplace` or similar.

try:
    from valori.valori_ffi import ValoriEngine
except ImportError as e:
    import traceback
    traceback.print_exc()
    print(f"Could not import ValoriEngine: {e}")
    sys.exit(1)

import shutil

def test_integration():
    db_path = "/tmp/valori_test_db"
    if os.path.exists(db_path):
        shutil.rmtree(db_path)
    
    print(f"Initializing ValoriEngine at {db_path}...")
    engine = ValoriEngine(db_path)
    
    print("Inserting vector...")
    # Dim 384
    vec = [0.1] * 384
    # Insert returns auto-assigned ID
    assigned_id = engine.insert(vec)
    print(f"Assigned ID: {assigned_id}")
    assert assigned_id == 0 # First slot
    
    print("Searching...")
    results = engine.search(vec, 5)
    print(f"Results: {results}")
    
    assert len(results) >= 1
    # Check ID match (0)
    assert results[0][0] == 0
    print("SUCCESS: Insert and Search Verified.")

if __name__ == "__main__":
    test_integration()
