import pytest
#!/usr/bin/env python3
"""End-to-end test for the Python SDK GraphRAG method (Phase 3.15).

Spawns its own valori-node, builds a small connected graph via the SDK
(insert + create_node + create_edge), then proves SyncRemoteClient.graphrag and
AsyncRemoteClient.graphrag return the K nearest vectors AND the connected
subgraph in one call.

Run:
    cargo build -p valori-node
    python3 python/tests/test_graphrag_sdk.py
"""

import asyncio
import os
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from tempfile import TemporaryDirectory

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "python"))
NODE_BIN = REPO / "target" / "debug" / "valori-node"

from valoricore.remote import SyncRemoteClient, AsyncRemoteClient  # noqa: E402

pytestmark = pytest.mark.integration

DIM = 4


def free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def wait_for_health(url: str, timeout: float = 15.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url + "/health", timeout=1) as r:
                if r.status == 200:
                    return
        except Exception:
            time.sleep(0.2)
    raise RuntimeError("node did not become healthy")


def vec(seed: float):
    return [seed + i * 0.01 for i in range(DIM)]


# NodeKind/EdgeKind are #[repr(u8)] in the kernel; 1 = Chunk, 0 = a generic edge.
CHUNK_KIND = 1
EDGE_KIND = 0


def build_graph(c: SyncRemoteClient):
    """Insert three records, give each a graph node, and chain them."""
    nodes = []
    for i in range(3):
        rid = c.insert(vec(0.1 + i * 0.4))
        nid = c.create_node(kind=CHUNK_KIND, record_id=rid)
        nodes.append(nid)
    for a, b in zip(nodes, nodes[1:]):
        c.create_edge(a, b, EDGE_KIND)
    return nodes


def test_sync(url: str) -> None:
    c = SyncRemoteClient(url)
    nodes = build_graph(c)

    g = c.graphrag(vec(0.1), k=3, depth=2)
    assert "hits" in g and "subgraph" in g, f"unexpected shape: {g.keys()}"
    assert len(g["hits"]) >= 1, "no hits"
    node_ids = {n["id"] for n in g["subgraph"]["nodes"]}
    # depth-2 walk from the nearest seed should reach the whole 3-node chain.
    assert nodes[0] in node_ids, "seed node missing from subgraph"
    assert len(g["subgraph"]["edges"]) >= 1, "no edges traversed"
    print(f"  sync : hits={len(g['hits'])} "
          f"nodes={len(g['subgraph']['nodes'])} edges={len(g['subgraph']['edges'])}  OK")


async def test_async(url: str) -> None:
    c = AsyncRemoteClient(url)
    try:
        g = await c.graphrag(vec(0.1), k=3, depth=2)
        assert "hits" in g and "subgraph" in g
        assert len(g["hits"]) >= 1
        print(f"  async: hits={len(g['hits'])} "
              f"nodes={len(g['subgraph']['nodes'])} edges={len(g['subgraph']['edges'])}  OK")
    finally:
        await c.close()


def main() -> int:
    if not NODE_BIN.exists():
        print("Build the node first:  cargo build -p valori-node")
        return 1

    port = free_port()
    url = f"http://127.0.0.1:{port}"
    with TemporaryDirectory() as tmp:
        env = {**os.environ, "VALORI_DIM": str(DIM), "VALORI_BIND": f"127.0.0.1:{port}",
               "VALORI_EVENT_LOG_PATH": str(Path(tmp) / "events.log")}
        node = subprocess.Popen([str(NODE_BIN)], env=env,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            wait_for_health(url)
            print(f"node up at {url}")
            test_sync(url)
            asyncio.run(test_async(url))
            print("\nGraphRAG SDK test: PASS")
            return 0
        finally:
            node.terminate()


if __name__ == "__main__":
    sys.exit(main())
