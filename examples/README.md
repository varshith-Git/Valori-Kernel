# Examples

Runnable examples for Valori. Most assume the Python SDK is installed
(`pip install ./python` from the repo root, or `make dev`).

## Cluster

| Example | What it shows |
|---|---|
| [cluster_quickstart.py](cluster_quickstart.py) | **Part 1** — Drive a 3-node Raft cluster: write to any node, search locally on every node, prove all replicas share one BLAKE3 state hash. **Part 2** — Multi-tenancy: create named collections, insert into scoped namespaces, confirm isolation, drop a collection. Start a cluster first with `docker compose up -d` or `./start-local-cluster.sh`. |

### Running the cluster quickstart

```bash
# 1. Start a 3-node cluster
docker compose up -d --build

# 2. Wait a few seconds for leader election, then run the demo
python examples/cluster_quickstart.py
```

Expected output (abbreviated):

```
Waiting for the cluster to elect a leader...
  leader elected: node 1 (term 1)

─────────────────────────────────────────────────────────
PART 1 — Insert, search, verify identical state hashes
─────────────────────────────────────────────────────────

1. Inserting 5 vectors via node 2 (writes redirect to the leader)...
   inserted record ids: [0, 1, 2, 3, 4]

2. Searching the same query on each node (served locally):
   http://localhost:3001  top hit: {'id': 0, 'score': 0}
   http://localhost:3002  top hit: {'id': 0, 'score': 0}
   http://localhost:3003  top hit: {'id': 0, 'score': 0}

3. State hash on each node (must all match):
   http://localhost:3001  a3f2c1...
   http://localhost:3002  a3f2c1...
   http://localhost:3003  a3f2c1...

   ✓ all nodes agree — replicas are cryptographically identical

─────────────────────────────────────────────────────────
PART 2 — Collections (multi-tenancy)
─────────────────────────────────────────────────────────

1. Creating collection 'tenant-acme'...
   {'name': 'tenant-acme', 'id': 1, 'created': True}
   (idempotent) {'name': 'tenant-acme', 'id': 1, 'created': False}

4. Confirming namespace isolation (linearizable reads)...
   tenant-acme hits:  [5]
   default hits:      [0, 1, 2, 3, 4, 6]
   ✓ namespaces are fully isolated

5. Dropping 'tenant-acme'...
   ✓ searching dropped collection raises: ValueError: ...

6. State hash after drop (all nodes must still agree):
   ✓ all nodes agree
```

## Framework integrations

| Example | What it shows |
|---|---|
| [langchain_example.py](langchain_example.py) | Valori as a LangChain vector store with `RetrievalQA` |
| [llamaindex_example.py](llamaindex_example.py) | Valori as a LlamaIndex vector store |

## SDK usage (under `python/examples/`)

| Example | What it shows |
|---|---|
| [comprehensive_demo.py](../python/examples/comprehensive_demo.py) | End-to-end tour of the SDK surface |
| [demo_remote.py](../python/examples/demo_remote.py) | Talking to a node over HTTP with `SyncRemoteClient` |
| [async_fastapi_demo.py](../python/examples/async_fastapi_demo.py) | `AsyncRemoteClient` inside a FastAPI service |
| [demo_embeddings.py](../python/examples/demo_embeddings.py) | Generating and ingesting embeddings |
| [demo_sentence_transformers.py](../python/examples/demo_sentence_transformers.py) | Sentence-Transformers → Valori pipeline |

## See also

- [docs/CLUSTER.md](../docs/CLUSTER.md) — cluster operations guide
- [docs/python-usage-guide.md](../docs/python-usage-guide.md) — full SDK walkthrough
- [docs/embedded-quickstart.md](../docs/embedded-quickstart.md) — the no-server embedded engine
