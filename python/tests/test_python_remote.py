import sys
import time
import requests

def test_remote():
    base_url = "http://127.0.0.1:3032"
    print(f"Testing Valori Node at {base_url}")
    
    # 1. Insert
    print("\nInserting vectors...")
    v1 = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
    resp = requests.post(f"{base_url}/records", json={"values": v1})
    print(f"Insert V1: {resp.status_code} - {resp.json()}")
    id1 = resp.json()["id"]

    v2 = [10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0]
    resp = requests.post(f"{base_url}/records", json={"values": v2})
    print(f"Insert V2: {resp.status_code} - {resp.json()}")
    id2 = resp.json()["id"]

    # 2. Search
    print("\nSearching...")
    query = [1.1, 2.1, 3.1, 4.1, 5.1, 6.1, 7.1, 8.1]
    resp = requests.post(f"{base_url}/search", json={"query": query, "k": 2})
    print(f"Search Query 1: {resp.status_code} - {resp.json()}")
    
    # 3. Memory Search (Document logic)
    print("\nMemory Upsert...")
    resp = requests.post(f"{base_url}/v1/memory/upsert_vector", json={
        "vector": [5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0],
        "metadata": {"title": "Test Document", "content": "Hello Valori"}
    })
    print(f"Memory Upsert: {resp.status_code} - {resp.json()}")
    
    print("\nMemory Search...")
    resp = requests.post(f"{base_url}/v1/memory/search_vector", json={
        "query_vector": [5.1, 5.1, 5.1, 5.1, 5.1, 5.1, 5.1, 5.1],
        "k": 1
    })
    print(f"Memory Search: {resp.status_code} - {resp.json()}")

    # 4. Proof
    print("\nGetting Proof...")
    resp = requests.get(f"{base_url}/v1/proof/state")
    print(f"Proof: {resp.status_code} - {resp.json()}")

if __name__ == "__main__":
    test_remote()
