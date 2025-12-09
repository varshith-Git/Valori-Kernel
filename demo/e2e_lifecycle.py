import subprocess
import time
import os
import shutil
import sys
import requests
from valori import ProtocolClient as Client

# Config
# Assumes running from project root
SERVER_BIN = "./target/debug/valori-node"
SNAPSHOT_PATH = "./valori_e2e.snapshot"
PORT = 3334
HOST = f"http://127.0.0.1:{PORT}"

def build_server():
    print("Building server...")
    subprocess.run(["cargo", "build", "-p", "valori-node"], check=True)

def start_server(env=None):
    print(f"Starting server on port {PORT}...")
    my_env = os.environ.copy()
    my_env["VALORI_BIND"] = f"127.0.0.1:{PORT}"
    my_env["VALORI_SNAPSHOT_PATH"] = SNAPSHOT_PATH
    my_env["VALORI_SNAPSHOT_INTERVAL"] = "1" # Fast auto-snapshot
    my_env["RUST_LOG"] = "info"
    if env:
        my_env.update(env)
        
    proc = subprocess.Popen([SERVER_BIN], env=my_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    
    # Wait for health
    print("Waiting for server health...")
    for _ in range(20):
        try:
            # We use a raw requests check because Client might error on connect
            # Use search endpoint as health check (requires no params if empty?) 
            # Actually MemorySearchVectorRequest needs params.
            # Using /v1/memory/meta/get with a dummy ID is safer/cheaper?
            # Or just /search with empty query?
            # Let's try /v1/memory/meta/get
            requests.get(f"{HOST}/v1/memory/meta/get?target_id=healthcheck", timeout=1)
            print("Server is up!")
            return proc
        except Exception as e:
            time.sleep(0.5)
            
    # Failed
    print("Server failed to start. Stderr:")
    print(proc.stderr.read().decode())
    proc.kill()
    raise RuntimeError("Server failed to start")

def main():
    # Cleanup
    if os.path.exists(SNAPSHOT_PATH):
        os.remove(SNAPSHOT_PATH)
    if os.path.exists(SNAPSHOT_PATH + ".tmp"):
        os.remove(SNAPSHOT_PATH + ".tmp")

    try:
        build_server()
    except Exception as e:
        print(f"Build failed: {e}")
        return

    # 1. Start Fresh
    p1 = start_server()
    doc_node_id = None
    
    try:
        # Dummy embedder for consistency
        embedding = [0.1] * 16
        def mock_embed(text):
            return embedding

        client = Client(remote=HOST, embed=mock_embed)
        
        print("\n--- Step 1: Upserting Data ---")
        # Text upsert
        res = client.upsert_text("Hello Integration World!", metadata={"test_run": 1})
        doc_node_id = res["document_node_id"]
        print(f"Inserted Document Node: {doc_node_id}")
        
        # Verify Metadata immediately
        meta = client.get_metadata(f"node:{doc_node_id}")
        print(f"Retrieved Metadata: {meta}")
        if meta.get("test_run") != 1:
            raise ValueError(f"Metadata mismatch immediate: {meta}")
            
        print("\n--- Step 2: Waiting for Auto-Snapshot ---")
        # We set interval to 1s, so we wait 2.5s to be sure
        time.sleep(2.5)
        
    finally:
        print("Stopping Server 1...")
        p1.terminate()
        p1.wait()

    print("Server 1 Stopped. Checking Snapshot file...")
    if not os.path.exists(SNAPSHOT_PATH):
        raise ValueError("Snapshot file not found!")
    print(f"Snapshot found! Size: {os.path.getsize(SNAPSHOT_PATH)} bytes")
        
    # 2. Restart and Verify
    print("\n--- Step 3: Restarting Server (Restore) ---")
    p2 = start_server()
    
    try:
        def mock_embed(text):
            return [0.1] * 16
            
        client = Client(remote=HOST, embed=mock_embed)
        
        print("\n--- Step 4: Searching Data ---")
        # The index should have been rebuilt from the snapshot
        hits = client.search_text("Hello World", k=1)
        
        if not hits["results"]:
             raise ValueError("No hits found! Index persistence failed.")
        else:
             print(f"Found {len(hits['results'])} hits. Top score: {hits['results'][0]['score']}")
             
        print("\n--- Step 5: Verifying Metadata Persistence ---")
        if doc_node_id is None:
             raise ValueError("Lost Doc Node ID from Step 1 state. Script logic error.")
             
        meta = client.get_metadata(f"node:{doc_node_id}")
        print(f"Retrieved Metadata: {meta}")
        if not meta or meta.get("test_run") != 1:
            raise ValueError(f"Metadata persistence failed. Expected {{'test_run': 1}}, got {meta}")
            
        print("\n>>> SUCCESS: Full Integration Test Passed! <<<")
        
    except Exception as e:
        print(f"\n!!! Test Failed: {e} !!!")
        raise e
    finally:
        print("Stopping Server 2...")
        p2.terminate()
        p2.wait()
        # Cleanup
        if os.path.exists(SNAPSHOT_PATH):
            os.remove(SNAPSHOT_PATH)

if __name__ == "__main__":
    main()
