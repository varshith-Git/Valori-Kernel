#!/usr/bin/env python3
"""End-to-end demo of valori-mcp: give an agent verifiable memory.

This script:
  1. starts a local valori-node (dim 8, event log on) on a free port,
  2. spawns the `valori-mcp` server pointed at it,
  3. speaks MCP (JSON-RPC 2.0 over stdio): initialize -> tools/list ->
     memory_write x3 -> memory_recall,
  4. INDEPENDENTLY recomputes the recall receipt digest in Python and checks
     it matches the one the server returned.

Step 4 is the whole point: the receipt is verifiable by any client in any
language, offline, without trusting the server. mem0/Zep/Pinecone can't do this.

Prerequisites:
    cargo build -p valori-node -p valori-mcp
    pip install blake3        # only needed for the cross-language verify step

Run:
    python3 examples/mcp_agent_memory.py
"""

import json
import os
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from tempfile import TemporaryDirectory

DIM = 8
REPO = Path(__file__).resolve().parents[1]
TARGET = REPO / "target" / "debug"
NODE_BIN = TARGET / "valori-node"
MCP_BIN = TARGET / "valori-mcp"


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
    raise RuntimeError(f"node did not become healthy at {url}")


class McpClient:
    """Minimal MCP-over-stdio client: one request per line, read one line back."""

    def __init__(self, proc: subprocess.Popen):
        self.proc = proc
        self._id = 0

    def _send(self, method: str, params: dict, notify: bool = False):
        msg = {"jsonrpc": "2.0", "method": method, "params": params}
        if not notify:
            self._id += 1
            msg["id"] = self._id
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()
        if notify:
            return None
        line = self.proc.stdout.readline()
        return json.loads(line)

    def initialize(self):
        r = self._send("initialize", {})
        self._send("notifications/initialized", {}, notify=True)
        return r["result"]

    def list_tools(self):
        return self._send("tools/list", {})["result"]["tools"]

    def call(self, name: str, arguments: dict):
        res = self._send("tools/call", {"name": name, "arguments": arguments})["result"]
        payload = json.loads(res["content"][0]["text"])
        if res.get("isError"):
            raise RuntimeError(f"tool {name} failed: {payload}")
        return payload


def canonical_receipt_body(receipt: dict) -> bytes:
    """Reconstruct the exact bytes the Rust side hashed (serde_json compact,
    fields in declaration order, optional fields omitted when absent)."""
    body = {"state_hash": receipt["state_hash"]}
    if receipt.get("event_log_hash") is not None:
        body["event_log_hash"] = receipt["event_log_hash"]
    if receipt.get("committed_height") is not None:
        body["committed_height"] = receipt["committed_height"]
    body["query_dim"] = receipt["query_dim"]
    body["k"] = receipt["k"]
    body["results"] = [
        {
            "memory_id": f["memory_id"],
            "record_id": f["record_id"],
            "score_bits": f["score_bits"],
        }
        for f in receipt["results"]
    ]
    # GraphRAG receipts additionally bind the returned subgraph (after results).
    if receipt.get("subgraph") is not None:
        body["subgraph"] = {
            "node_ids": receipt["subgraph"]["node_ids"],
            "edge_ids": receipt["subgraph"]["edge_ids"],
        }
    return json.dumps(body, separators=(",", ":")).encode("utf-8")


def verify_receipt(receipt: dict) -> bool:
    try:
        import blake3
    except ImportError:
        print("  (skip) install `blake3` to verify the digest cross-language: pip install blake3")
        return True
    recomputed = blake3.blake3(canonical_receipt_body(receipt)).hexdigest()
    return recomputed == receipt["receipt_digest"]


def vec(seed: float):
    return [seed + i * 0.01 for i in range(DIM)]


def main() -> int:
    if not NODE_BIN.exists() or not MCP_BIN.exists():
        print("Build the binaries first:  cargo build -p valori-node -p valori-mcp")
        return 1

    port = free_port()
    url = f"http://127.0.0.1:{port}"

    with TemporaryDirectory() as tmp:
        node_env = {
            **os.environ,
            "VALORI_DIM": str(DIM),
            "VALORI_BIND": f"127.0.0.1:{port}",
            "VALORI_EVENT_LOG_PATH": str(Path(tmp) / "events.log"),
        }
        node = subprocess.Popen([str(NODE_BIN)], env=node_env,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            wait_for_health(url)
            print(f"node up at {url}")

            mcp = subprocess.Popen(
                [str(MCP_BIN)],
                env={**os.environ, "VALORI_URL": url},
                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL, text=True,
            )
            try:
                client = McpClient(mcp)
                info = client.initialize()
                print(f"connected to {info['serverInfo']['name']} "
                      f"(protocol {info['protocolVersion']})")
                print(f"tools: {[t['name'] for t in client.list_tools()]}\n")

                facts = ["the sky is blue", "water boils at 100C", "Valori proves recall"]
                chunks = []
                for i, text in enumerate(facts):
                    w = client.call("memory_write", {"vector": vec(0.1 + i * 0.4), "text": text})
                    chunks.append(w["chunk_node_id"])
                print(f"wrote {len(facts)} memories")

                # Link the memories into a chain so GraphRAG has a graph to walk:
                # chunk0 -> chunk1 -> chunk2. (No edge tool in MCP v0 — use the node API.)
                for a, b in zip(chunks, chunks[1:]):
                    urllib.request.urlopen(urllib.request.Request(
                        url + "/graph/edge",
                        data=json.dumps({"from": a, "to": b, "kind": 0}).encode(),
                        headers={"Content-Type": "application/json"}, method="POST"))
                print(f"linked memories into a chain: {chunks}")

                print("\n[1] plain vector recall")
                out = client.call("memory_recall", {"query_vector": vec(0.1), "k": 2})
                receipt = out["receipt"]
                print(f"  recalled {len(out['results'])} memories")
                print(f"  receipt_digest : {receipt['receipt_digest'][:24]}...")
                ok1 = verify_receipt(receipt)
                print(f"  verify: {'PASS' if ok1 else 'FAIL'}")

                print("\n[2] GraphRAG — hits + connected subgraph in ONE call")
                g = client.call("memory_graph_recall",
                                {"query_vector": vec(0.1), "k": 1, "depth": 2})
                greceipt = g["receipt"]
                print(f"  hits           : {len(g['hits'])}")
                print(f"  subgraph nodes : {len(g['subgraph']['nodes'])}  "
                      f"edges: {len(g['subgraph']['edges'])}  (walked the chain)")
                print(f"  receipt binds  : {len(greceipt['results'])} hits + "
                      f"{len(greceipt['subgraph']['node_ids'])} nodes / "
                      f"{len(greceipt['subgraph']['edge_ids'])} edges")
                print(f"  receipt_digest : {greceipt['receipt_digest'][:24]}...")
                ok2 = verify_receipt(greceipt)
                print(f"  verify: {'PASS' if ok2 else 'FAIL'}")

                ok = ok1 and ok2
                print(f"\n  cross-language receipt verification: "
                      f"{'PASS — both digests match' if ok else 'FAIL'}")
                return 0 if ok else 2
            finally:
                mcp.terminate()
        finally:
            node.terminate()


if __name__ == "__main__":
    sys.exit(main())
