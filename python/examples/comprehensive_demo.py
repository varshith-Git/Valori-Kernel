import os
import shutil
import sys

# Ensure we can import valoricore from the sibling directory
sys.path.append(os.path.join(os.path.dirname(__file__), ".."))

try:
    from valoricore import Valoricore, ingest_embedding, generate_proof
except ImportError as e:
    print(f"Error: {e}")
    print("Please make sure you have installed the package or are running from the correct directory.")
    sys.exit(1)

def run_demo():
    print("🛡️ Starting Valoricore Comprehensive Demo\n")
    
    # 1. Initialization
    # We'll use Local mode for this demo
    db_path = "./demo_valoricore_db"
    if os.path.exists(db_path):
        shutil.rmtree(db_path)
    
    # Note: Valoricore factory handles Local vs Remote
    db = Valoricore(path=db_path)
    print(f"✅ Initialized Local Engine at {db_path}")

    # 2. Basic Ingestion
    # Creating a 16-dimensional vector for demonstration
    vec_a = [0.1, 0.2, 0.3, 0.4] + [0.0]*12 
    rid_a = db.insert(vec_a, tag=101)
    print(f"📥 Inserted Vector A (ID: {rid_a}, Tag: 101)")

    # 3. Metadata Management
    # You can store binary blobs (like serialized JSON, strings, etc.)
    meta_content = b"Document: User Profile, Role: Admin"
    db.set_metadata(rid_a, meta_content)
    retrieved_meta = db.get_metadata(rid_a)
    print(f"📝 Metadata Set/Get: {retrieved_meta.decode()}")

    # 4. Batch Ingestion with Proofs
    # Valoricore is optimized for batching. Proofs are generated atomically.
    batch = [
        [0.5, 0.5, 0.5, 0.5] + [0.0]*12,
        [0.9, 0.9, 0.9, 0.9] + [0.0]*12
    ]
    results = db.insert_batch_with_proof(batch, tags=[202, 202])
    for rid, proof in results:
        print(f"🔐 Batch Insert: ID {rid} generated Cryptographic Proof: {proof[:16]}...")

    # 5. Knowledge Graph Primitives
    # Link Vector A to a higher-level Graph Node
    # Kind 5 = 'Document', Kind 6 = 'Chunk'
    node_id = db.create_node(kind=5, record_id=rid_a) 
    chunk_node = db.create_node(kind=6, record_id=results[0][0]) 
    
    # Create a directed relationship (Edge)
    # Kind 6 = 'ParentOf'
    db.create_edge(from_id=node_id, to_id=chunk_node, kind=6) 
    print(f"🕸️ Knowledge Graph: Linked Node {node_id} -> Node {chunk_node} via Edge (ParentOf)")

    # 6. Semantic Search with O(1) Tag Filtering
    print("\n🔍 Performing Semantic Search...")
    query = [0.1, 0.2, 0.3, 0.4] + [0.0]*12
    # k=2, filter_tag=101 means "find top 2 but only if they have tag 101"
    hits = db.search(query, k=2, filter_tag=101) 
    for hit in hits:
        print(f"   Match Found: ID {hit['id']}, Score: {hit['score']}")

    # 7. Integrity & Stats
    count = db.record_count()
    state_hash = db.get_state_hash()
    print(f"\n📊 Stats: {count} total records stored.")
    print(f"🔒 Global State Hash (Merkle Root): {state_hash}")

    # 8. Snapshot & Restore (Persistence)
    print("\n💾 Testing Snapshot/Restore...")
    snap_data = db.snapshot()
    print(f"   Snapshot generated ({len(snap_data)} bytes)")
    
    # Simulate a fresh engine restore to a different path
    restore_path = "./temp_restore_db"
    if os.path.exists(restore_path):
        shutil.rmtree(restore_path)
        
    db_restore = Valoricore(path=restore_path)
    db_restore.restore(snap_data)
    
    # Crucially, the State Hash must be identical post-restore
    assert db_restore.get_state_hash() == state_hash
    print("   ✅ Bit-exact recovery verified via state hash.")

    # 9. Deletion
    # Soft deletion marks the record as inactive in the pool and index
    db.delete(rid_a)
    print(f"\n🗑️ Deleted Record {rid_a}. New Count: {db.record_count()}")

    print("\n✨ Demo Complete!")
    
    # Cleanup demo folders
    shutil.rmtree(db_path)
    shutil.rmtree(restore_path)

if __name__ == "__main__": 
    run_demo()
