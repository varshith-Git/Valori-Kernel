import sys
import os

# Ensure the current directory is in sys.path to load the module
sys.path.append(os.getcwd())

try:
    import valori_ffi
except ImportError:
    # Try looking in target/debug for convenience during dev
    print("Could not import valori_ffi directly. Checking target/debug...")
    sys.path.append(os.path.join(os.getcwd(), "target", "debug"))
    try:
        import valori_ffi
    except ImportError as e:
        print(f"Failed to import valori_ffi: {e}")
        print("Make sure valori_ffi.pyd (Windows) or .so (Linux/Mac) is available.")
        sys.exit(1)

print(f"Successfully imported valori_ffi: {valori_ffi}")

def test_kernel():
    print("Creating PyKernel...")
    k = valori_ffi.PyKernel()
    
    print("Inserting records...")
    id0 = k.insert([1.0] + [0.0]*15)
    print(f"Inserted ID: {id0}")
    assert id0 == 0
    
    id1 = k.insert([0.0, 1.0] + [0.0]*14)
    print(f"Inserted ID: {id1}")
    assert id1 == 1
    
    print("Searching...")
    hits = k.search([1.0] + [0.0]*15, 2)
    print(f"Hits: {hits}")
    assert len(hits) == 2
    assert hits[0][0] == 0  # ID 0 should be top
    
    print("Snapshotting...")
    snap = k.snapshot()
    print(f"Snapshot size: {len(snap)} bytes")
    
    print("Restoring to new kernel...")
    k2 = valori_ffi.PyKernel()
    k2.restore(snap)
    
    hits2 = k2.search([1.0] + [0.0]*15, 2)
    print(f"Hits2: {hits2}")
    assert hits == hits2
    
    print("ALL TESTS PASSED")

if __name__ == "__main__":
    test_kernel()
