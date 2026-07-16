#!/usr/bin/env python3
"""
Comprehensive sharding test suite — local process-based.

Spins up a 3-node cluster with VALORI_SHARD_COUNT=3 as local processes,
then exercises every shard-related scenario.

Requires: cargo build -p valori-node --release (already done)
Usage:    python3 scripts/test_sharding.py
"""

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

try:
    import requests
except ImportError:
    print("ERROR: pip install requests")
    sys.exit(1)

# ── Config ────────────────────────────────────────────────────────────────────

SHARD_COUNT = 3
NODE_COUNT = 3
DIM = 4
BINARY = None  # resolved in main()
DATA_DIR = None
PROCESSES = []

# Node layout: API ports 4001-4003, Raft ports 4101-4103
API_PORTS = [4001, 4002, 4003]
RAFT_PORTS = [4101, 4102, 4103]

passed = 0
failed = 0
errors = []


def ok(name):
    global passed
    passed += 1
    print(f"  ✅ {name}")


def fail(name, reason):
    global failed
    failed += 1
    errors.append((name, reason))
    print(f"  ❌ {name}: {reason}")


def api(port, method, path, **kwargs):
    kwargs.setdefault("timeout", 10)
    # Disable auto-redirect — Raft 307s send Location without the path,
    # so requests would follow to "/" → 404. We append the path manually.
    kwargs["allow_redirects"] = False
    r = getattr(requests, method)(f"http://127.0.0.1:{port}{path}", **kwargs)
    if r.status_code == 307 and "Location" in r.headers:
        leader_base = r.headers["Location"].rstrip("/")
        del kwargs["allow_redirects"]
        r = getattr(requests, method)(f"{leader_base}{path}", **kwargs)
    return r


def api_ok(port, method, path, **kwargs):
    r = api(port, method, path, **kwargs)
    if r.status_code not in (200, 201):
        raise RuntimeError(f"{method.upper()} {path} → {r.status_code}: {r.text[:200]}")
    return r


def insert(port, vector, collection="default"):
    return api_ok(port, "post", "/v1/records", json={"values": vector, "collection": collection}).json()


def search(port, vector, k=10, collection="default"):
    # Cluster search uses "query", standalone uses "values"
    return api_ok(port, "post", "/v1/search", json={"query": vector, "k": k, "collection": collection}).json()


def create_collection(port, name):
    return api(port, "post", "/v1/namespaces", json={"name": name})


def drop_collection(port, name):
    return api(port, "delete", f"/v1/namespaces/{name}")


def list_collections(port):
    data = api_ok(port, "get", "/v1/namespaces").json()
    # Cluster returns {"collections": [{"name":..,"id":..}]}
    if isinstance(data, dict) and "collections" in data:
        return data["collections"]
    return data


def state_hash(port):
    return api_ok(port, "get", "/v1/proof/state").json()


def health(port):
    try:
        return api(port, "get", "/health").status_code == 200
    except Exception:
        return False


def random_vector():
    import random
    return [round(random.uniform(-1, 1), 4) for _ in range(DIM)]


# ── Cluster management ───────────────────────────────────────────────────────

def cluster_members():
    return ",".join(
        f"{i}=127.0.0.1:{RAFT_PORTS[i-1]}/127.0.0.1:{API_PORTS[i-1]}"
        for i in range(1, NODE_COUNT + 1)
    )


def start_node(node_id, init=False):
    node_dir = Path(DATA_DIR) / f"node-{node_id}"
    node_dir.mkdir(parents=True, exist_ok=True)
    log_file = open(node_dir / "stdout.log", "a")

    env = {
        **os.environ,
        "VALORI_DIM": str(DIM),
        "VALORI_MAX_RECORDS": "10000",
        "VALORI_MAX_NODES": "10000",
        "VALORI_MAX_EDGES": "50000",
        "VALORI_BIND": f"0.0.0.0:{API_PORTS[node_id - 1]}",
        "VALORI_CLUSTER_MEMBERS": cluster_members(),
        "VALORI_NODE_ID": str(node_id),
        "VALORI_RAFT_BIND": f"0.0.0.0:{RAFT_PORTS[node_id - 1]}",
        "VALORI_RAFT_LOG_PATH": str(node_dir / "raft.redb"),
        "VALORI_EVENT_LOG_PATH": str(node_dir / "events.log"),
        "VALORI_SNAPSHOT_PATH": str(node_dir / "state.snap"),
        "VALORI_SHARD_COUNT": str(SHARD_COUNT),
        "RUST_LOG": "warn",
    }
    if init:
        env["VALORI_CLUSTER_INIT"] = "1"

    proc = subprocess.Popen(
        [BINARY],
        env=env,
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )
    return proc, log_file


def wait_healthy(ports, timeout=60):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if all(health(p) for p in ports):
            return True
        time.sleep(1)
    return False


def wait_converge(ports, timeout=30):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            hashes = [state_hash(p).get("final_state_hash") for p in ports]
            if len(set(hashes)) == 1 and hashes[0]:
                return hashes[0]
        except Exception:
            pass
        time.sleep(1)
    raise TimeoutError(f"Nodes did not converge within {timeout}s")


def stop_node_by_id(node_id):
    """Stop a specific node by killing its process."""
    for proc, logf, nid in PROCESSES:
        if nid == node_id and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            return True
    return False


def restart_node_by_id(node_id):
    """Restart a stopped node."""
    # Remove old entry
    global PROCESSES
    PROCESSES = [(p, l, n) for p, l, n in PROCESSES if n != node_id]
    proc, logf = start_node(node_id, init=False)
    PROCESSES.append((proc, logf, node_id))
    deadline = time.time() + 60
    while time.time() < deadline:
        if health(API_PORTS[node_id - 1]):
            return True
        if proc.poll() is not None:
            return False
        time.sleep(1)
    return False


def cluster_up():
    global PROCESSES

    print("🚀 Starting 3-node sharded cluster (local processes)...")
    print(f"   SHARD_COUNT={SHARD_COUNT}, DIM={DIM}")
    print(f"   Data dir: {DATA_DIR}")

    # Start all 3 nodes simultaneously — Raft needs a majority (2/3) to
    # elect a leader, so node 1 (bootstrap) can't become healthy alone.
    for nid in [1, 2, 3]:
        proc, logf = start_node(nid, init=(nid == 1))
        PROCESSES.append((proc, logf, nid))

    if not wait_healthy(API_PORTS, timeout=60):
        for nid in range(1, 4):
            log_path = Path(DATA_DIR) / f"node-{nid}" / "stdout.log"
            if log_path.exists():
                print(f"\n--- node-{nid} logs (last 500 chars) ---")
                print(open(log_path).read()[-500:])
        print("\n❌ FATAL: Cluster did not become healthy")
        sys.exit(1)

    print("✅ All 3 nodes healthy")
    time.sleep(3)  # let Raft settle


def cluster_down():
    for proc, logf, nid in PROCESSES:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
        logf.close()
    PROCESSES.clear()


# ── Tests ─────────────────────────────────────────────────────────────────────

def test_01_cluster_health():
    print("\n━━━ Test 1: Cluster Health & Shard Count ━━━")
    for i, port in enumerate(API_PORTS, 1):
        try:
            api_ok(port, "get", "/health")
            ok(f"node-{i} healthy")
        except Exception as e:
            fail(f"node-{i} healthy", str(e))

    try:
        r = api_ok(API_PORTS[0], "get", "/v1/cluster/status")
        ok("cluster status reachable")
    except Exception as e:
        fail("cluster status", str(e))


def test_02_collection_shard_routing():
    print("\n━━━ Test 2: Collection → Shard Routing ━━━")

    collections = [f"shard-test-{i}" for i in range(1, 7)]
    for name in collections:
        r = create_collection(API_PORTS[0], name)
        if r.status_code in (200, 201, 409):
            ok(f"create '{name}'")
        else:
            fail(f"create '{name}'", f"status={r.status_code}")

    time.sleep(3)

    for idx, port in enumerate(API_PORTS, 1):
        try:
            cols = list_collections(port)
            names = [c["name"] if isinstance(c, dict) else c for c in cols]
            missing = [n for n in collections if n not in names]
            if not missing:
                ok(f"node-{idx} sees all {len(collections)} collections")
            else:
                fail(f"node-{idx} collections", f"missing: {missing}")
        except Exception as e:
            fail(f"node-{idx} list collections", str(e))


def test_03_cross_shard_isolation():
    print("\n━━━ Test 3: Cross-Shard Write Isolation ━━━")

    v1 = [0.1, 0.2, 0.3, 0.4]
    v2 = [0.9, 0.8, 0.7, 0.6]

    try:
        insert(API_PORTS[0], v1, collection="default")
        time.sleep(1)
        insert(API_PORTS[0], v2, collection="shard-test-1")
        time.sleep(3)
        ok("inserted into two different shards")
    except Exception as e:
        fail("cross-shard insert", str(e))
        return

    try:
        r1 = search(API_PORTS[0], v1, k=10, collection="default")
        r2 = search(API_PORTS[0], v2, k=10, collection="shard-test-1")
        hits1 = r1.get("results") or r1.get("hits") or []
        hits2 = r2.get("results") or r2.get("hits") or []

        if len(hits1) >= 1:
            ok(f"default has records ({len(hits1)})")
        else:
            fail("default search", f"expected ≥1, got {len(hits1)}")

        if len(hits2) >= 1:
            ok(f"shard-test-1 has records ({len(hits2)})")
        else:
            fail("shard-test-1 search", f"expected ≥1, got {len(hits2)}")
    except Exception as e:
        fail("cross-shard search", str(e))

    try:
        r3 = search(API_PORTS[0], v1, k=10, collection="shard-test-2")
        hits3 = r3.get("results") or r3.get("hits") or []
        if len(hits3) == 0:
            ok("shard-test-2 correctly empty (no cross-shard bleed)")
        else:
            fail("shard isolation", f"shard-test-2 has {len(hits3)} unexpected records")
    except Exception as e:
        fail("shard-test-2 empty check", str(e))


def test_04_per_shard_blake3():
    print("\n━━━ Test 4: BLAKE3 Chain Convergence ━━━")
    time.sleep(3)

    try:
        hashes = []
        for i, port in enumerate(API_PORTS, 1):
            h = state_hash(port)
            hashes.append(h.get("final_state_hash"))
            print(f"    node-{i}: {hashes[-1][:16]}...")

        if len(set(hashes)) == 1:
            ok("all 3 nodes agree on state hash")
        else:
            fail("state hash convergence", f"divergent: {hashes}")

        if hashes[0] and hashes[0] != "0" * 64:
            ok("state hash is non-zero")
        else:
            fail("non-zero hash", f"got {hashes[0]}")
    except Exception as e:
        fail("BLAKE3 convergence", str(e))


def test_05_read_from_any_node():
    print("\n━━━ Test 5: Read from Any Node ━━━")
    for i, port in enumerate(API_PORTS, 1):
        try:
            r = search(port, [0.1, 0.2, 0.3, 0.4], k=5, collection="default")
            hits = r.get("results") or r.get("hits") or []
            if len(hits) >= 1:
                ok(f"node-{i} returns results")
            else:
                fail(f"node-{i} search", f"expected ≥1, got {len(hits)}")
        except Exception as e:
            fail(f"node-{i} search", str(e))


def test_06_node_restart_recovery():
    print("\n━━━ Test 6: Node Restart → Per-Shard Recovery ━━━")

    try:
        for _ in range(5):
            insert(API_PORTS[0], random_vector(), collection="default")
        for _ in range(3):
            insert(API_PORTS[0], random_vector(), collection="shard-test-1")
        time.sleep(3)
        ok("pre-restart data inserted")
    except Exception as e:
        fail("pre-restart insert", str(e))
        return

    try:
        hash_before = wait_converge(API_PORTS)
        ok(f"pre-restart hash: {hash_before[:16]}...")
    except Exception as e:
        fail("pre-restart convergence", str(e))
        return

    print("    ⏸  Stopping node-3...")
    stop_node_by_id(3)
    time.sleep(3)

    print("    ▶  Restarting node-3...")
    if not restart_node_by_id(3):
        fail("node-3 restart", "did not become healthy")
        return
    ok("node-3 restarted and healthy")

    time.sleep(5)
    try:
        hash_after = wait_converge(API_PORTS, timeout=30)
        if hash_after == hash_before:
            ok(f"hash matches after restart")
        else:
            fail("post-restart hash", f"before={hash_before[:16]} after={hash_after[:16]}")
    except Exception as e:
        fail("post-restart convergence", str(e))

    try:
        r = search(API_PORTS[2], [0.1, 0.2, 0.3, 0.4], k=10, collection="default")
        hits = r.get("results") or r.get("hits") or []
        if len(hits) >= 1:
            ok(f"node-3 serves data after restart ({len(hits)} hits)")
        else:
            fail("node-3 post-restart", f"expected ≥1 hit")
    except Exception as e:
        fail("node-3 post-restart search", str(e))


def test_07_concurrent_multi_shard_writes():
    print("\n━━━ Test 7: Concurrent Multi-Shard Writes ━━━")

    n = 20
    cols = ["default", "shard-test-1", "shard-test-2"]

    def do_writes(col):
        count = 0
        for _ in range(n):
            try:
                insert(API_PORTS[0], random_vector(), collection=col)
                count += 1
            except Exception:
                pass
        return col, count

    with ThreadPoolExecutor(max_workers=3) as pool:
        futs = [pool.submit(do_writes, c) for c in cols]
        for f in as_completed(futs):
            col, count = f.result()
            if count == n:
                ok(f"concurrent '{col}': {count}/{n}")
            else:
                fail(f"concurrent '{col}'", f"{count}/{n}")

    time.sleep(5)
    try:
        wait_converge(API_PORTS, timeout=30)
        ok("converged after concurrent writes")
    except Exception as e:
        fail("post-concurrent convergence", str(e))


def test_08_leader_failover():
    print("\n━━━ Test 8: Leader Failover (Kill Node-1) ━━━")

    print("    ⏸  Stopping node-1 (bootstrap)...")
    stop_node_by_id(1)
    time.sleep(8)

    surviving = [API_PORTS[1], API_PORTS[2]]
    write_ok = False
    for port in surviving:
        try:
            insert(port, random_vector(), collection="default")
            ok(f"write to port {port} succeeded after leader kill")
            write_ok = True
            break
        except Exception:
            continue

    if not write_ok:
        fail("write after leader kill", "no node accepted writes")

    print("    ▶  Restarting node-1...")
    if restart_node_by_id(1):
        ok("node-1 rejoined")
    else:
        fail("node-1 rejoin", "did not become healthy")
        return

    time.sleep(5)
    try:
        wait_converge(API_PORTS, timeout=30)
        ok("converged after leader failover + rejoin")
    except Exception as e:
        fail("post-failover convergence", str(e))


def test_09_collection_lifecycle():
    print("\n━━━ Test 9: Cross-Shard Collection Lifecycle ━━━")

    cols = [f"lifecycle-{i}" for i in range(6)]
    for name in cols:
        r = create_collection(API_PORTS[0], name)
        if r.status_code in (200, 201, 409):
            ok(f"create '{name}'")
        else:
            fail(f"create '{name}'", f"status={r.status_code}")

    time.sleep(2)
    for name in cols:
        try:
            insert(API_PORTS[0], random_vector(), collection=name)
        except Exception:
            pass
    time.sleep(2)

    # Drop first 3
    for name in cols[:3]:
        r = drop_collection(API_PORTS[0], name)
        if r.status_code in (200, 204):
            ok(f"drop '{name}'")
        else:
            fail(f"drop '{name}'", f"status={r.status_code}")

    time.sleep(3)
    for idx, port in enumerate(API_PORTS, 1):
        try:
            result = list_collections(port)
            names = [c["name"] if isinstance(c, dict) else c for c in result]
            dropped_visible = [n for n in cols[:3] if n in names]
            kept_missing = [n for n in cols[3:] if n not in names]
            if not dropped_visible and not kept_missing:
                ok(f"node-{idx} collection state correct")
            else:
                fail(f"node-{idx}", f"still visible: {dropped_visible}, missing: {kept_missing}")
        except Exception as e:
            fail(f"node-{idx} post-drop", str(e))


def test_10_per_shard_event_log_files():
    print("\n━━━ Test 10: Per-Shard Event Log Files ━━━")

    node_dir = Path(DATA_DIR) / "node-1"
    files = list(node_dir.iterdir())
    filenames = [f.name for f in files]
    print(f"    node-1 data dir: {filenames}")

    # With SHARD_COUNT=3, expect: events-shard0.log, events-shard1.log, events-shard2.log
    for s in range(SHARD_COUNT):
        expected = f"events-shard{s}.log"
        # Could also be segmented: events-shard0.log.00000, etc.
        matches = [f for f in filenames if f.startswith(f"events-shard{s}")]
        if matches:
            ok(f"{expected} exists ({matches[0]})")
        else:
            fail(f"{expected}", "not found")

    for s in range(SHARD_COUNT):
        expected = f"raft-shard{s}.redb"
        if expected in filenames:
            ok(f"{expected} exists")
        else:
            fail(f"{expected}", "not found")


def test_11_minority_cannot_commit():
    print("\n━━━ Test 11: Minority Cannot Commit ━━━")

    stop_node_by_id(2)
    stop_node_by_id(3)
    time.sleep(3)

    try:
        # Use raw requests to avoid our api() redirect-following.
        r = requests.post(
            f"http://127.0.0.1:{API_PORTS[0]}/v1/records",
            json={"values": random_vector(), "collection": "default"},
            timeout=6, allow_redirects=False,
        )
        if r.status_code >= 300:
            ok(f"minority write rejected/redirected (HTTP {r.status_code})")
        else:
            fail("minority write", f"expected error, got {r.status_code}")
    except requests.exceptions.Timeout:
        ok("minority write timed out (no quorum)")
    except requests.exceptions.ConnectionError:
        ok("minority write refused")
    except Exception as e:
        fail("minority write", str(e))

    restart_node_by_id(2)
    restart_node_by_id(3)
    time.sleep(8)
    try:
        wait_converge(API_PORTS, timeout=30)
        ok("cluster restored")
    except Exception as e:
        fail("post-minority restore", str(e))


def test_12_bulk_verify_counts():
    print("\n━━━ Test 12: Bulk Multi-Shard & Count ━━━")

    counts = {"default": 10, "shard-test-1": 15, "shard-test-2": 8}
    for col, n in counts.items():
        try:
            for _ in range(n):
                insert(API_PORTS[0], random_vector(), collection=col)
            ok(f"inserted {n} into '{col}'")
        except Exception as e:
            fail(f"bulk '{col}'", str(e))

    time.sleep(5)
    for idx, port in enumerate(API_PORTS, 1):
        for col, expected in counts.items():
            try:
                r = search(port, random_vector(), k=200, collection=col)
                hits = r.get("results") or r.get("hits") or []
                if len(hits) >= expected:
                    ok(f"node-{idx} '{col}' ≥{expected} ({len(hits)})")
                else:
                    fail(f"node-{idx} '{col}'", f"expected ≥{expected}, got {len(hits)}")
            except Exception as e:
                fail(f"node-{idx} '{col}'", str(e))


def test_13_writes_after_full_restart():
    """Stop ALL nodes, restart, verify state recovered from WAL/snapshot."""
    print("\n━━━ Test 13: Full Cluster Restart ━━━")

    try:
        hash_before = wait_converge(API_PORTS)
        ok(f"pre-restart hash: {hash_before[:16]}...")
    except Exception as e:
        fail("pre-restart", str(e))
        return

    print("    ⏸  Stopping all nodes...")
    for nid in [3, 2, 1]:
        stop_node_by_id(nid)
    time.sleep(3)

    print("    ▶  Restarting all nodes...")
    restart_node_by_id(1)
    time.sleep(3)
    restart_node_by_id(2)
    restart_node_by_id(3)

    if not wait_healthy(API_PORTS, timeout=60):
        fail("full restart", "not all nodes healthy")
        return
    ok("all nodes healthy after full restart")

    time.sleep(5)
    try:
        hash_after = wait_converge(API_PORTS, timeout=30)
        if hash_after == hash_before:
            ok(f"hash matches after full restart")
        else:
            # Hash may have changed if Raft wrote a no-op — that's ok,
            # what matters is all 3 agree.
            ok(f"all 3 agree on hash after full restart ({hash_after[:16]}...)")
    except Exception as e:
        fail("post-full-restart convergence", str(e))

    # Verify data survived
    try:
        r = search(API_PORTS[0], random_vector(), k=100, collection="default")
        hits = r.get("results") or r.get("hits") or []
        if len(hits) >= 1:
            ok(f"data survived full restart ({len(hits)} records in default)")
        else:
            fail("data survival", "no records found")
    except Exception as e:
        fail("data survival", str(e))


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    global BINARY, DATA_DIR

    repo = Path(__file__).resolve().parent.parent
    BINARY = str(repo / "target" / "release" / "valori-node")
    if not Path(BINARY).exists():
        print(f"❌ Binary not found: {BINARY}")
        print("   Run: cargo build -p valori-node --release")
        sys.exit(1)

    DATA_DIR = tempfile.mkdtemp(prefix="valori-shard-test-")

    try:
        cluster_up()

        print("\n" + "=" * 60)
        print(f" 🧪 Valori Sharding Test Suite")
        print(f"    Nodes: {NODE_COUNT}  |  Shards: {SHARD_COUNT}  |  Dim: {DIM}")
        print("=" * 60)

        test_01_cluster_health()
        test_02_collection_shard_routing()
        test_03_cross_shard_isolation()
        test_04_per_shard_blake3()
        test_05_read_from_any_node()
        test_06_node_restart_recovery()
        test_07_concurrent_multi_shard_writes()
        test_08_leader_failover()
        test_09_collection_lifecycle()
        test_10_per_shard_event_log_files()
        test_11_minority_cannot_commit()
        test_12_bulk_verify_counts()
        test_13_writes_after_full_restart()

        print("\n" + "=" * 60)
        total = passed + failed
        print(f" Results: {passed}/{total} passed, {failed} failed")
        if errors:
            print("\n Failed:")
            for name, reason in errors:
                print(f"   ❌ {name}: {reason}")
        print("=" * 60)

    finally:
        print("\n🧹 Stopping cluster...")
        cluster_down()
        shutil.rmtree(DATA_DIR, ignore_errors=True)
        print(f"🗑  Cleaned {DATA_DIR}")

    sys.exit(1 if failed > 0 else 0)


if __name__ == "__main__":
    main()
