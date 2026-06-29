import pytest
from valoricore import Valoricore, MemoryClient
import requests
import time
import random

pytestmark = pytest.mark.integration

def test_scale_10k():
    base_url = "http://127.0.0.1:3033"
    print(f"Testing Scale (10,000 records) at {base_url}")
    
    dim = 16
    total_records = 10000
    batch_size = 500
    
    # 1. Batch Insert 10k records
    print(f"\nIngesting {total_records} records in batches of {batch_size}...")
    start_time = time.time()
    
    for i in range(0, total_records, batch_size):
        batch = [[random.uniform(-1, 1) for _ in range(dim)] for _ in range(batch_size)]
        resp = requests.post(f"{base_url}/v1/vectors/batch_insert", json={"batch": batch})
        if resp.status_code != 200:
            print(f"Error at batch {i}: {resp.status_code} - {resp.text}")
            return
        if (i + batch_size) % 1000 == 0:
            print(f"  ... ingested {i + batch_size} records")
            
    ingest_duration = time.time() - start_time
    print(f"Ingestion complete in {ingest_duration:.2f} seconds.")
    print(f"Throughput: {total_records / ingest_duration:.2f} vectors/sec")

    # 2. Verify Count
    resp = requests.get(f"{base_url}/v1/replication/state")
    # Actually, let's use the health check or metrics for count if available
    # For now, let's just search
    
    # 3. Search
    print("\nPerforming Search across 10,000 records...")
    query = [0.0] * dim
    start_time = time.time()
    resp = requests.post(f"{base_url}/search", json={"query": query, "k": 5})
    search_duration = time.time() - start_time
    
    if resp.status_code == 200:
        results = resp.json()["results"]
        print(f"Search complete in {search_duration:.4f} seconds.")
        print(f"Top 5 results: {results}")
        
        # 4. Delete Test
        if len(results) > 0:
            target_id = results[0]["id"]
            print(f"\nDeleting ID {target_id}...")
            del_resp = requests.post(f"{base_url}/v1/delete", json={"id": target_id})
            print(f"  Delete status: {del_resp.status_code}")
            
            print(f"Verifying deletion of {target_id}...")
            search_resp = requests.post(f"{base_url}/search", json={"query": query, "k": 5})
            new_results = search_resp.json()["results"]
            found = any(r["id"] == target_id for r in new_results)
            if not found:
                print(f"  ✅ ID {target_id} is no longer in search results.")
            else:
                print(f"  ❌ ID {target_id} still found!")

    # 5. Upsert (Update) Test via Memory API
    print("\nTesting Upsert (Update) via Memory API...")
    mem_vec = [0.5] * dim
    mem_resp = requests.post(f"{base_url}/v1/memory/upsert_vector", json={
        "vector": mem_vec,
        "metadata": {"title": "Scale Test Doc", "version": 1}
    })
    if mem_resp.status_code != 200:
        print(f"  ❌ Upsert failed: {mem_resp.status_code} - {mem_resp.text}")
        return
    mem_data = mem_resp.json()
    print(f"  Upserted Memory ID: {mem_data['memory_id']}")
    
    print("Updating the same Memory ID with new data...")
    # In Valori, you can update metadata or replace the vector for a semantic entity
    update_resp = requests.post(f"{base_url}/v1/memory/meta/set", json={
        "target_id": mem_data['memory_id'],
        "metadata": {"title": "Scale Test Doc", "version": 2, "status": "updated"}
    })
    print(f"  Update Status: {update_resp.status_code}")
    
    print("Verifying Update...")
    get_resp = requests.get(f"{base_url}/v1/memory/meta/get?target_id={mem_data['memory_id']}")
    meta = get_resp.json()
    metadata_val = meta.get("metadata", {})
    if metadata_val and metadata_val.get("version") == 2:
        print(f"  ✅ Metadata updated successfully: {metadata_val}")
    else:
        print(f"  ❌ Metadata update failed: {meta}")

    # 6. Final Proof
    print("\nFinal State Hash:")
    resp = requests.get(f"{base_url}/v1/proof/state")
    if resp.status_code == 200:
        proof = resp.json()
        hash_hex = "".join([f"{b:02x}" for b in proof["final_state_hash"]])
        print(f"  {hash_hex}")
    else:
        print(f"  Proof failed: {resp.status_code}")

if __name__ == "__main__":
    test_scale_10k()
