# Examples

Runnable examples for Valori. Most assume the Python SDK is installed
(`pip install ./python` from the repo root, or `make dev`).

## Cluster

| Example | What it shows |
|---|---|
| [cluster_quickstart.py](cluster_quickstart.py) | Drive a 3-node Raft cluster: write to any node, read locally on every node, prove all replicas share one state hash. Start a cluster first with `docker compose up -d` or `./start-local-cluster.sh`. |

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
