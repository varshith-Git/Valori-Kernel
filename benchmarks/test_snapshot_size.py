import time
import os
import sys
import tempfile
import math

# Ensure local valoricore is in path
sys.path.insert(0, os.path.abspath("python"))
try:
    from valoricore import LocalClient
except ImportError:
    print("Please build python package first: maturin develop")
    sys.exit(1)

def vec(seed: int, dim: int) -> list:
    return [math.sin(seed * 1.7 + i * 0.9) for i in range(dim)]

def test_snapshot_at_target_mb(target_mb, dim=768):
    print(f"\n=======================================================")
    print(f" 🧪 Testing ~{target_mb} MB Snapshot (Dimension: {dim})")
    print(f"=======================================================")
    
    # 768 floats * 4 bytes = 3072 bytes per vector (~3.1 KB)
    # Estimate records needed for target_mb
    bytes_per_rec = dim * 4 + 100 # vector + overhead
    target_bytes = target_mb * 1024 * 1024
    num_records = int(target_bytes / bytes_per_rec)
    
    print(f"[{target_mb} MB Target] Creating fresh collection and inserting {num_records:,} vectors...")
    t0 = time.perf_counter()
    
    # Create local client
    tmp_path = tempfile.mkdtemp(prefix=f"val_snap_{target_mb}mb_")
    c = LocalClient(
        path=tmp_path,
        dim=dim,
        index_kind="bruteforce",
        max_records=num_records + 1000
    )
    
    # Generate in batches of 5000 for high speed
    batch_size = 5000
    for start in range(0, num_records, batch_size):
        end = min(start + batch_size, num_records)
        batch = [vec(i, dim) for i in range(start, end)]
        c.insert_batch(batch)
        
    insert_time = time.perf_counter() - t0
    print(f"✅ Inserted {num_records:,} records in {insert_time:.2f} s ({int(num_records/insert_time):,} rec/s)")
    
    # 1. Test snapshot() - serialize to memory
    print(f"\n--- 1. Serializing Snapshot to Memory ---")
    t0 = time.perf_counter()
    snap_bytes = c.snapshot()
    mem_snap_ms = (time.perf_counter() - t0) * 1000.0
    actual_mb = len(snap_bytes) / (1024 * 1024)
    print(f"📦 Serialized size: {actual_mb:.2f} MB")
    print(f"⏱️  Time for c.snapshot() (serialize to RAM): {mem_snap_ms:.2f} ms")
    
    # 2. Save to disk
    snap_filename = f"test_{target_mb}mb.snap"
    with open(snap_filename, "wb") as f:
        f.write(snap_bytes)
    disk_mb = os.path.getsize(snap_filename) / (1024 * 1024)
    print(f"💾 Written to disk: {snap_filename} ({disk_mb:.2f} MB)")
    
    # 3. Test DISK READ speed (Reading file from SSD into RAM)
    print(f"\n--- 2. Reading {disk_mb:.2f} MB Snapshot File from Disk ---")
    t0 = time.perf_counter()
    with open(snap_filename, "rb") as f:
        loaded_bytes = f.read()
    disk_read_ms = (time.perf_counter() - t0) * 1000.0
    print(f"⏱️  Time to read {disk_mb:.2f} MB from disk into RAM: {disk_read_ms:.2f} ms ({disk_mb / (disk_read_ms/1000.0):.1f} MB/s)")
    
    # 4. Test RESTORE speed (Restoring from memory bytes into Valori engine)
    print(f"\n--- 3. Restoring Snapshot into Valori Engine ---")
    tmp_path_restored = tempfile.mkdtemp(prefix=f"val_snap_restored_{target_mb}mb_")
    c_new = LocalClient(
        path=tmp_path_restored,
        dim=dim,
        index_kind="bruteforce",
        max_records=num_records + 1000
    )
    t0 = time.perf_counter()
    c_new.restore(loaded_bytes)
    restore_ms = (time.perf_counter() - t0) * 1000.0
    print(f"⏱️  Time for engine.restore() (rebuilding index in RAM): {restore_ms:.2f} ms")
    
    # 5. Total Cold Boot Time (Disk Read + Engine Restore)
    total_boot_ms = disk_read_ms + restore_ms
    print(f"\n🚀 TOTAL COLD BOOT TIME (Disk Read + Engine Restore): {total_boot_ms:.2f} ms ({total_boot_ms/1000.0:.3f} seconds)")
    
    # Cleanup
    if os.path.exists(snap_filename):
        os.remove(snap_filename)

if __name__ == "__main__":
    test_snapshot_at_target_mb(50)
    test_snapshot_at_target_mb(100)
