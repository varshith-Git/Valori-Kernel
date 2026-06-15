"""
Valori Cluster Quickstart
=========================

Drives a 3-node Raft cluster through the Python SDK: insert through any node,
search locally on every node, and prove all replicas hold identical state.

Start a cluster first (either one works):

    docker compose up -d --build        # nodes on host ports 3001/3002/3003
    # or
    ./start-local-cluster.sh            # same, without Docker

Then:

    python examples/cluster_quickstart.py

The point of the demo: you can talk to ANY node. Writes are transparently
redirected to the leader; reads are answered locally by whichever node you hit;
and every node converges to the same cryptographic state hash.
"""

import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))

from valoricore import SyncRemoteClient, NotLeaderError  # noqa: E402

# Adjust if you started the cluster on different ports.
NODES = [
    "http://localhost:3001",
    "http://localhost:3002",
    "http://localhost:3003",
]


def wait_for_leader(clients, timeout=30):
    """Block until some node reports an elected leader."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        for c in clients:
            try:
                if c.cluster_health():
                    status = c.cluster_status()
                    print(f"  leader elected: node {status.get('leader')} (term {status.get('term')})")
                    return
            except Exception:
                pass
        time.sleep(1)
    raise SystemExit("no leader after 30s — is the cluster up? (docker compose ps)")


def main():
    clients = [SyncRemoteClient(url) for url in NODES]

    print("1. Waiting for the cluster to elect a leader...")
    wait_for_leader(clients)

    # 2. Write through a NON-leader on purpose — the SDK follows the 307 redirect.
    writer = clients[1]  # node 2, very likely a follower
    print("\n2. Inserting 5 vectors via node 2 (writes redirect to the leader)...")
    dim = 8
    ids = []
    for i in range(5):
        vec = [float((i + j) % 7) for j in range(dim)]
        try:
            ids.append(writer.insert(vec, tag=i))
        except NotLeaderError as e:
            raise SystemExit(f"cluster has no leader right now: {e}")
    print(f"   inserted record ids: {ids}")

    # 3. Search locally on EVERY node — all should return the same top hit.
    query = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.0]
    print("\n3. Searching the same query on each node (served locally):")
    for url, c in zip(NODES, clients):
        hits = c.search(query, k=3)
        top = hits[0] if hits else None
        print(f"   {url:24s} top hit: {top}")

    # 4. Prove the replicas are byte-identical: same BLAKE3 state hash everywhere.
    print("\n4. State hash on each node (must all match):")
    hashes = []
    for url, c in zip(NODES, clients):
        h = c.get_state_hash()
        hashes.append(h)
        print(f"   {url:24s} {h[:32]}...")
    if len(set(hashes)) == 1:
        print("\n   ✅ all nodes agree — replicas are cryptographically identical")
    else:
        print("\n   ⚠ hashes differ — give replication a moment and re-run")


if __name__ == "__main__":
    main()
