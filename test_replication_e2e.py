import requests
import subprocess
import time
import json
import os
import signal
import sys

# Configuration
NODE_BIN = "./target/debug/valori-node"
VERIFY_BIN = "./target/debug/valori-verify"
PORT = 3000
BASE_URL = f"http://localhost:{PORT}"
WAL_PATH = "node.wal"
SNAPSHOT_PATH = "node.snapshot"

def check_bins():
    if not os.path.exists(NODE_BIN):
        print(f"Node binary not found at {NODE_BIN}. Run `cargo build -p valori-node`")
        sys.exit(1)
    if not os.path.exists(VERIFY_BIN):
        print(f"Verify binary not found at {VERIFY_BIN}. Run `cargo build -p valori-verify`")
        sys.exit(1)

def run_node():
    print(f"Starting Node on port {PORT}...")
    # Clean previous state
    if os.path.exists(WAL_PATH): os.remove(WAL_PATH)
    if os.path.exists(SNAPSHOT_PATH): os.remove(SNAPSHOT_PATH)

    env = os.environ.copy()
    env["RUST_LOG"] = "info"
    
    # Configure Node via CLI or Config?
    # Node usually reads config.toml. Or Environment.
    # We rely on defaults or pass config file.
    # Assuming standard config.toml is used if present, or defaults.
    # Need to ensure WAL is enabled.
    # Default node might NOT have wal_path set.
    # We should create a temp config.toml
    
    # Configure Node via Environment Variables
    env["VALORI_BIND"] = f"127.0.0.1:{PORT}"
    env["VALORI_MAX_RECORDS"] = "1024"
    env["VALORI_DIM"] = "16"
    env["VALORI_MAX_NODES"] = "1024"
    env["VALORI_MAX_EDGES"] = "2048"
    env["VALORI_WAL_PATH"] = WAL_PATH
    env["VALORI_SNAPSHOT_PATH"] = SNAPSHOT_PATH
    env["VALORI_INDEX"] = "BruteForce"
    env["VALORI_QUANT"] = "None"
    
    # Redirect output to file to avoid blocking PIPE and capture log
    log_file = open("node.log", "w")
    proc = subprocess.Popen([NODE_BIN], env=env, stdout=log_file, stderr=subprocess.STDOUT)
    
    # Wait for startup
    attempts = 0
    while attempts < 10:
        try:
            requests.get(f"{BASE_URL}/v1/proof/state", timeout=1)
            print("Node is up.")
            return proc, log_file
        except:
            time.sleep(0.5)
            attempts += 1
            
    print("Node failed to start. Check node.log:")
    log_file.flush()
    with open("node.log", "r") as f:
        print(f.read())
    sys.exit(1)

def main():
    check_bins()
    
    # Clean logs
    if os.path.exists("node.log"): os.remove("node.log")

    node_proc, log_file = run_node()
    
    try:
        # 1. Save & Download EMPTY Snapshot First
        print("Saving Empty Snapshot...")
        resp = requests.post(f"{BASE_URL}/v1/snapshot/save", json={"path": SNAPSHOT_PATH})
        assert resp.status_code == 200, f"Snapshot save failed: {resp.text}"
        
        print("Downloading Empty Snapshot...")
        resp = requests.get(f"{BASE_URL}/v1/snapshot/download")
        assert resp.status_code == 200
        with open("downloaded.snap", "wb") as f:
            f.write(resp.content)
            
        # 2. Insert Data (Generate WAL)
        print("Inserting records...")
        records = []
        for i in range(10):
            vec = [float(i)] * 16
            resp = requests.post(f"{BASE_URL}/records", json={"values": vec})
            assert resp.status_code == 200, f"Insert failed: {resp.text}"
            records.append(i)
            
        # 3. Get Node Proof
        print("Fetching Node Proof associated with current state...")
        resp = requests.get(f"{BASE_URL}/v1/proof/state")
        assert resp.status_code == 200, f"Proof fetch failed"
        node_proof = resp.json()
        print(f"Node Proof: {json.dumps(node_proof, indent=2)}")
        
        # 4. Download WAL
        print("Downloading WAL...")
        resp = requests.get(f"{BASE_URL}/v1/replication/wal")
        assert resp.status_code == 200, f"WAL Download Failed: {resp.status_code} {resp.text}"
        with open("downloaded.wal", "wb") as f:
            f.write(resp.content)
            
        print(f"WAL Size: {os.path.getsize('downloaded.wal')} bytes")
        
        # 5. Run Verifier
        print("Running Verifier via Cargo...")
        cmd = [VERIFY_BIN, "downloaded.snap", "downloaded.wal"]
        verify_proc = subprocess.run(cmd, capture_output=True, text=True)
        
        if verify_proc.returncode != 0:
            print("Verifier FAILED:")
            print(verify_proc.stderr)
            sys.exit(1)
            
        verifier_proof = json.loads(verify_proc.stdout)
        print(f"Verifier Proof: {json.dumps(verifier_proof, indent=2)}")
        
        # 6. Compare Hash
        n_hash = node_proof["final_state_hash"]
        v_hash = verifier_proof["final_state_hash"]
        
        if n_hash == v_hash:
            print("\nSUCCESS: Proofs Match!")
            print(f"State Hash: {n_hash}")
        else:
            print("\nFAILURE: Proofs Mismatch!")
            print(f"Node: {n_hash}")
            print(f"Verify: {v_hash}")
            sys.exit(1)
            
        n_wal = node_proof["wal_hash"]
        v_wal = verifier_proof["wal_hash"]
        if n_wal == v_wal:
            print("SUCCESS: WAL Hash Match!")
            print(f"WAL Hash: {n_wal}")
        else:
             print("FAILURE: WAL Hash Mismatch!")
             print(f"Node: {n_wal}")
             print(f"Verify: {v_wal}")
             sys.exit(1)
        
    except Exception as e:
        print(f"Test failed: {e}")
        # Print Node Log on failure
        print("\n--- Node Log ---")
        log_file.flush()
        with open("node.log", "r") as f:
            print(f.read())
        print("----------------")
        
    finally:
        node_proc.terminate()
        log_file.close()
        try:
             os.remove("e2e_config.toml")
             os.remove("downloaded.snap")
             os.remove("downloaded.wal")
             if os.path.exists(WAL_PATH): os.remove(WAL_PATH)
             if os.path.exists(SNAPSHOT_PATH): os.remove(SNAPSHOT_PATH)
        except: pass
        
if __name__ == "__main__":
    main()
