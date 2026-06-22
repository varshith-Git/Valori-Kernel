# valori-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) server that gives
any agent **verifiable, deterministic long-term memory** backed by a Valori node.

The differentiator vs mem0 / Zep / Letta / Pinecone: `memory_recall` returns a
cryptographic **receipt** — a BLAKE3 digest binding the exact result set to the
committed state hash at recall time. You can prove, later and offline, what your
agent recalled. No other agent-memory store does this.

## How it works

`valori-mcp` is a thin MCP front-end. It speaks JSON-RPC 2.0 over stdio and
translates the six memory tools into HTTP calls against endpoints a Valori node
already exposes. It adds no server logic — it *composes* (recall = search +
proof → receipt).

```
MCP client (Claude Desktop / agent)
        │  JSON-RPC 2.0 over stdio
        ▼
   valori-mcp  ──HTTP──▶  valori-node  (/v1/memory/*, /v1/proof/*, /graph/*, …)
```

## Tools

| Tool | Backed by | What's special |
|---|---|---|
| `memory_write` | `POST /v1/memory/upsert_vector` | every write is BLAKE3-chained into the audit log |
| `memory_recall` | search + `/v1/proof/*` | **returns a verifiable receipt** |
| `memory_graph_recall` | `POST /v1/graphrag` + `/v1/proof/*` | **GraphRAG in one call** — hits + connected subgraph, receipt binds both |
| `memory_why` | `GET /graph/subgraph` | provenance subgraph — why a memory is held |
| `memory_timeline` | `GET /v1/timeline` | the agent's memory as an auditable timeline |
| `memory_forget` | `DELETE /v1/crypto/shred/:key_id` | certified, GDPR-grade erasure |
| `memory_fork` | `POST /v1/snapshot/save` | deterministic snapshot = a fork point |

### `memory_graph_recall` — GraphRAG in one call

Recalls the k nearest memories **and** the knowledge subgraph connecting them
(sources, related entities, citations) up to `depth` hops — from a single
consistent snapshot, no Neo4j+vector-DB two-system dance. The receipt binds both
the hits and the subgraph:

```jsonc
{
  "hits":       [ { "memory_id", "record_id", "score", "node_id" } ],
  "subgraph":   { "nodes": [...], "edges": [...] },
  "seed_nodes": [ ... ],
  "receipt":    { ..., "subgraph": { "node_ids": [...], "edge_ids": [...] },
                  "receipt_digest": "<64 hex>" }
}
```

> v0 takes a pre-computed embedding `vector` on writes/recalls (the client
> embeds). A built-in embedding adapter is a planned follow-up.

## The receipt

`memory_recall` returns `{ results, receipt }`. The receipt body is hashed in
declaration order:

```jsonc
{
  "state_hash": "<64 hex>",          // kernel BLAKE3 Merkle root at recall time
  "event_log_hash": "<64 hex>",      // BLAKE3 of the on-disk event log (if enabled)
  "committed_height": 12,            // events backing this state
  "query_dim": 8,
  "k": 2,
  "results": [                       // ordered fingerprints of what was returned
    { "memory_id": "...", "record_id": 1, "score_bits": "<f64 bits>" }
  ]
}
// receipt_digest = BLAKE3(compact_json(body))  — recomputable by any client
```

`score_bits` is the raw IEEE-754 bit pattern (decimal string) so the digest is
exact across languages and platforms. `recalled_at_unix` is advisory and is
**not** part of the digest — the cryptographic anchor is the committed state.

To verify: reconstruct the body, serialize it compact in declaration order,
BLAKE3 it, and compare to `receipt_digest`. See
[`examples/mcp_agent_memory.py`](../../examples/mcp_agent_memory.py) for a
working cross-language verifier in Python.

## Run it

```bash
# 1. start a node (event log on → receipts carry an event-log hash)
VALORI_DIM=8 VALORI_EVENT_LOG_PATH=/tmp/events.log valori-node &

# 2. point the MCP server at it
VALORI_URL=http://localhost:3000 valori-mcp
```

### Claude Desktop

Add to your `claude_desktop_config.json` (see
[`examples/claude_desktop_config.json`](../../examples/claude_desktop_config.json)):

```json
{ "mcpServers": { "valori": {
  "command": "valori-mcp",
  "env": { "VALORI_URL": "http://localhost:3000" }
} } }
```

## Configuration

| Flag | Env | Default | Purpose |
|---|---|---|---|
| `--url` | `VALORI_URL` | `http://localhost:3000` | Valori node base URL |
| `--auth-token` | `VALORI_AUTH_TOKEN` | — | Bearer token if the node has auth on |

## Testing

```bash
cargo test -p valori-mcp
```

| Suite | Covers |
|---|---|
| `src/*` unit tests (29) | JSON-RPC framing, receipt digest determinism/tamper-evidence (incl. subgraph binding), tool dispatch, MCP handshake, stdio line handling |
| `tests/integration_node.rs` (4) | full flow against a **real** in-process node: recall + GraphRAG receipts independently recomputed and matched; timeline reflects writes; handshake over the line transport |

## Protocol notes

- MCP revision `2024-11-05` (stdio binding).
- stdout is the protocol channel — all diagnostics go to **stderr**.
- Tool failures are returned as a result with `isError: true` (so the model sees
  them); only protocol-level faults are JSON-RPC errors.
