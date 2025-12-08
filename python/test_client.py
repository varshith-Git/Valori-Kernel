import sys
import os
import time
import requests

# Ensure local package is importable
sys.path.append(os.path.join(os.getcwd(), "python"))

from valori import Client

def test_client():
    client = Client() # Defaults to localhost:3000
    
    print("Checking connection...")
    try:
        # Simple health check or just try insert
        # We don't have /health, so just Insert
        pass
    except Exception:
        print("Could not connect. Is values-node running?")
        return

    print("Inserting records...")
    try:
        id1 = client.insert_record([1.0] + [0.0]*15)
        print(f"Inserted ID: {id1}")
        
        id2 = client.insert_record([0.0, 1.0] + [0.0]*14)
        print(f"Inserted ID: {id2}")
        
        print("Searching...")
        hits = client.search([1.0] + [0.0]*15, 2)
        print(f"Hits: {hits}")
        assert len(hits) >= 1
        assert hits[0]['id'] == id1
        
        print("Creating graph...")
        n1 = client.create_node(kind=0, record_id=id1)
        n2 = client.create_node(kind=0, record_id=id2)
        e1 = client.create_edge(from_id=n1, to_id=n2, kind=0)
        print(f"Created edge: {e1}")
        
        print("Client Test PASSED")
        
    except requests.exceptions.ConnectionError:
        print("Connection failed. Make sure valori-node is running on port 3000.")
    except Exception as e:
        print(f"Test failed: {e}")

if __name__ == "__main__":
    test_client()
