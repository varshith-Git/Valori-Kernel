"""
Valori Cluster Quickstart
=========================

Demonstrates a 3-node Raft cluster through the Python SDK:

  Part 1 — Basics
    • Insert through any node (writes redirect to the leader automatically).
    • Search locally on every node.
    • Verify all replicas hold the identical BLAKE3 state hash.

  Part 2 — Multi-tenancy (Collections)
    • Create named collections through the cluster.
    • Insert into a scoped collection and confirm isolation from the default
      namespace.
    • Drop a collection and verify the records are gone.

Prerequisites
─────────────
Start a cluster first:

    docker compose up -d --build        # nodes on host ports 3001/3002/3003
    # or
    ./start-local-cluster.sh

Then run:

    python examples/cluster_quickstart.py
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

DIM = 8


# ── helpers ───────────────────────────────────────────────────────────────────


def wait_for_leader(clients, timeout: int = 30):
    """Block until some node reports an elected leader."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        for c in clients:
            try:
                if c.cluster_health():
                    status = c.cluster_status()
                    print(
                        f"  leader elected: node {status.get('leader')} "
                        f"(term {status.get('term')})"
                    )
                    return
            except Exception:
                pass
        time.sleep(1)
    raise SystemExit("no leader after 30 s — is the cluster up? (docker compose ps)")


def vec(seed: int) -> list:
    return [float((seed + j) % 7) for j in range(DIM)]


# ── Part 1: basics ────────────────────────────────────────────────────────────


def part1_basics(clients):
    print("\n" + "─" * 60)
    print("PART 1 — Insert, search, verify identical state hashes")
    print("─" * 60)

    # Write through a NON-leader on purpose — the SDK follows the 307 redirect.
    writer = clients[1]  # node 2, very likely a follower
    print("\n1. Inserting 5 vectors via node 2 (writes redirect to the leader)...")
    ids = []
    for i in range(5):
        try:
            ids.append(writer.insert(vec(i), tag=i))
        except NotLeaderError as e:
            raise SystemExit(f"cluster has no leader right now: {e}")
    print(f"   inserted record ids: {ids}")

    # Search locally on EVERY node — all should return the same top hit.
    query = vec(0)
    print("\n2. Searching the same query on each node (served locally):")
    for url, c in zip(NODES, clients):
        hits = c.search(query, k=3)
        top = hits[0] if hits else None
        print(f"   {url:26s}  top hit: {top}")

    # Prove all replicas are byte-identical.
    print("\n3. State hash on each node (must all match):")
    hashes = []
    for url, c in zip(NODES, clients):
        h = c.get_state_hash()
        hashes.append(h)
        print(f"   {url:26s}  {h[:32]}...")
    if len(set(hashes)) == 1:
        print("\n   ✓ all nodes agree — replicas are cryptographically identical")
    else:
        print("\n   ✗ hashes differ — give replication a moment and re-run")


# ── Part 2: multi-tenancy ─────────────────────────────────────────────────────


def part2_collections(clients):
    print("\n" + "─" * 60)
    print("PART 2 — Collections (multi-tenancy)")
    print("─" * 60)

    # Use any node — the SDK follows the 307 redirect to the leader for writes.
    client = clients[0]

    # ── Create ──────────────────────────────────────────────────────────────
    print("\n1. Creating collection 'tenant-acme'...")
    result = client.create_collection("tenant-acme")
    print(f"   {result}")

    # Idempotent: calling again returns the same ID with created=False.
    result2 = client.create_collection("tenant-acme")
    print(f"   (idempotent) {result2}")

    # List — all nodes should show the same registry.
    print("\n2. Collections visible from each node:")
    for url, c in zip(NODES, clients):
        cols = c.list_collections()
        names = [col["name"] for col in cols]
        print(f"   {url:26s}  {names}")

    # ── Scoped insert ────────────────────────────────────────────────────────
    print("\n3. Inserting into 'tenant-acme' (via node 1) and 'default' (via node 3)...")
    acme_id = clients[0].insert(vec(10), collection="tenant-acme")
    default_id = clients[2].insert(vec(10))  # same vector, different tenant
    print(f"   tenant-acme record id: {acme_id}")
    print(f"   default    record id: {default_id}")

    # Batch insert into the collection.
    batch_ids = client.insert_batch(
        [vec(11), vec(12), vec(13)], collection="tenant-acme"
    )
    print(f"   batch into tenant-acme: ids = {batch_ids}")

    # ── Isolation check ──────────────────────────────────────────────────────
    print("\n4. Confirming namespace isolation (linearizable reads)...")

    # Search scoped to tenant-acme — must NOT see the default-namespace record.
    acme_hits = client.search(
        vec(10), k=10, collection="tenant-acme", consistency="linearizable"
    )
    acme_hit_ids = [h["id"] for h in acme_hits]

    # Search the default namespace — must NOT see the tenant-acme record.
    default_hits = client.search(vec(10), k=10, consistency="linearizable")
    default_hit_ids = [h["id"] for h in default_hits]

    print(f"   tenant-acme hits:  {acme_hit_ids}")
    print(f"   default hits:      {default_hit_ids}")

    acme_isolated = default_id not in acme_hit_ids
    default_isolated = acme_id not in default_hit_ids

    if acme_isolated and default_isolated:
        print("   ✓ namespaces are fully isolated")
    else:
        if not acme_isolated:
            print("   ✗ default record leaked into tenant-acme search")
        if not default_isolated:
            print("   ✗ tenant-acme record leaked into default search")

    # ── Drop ─────────────────────────────────────────────────────────────────
    print("\n5. Dropping 'tenant-acme'...")
    client.drop_collection("tenant-acme")

    # Searching the dropped collection should now return a 400 (ValueError).
    try:
        client.search(vec(10), k=5, collection="tenant-acme")
        print("   ✗ expected error — collection still exists after drop!")
    except Exception as e:
        print(f"   ✓ searching dropped collection raises: {type(e).__name__}: {e}")

    # Default record must still be searchable.
    remaining = client.search(vec(10), k=5)
    print(f"   default records still intact: {[h['id'] for h in remaining]}")

    # Final state-hash round to confirm all nodes converged after the drop.
    print("\n6. State hash after drop (all nodes must still agree):")
    hashes = []
    for url, c in zip(NODES, clients):
        h = c.get_state_hash()
        hashes.append(h)
        print(f"   {url:26s}  {h[:32]}...")
    if len(set(hashes)) == 1:
        print("\n   ✓ all nodes agree")
    else:
        print("\n   ✗ hashes differ")


# ── main ──────────────────────────────────────────────────────────────────────


def main():
    clients = [SyncRemoteClient(url) for url in NODES]

    print("Waiting for the cluster to elect a leader...")
    wait_for_leader(clients)

    part1_basics(clients)
    part2_collections(clients)

    print("\n" + "─" * 60)
    print("Done.")


if __name__ == "__main__":
    main()
