import sys
import os
import shutil

# Ensure the current directory is in sys.path to load the module
sys.path.append(os.getcwd())

try:
    import valoricore_ffi
except ImportError:
    # Try looking in target/debug for convenience during dev
    print("Could not import valoricore_ffi directly. Checking target/debug...")
    sys.path.append(os.path.join(os.getcwd(), "..", "target", "debug"))
    try:
        import valoricore_ffi
    except ImportError as e:
        print(f"Failed to import valoricore_ffi: {e}")
        print("Make sure valoricore_ffi.pyd (Windows) or .so (Linux/Mac) is available.")
        sys.exit(1)

print(f"Successfully imported valoricore_ffi: {valoricore_ffi}")

def test_kernel():
    db_path = "data/test_valori_db"
    if os.path.exists(db_path):
        shutil.rmtree(db_path)
    
    print(f"Creating ValoricoreEngine at {db_path}...")
    # The new engine requires a path for the event logs
    k = valoricore_ffi.ValoricoreEngine(db_path)
    
    print("Inserting records with proofs...")
    # insert_with_proof returns (id, proof_hex)
    id0, p0 = k.insert_with_proof([1.0] + [0.0]*14, 100)
    print(f"Inserted ID 0 with proof: {p0}")
    assert id0 == 0
    
    id1, p1 = k.insert_with_proof([0.0, 1.0] + [0.0]*13, 200)
    print(f"Inserted ID 1 with proof: {p1}")
    assert id1 == 1
    
    print("Searching...")
    # Search returns (id, score)
    hits = k.search([1.0] + [0.0]*14, 2)
    print(f"Hits: {hits}")
    assert len(hits) == 2
    assert hits[0][0] == 0  # ID 0 should be top
    
    print("Snapshotting...")
    snap = k.snapshot()
    print(f"Snapshot size: {len(snap)} bytes")
    
    print("Restoring to new engine...")
    k2 = valoricore_ffi.ValoricoreEngine("data/test_valori_db_restore")
    k2.restore(snap)
    
    hits2 = k2.search([1.0] + [0.0]*14, 2)
    print(f"Hits2: {hits2}")
    assert hits == hits2
    
    print("Verifying cryptographic determinism...")
    # Verify that the standalone verify function works with our new proofs
    is_valid = valoricore_ffi.verify_embedding([1.0] + [0.0]*14, p0)
    print(f"Integrity check for Record 0: {is_valid}")
    assert is_valid == True

    print("ALL TESTS PASSED")

if __name__ == "__main__":
    test_kernel()
