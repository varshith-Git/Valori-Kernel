import requests
import time
import random

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
    else:
        print(f"Search failed: {resp.status_code} - {resp.text}")

    # 4. Proof
    print("\nFinal State Hash:")
    resp = requests.get(f"{base_url}/v1/proof/state")
    if resp.status_code == 200:
        proof = resp.json()
        # final_state_hash is a list of 32 ints
        hash_hex = "".join([f"{b:02x}" for b in proof["final_state_hash"]])
        print(f"  {hash_hex}")
    else:
        print(f"  Proof failed: {resp.status_code}")

if __name__ == "__main__":
    test_scale_10k()
