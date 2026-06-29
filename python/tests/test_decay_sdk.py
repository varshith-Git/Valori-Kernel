import pytest
#!/usr/bin/env python3
"""End-to-end test for the Python SDK decay parameter (Phase C4.1).

Spawns its own valori-node, inserts records, and proves that
SyncRemoteClient.search / AsyncRemoteClient.search accept
``decay_half_life_secs`` and surface per-hit ``decay_factor`` / ``age_secs``
without breaking the plain (no-decay) path.

Run:
    cargo build -p valori-node
    python3 python/tests/test_decay_sdk.py
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


def test_sync(url: str) -> None:
    c = SyncRemoteClient(url)
    for i in range(3):
        c.insert(vec(0.1 + i * 0.3))

    # Plain search: no decay fields present.
    plain = c.search(vec(0.1), k=3)
    assert plain and "decay_factor" not in plain[0], "no decay fields when off"

    # Decayed search: every hit carries a factor in (0, 1].
    decayed = c.search(vec(0.1), k=3, decay_half_life_secs=3600)
    assert decayed, "decayed search returned no hits"
    for h in decayed:
        assert "decay_factor" in h, "decay_factor missing"
        assert 0.0 < h["decay_factor"] <= 1.0, f"factor out of range: {h['decay_factor']}"
        # Freshly inserted → barely aged → factor ~1.0.
        assert h["decay_factor"] > 0.99, "fresh record should barely decay"
    print(f"  sync : plain={len(plain)} decayed={len(decayed)} "
          f"factor0={decayed[0]['decay_factor']:.4f}  OK")


async def test_async(url: str) -> None:
    c = AsyncRemoteClient(url)
    try:
        decayed = await c.search(vec(0.1), k=3, decay_half_life_secs=60)
        assert decayed and "decay_factor" in decayed[0]
        print(f"  async: decayed={len(decayed)} "
              f"factor0={decayed[0]['decay_factor']:.4f}  OK")
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
            print("\nDecay SDK test: PASS")
            return 0
        finally:
            node.terminate()


if __name__ == "__main__":
    sys.exit(main())
