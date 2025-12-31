import sys
import os

# Ensure we can import the build
# This might need `pip install .` or `PYTHONPATH` manipulation if built in-place.
# For simplicity, we assume the user might run `python setup.py build_ext --inplace` or similar.

try:
    # Try top-level import (installed via maturin)
    from valori_ffi import ValoriEngine
except ImportError:
    try:
        # Try local submodule import (dev mode)
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
    # Insert returns auto-assigned ID. Tag 100 for this vector.
    assigned_id = engine.insert(vec, 100)
    print(f"Assigned ID: {assigned_id}")
    assert assigned_id == 0 # First slot
    
    print("Searching (Unfiltered)...")
    results = engine.search(vec, 5, None)
    print(f"Results (Unfiltered): {results}")
    
    assert len(results) >= 1
    # Check ID match (0)
    assert results[0][0] == 0
    print("SUCCESS: Insert and Search Verified.")

    print("Inserting vector with Tag 200...")
    vec2 = [0.2] * 384
    id2 = engine.insert(vec2, 200)
    assert id2 == 1
    
    print("Searching (Filter Tag=100)...")
    results_f1 = engine.search(vec, 5, 100)
    print(f"Results (Tag=100): {results_f1}")
    assert len(results_f1) == 1
    assert results_f1[0][0] == 0 # Should match ID 0
    
    print("Searching (Filter Tag=200)...")
    results_f2 = engine.search(vec, 5, 200)
    print(f"Results (Tag=200): {results_f2}")
    assert len(results_f2) == 1
    assert results_f2[0][0] == 1 # Should match ID 1 (even if vec is closer to vec, filter wins)
    
    print("Searching (Filter Tag=999)...")
    results_f3 = engine.search(vec, 5, 999)
    print(f"Results (Tag=999): {results_f3}")
    assert len(results_f3) == 0 # Should match nothing
    
    print("SUCCESS: Filtering Verified.")

if __name__ == "__main__":
    test_integration()
