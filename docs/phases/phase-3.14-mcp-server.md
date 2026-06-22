# Phase 3.14 — MCP server (verifiable agent memory)

## Goal

Ship `valori-mcp`: a Model Context Protocol server that exposes a Valori node as
verifiable, deterministic long-term memory for agents. This opens the
**agent-memory / MCP distribution wedge** — the differentiator vs mem0, Zep,
Letta and the vector DBs is that `memory_recall` returns a cryptographic
**receipt** proving exactly what was retrieved against the committed state hash,
verifiable offline by any client.

## Delivered

New crate `crates/valori-mcp` (added to workspace `members` + `default-members`).
It is a thin async HTTP client over endpoints that already exist — it adds no new
server logic, it only composes (recall = search + proof → receipt).

| File | Contains |
|---|---|
| `src/protocol.rs` | JSON-RPC 2.0 envelope (`Request`/`Response`/`ErrorObject`), error codes. Transport-agnostic, pure. |
| `src/receipt.rs` | `Receipt`/`ReceiptBody`/`ResultFingerprint` + `compute_digest` — `BLAKE3(canonical_json(body))`. The wedge. |
| `src/backend.rs` | `NodeClient` trait + `HttpBackend` (reqwest). 8 primitive node ops. |
| `src/tools.rs` | The six MCP tools + `call_tool` dispatch. Recall assembles the receipt. |
| `src/mcp.rs` | MCP method dispatch: `initialize`, `tools/list`, `tools/call`, `ping`. Protocol `2024-11-05`. |
| `src/stdio.rs` | Newline-delimited JSON-RPC over stdio (`handle_line` + `serve`). |
| `src/main.rs` | `valori-mcp` binary. `--url`/`VALORI_URL`, `--auth-token`/`VALORI_AUTH_TOKEN`. |

### The six tools

| Tool | Backed by | Wedge |
|---|---|---|
| `memory_write` | `POST /v1/memory/upsert_vector` | every write BLAKE3-chained; `text` folded into metadata |
| `memory_recall` | `memory/search_vector` + `proof/state` + `proof/event-log` | **returns a signed receipt** |
| `memory_why` | `GET /graph/subgraph` | provenance neighbourhood — "why is this held?" |
| `memory_timeline` | `GET /v1/timeline` | auditable memory history |
| `memory_forget` | `DELETE /v1/crypto/shred/:key_id` | certified erasure (GDPR) |
| `memory_fork` | `POST /v1/snapshot/save` | deterministic fork point |

### The receipt (verification contract)

`memory_recall` returns `{ results, receipt }` where the receipt body binds:
`state_hash` (kernel Merkle root) + `event_log_hash` + `committed_height` +
`query_dim` + `k` + ordered result fingerprints (`memory_id`, `record_id`,
`score_bits` as raw f64 bits). `receipt_digest = BLAKE3(serde_json(body))`.

Any client reconstructs the body in declaration order, serializes compact,
hashes, and compares. The Python example does exactly this in a different
language and matches byte-for-byte.

### Example + integration

- `examples/mcp_agent_memory.py` — starts a node, spawns `valori-mcp`, runs the
  full MCP handshake, writes 3 memories, recalls, and **re-derives the receipt
  digest in Python** to prove cross-language offline verification.
- `examples/claude_desktop_config.json` — copy-paste MCP client config.

## Findings

- **stdout is the protocol channel.** Any diagnostic print to stdout corrupts
  the JSON-RPC stream and breaks the client. All logs go to stderr; `serve()`
  enforces one compact JSON line per message (no embedded newlines).
- **Tool errors ≠ transport errors.** Per MCP, a failed tool returns a *result*
  with `isError: true` (so the model sees the failure) — only protocol-level
  problems (unknown method, missing `name`) are JSON-RPC errors.
- **Receipt must hash the body, not the wall clock.** `recalled_at_unix` is
  advisory and deliberately excluded from the digest; the cryptographic anchor
  is the committed state, which is reproducible. A test pins this.
- **Score must be hashed as raw bits.** Float formatting drifts across
  languages/platforms; `score_bits` (the f64 bit pattern as a decimal string)
  makes the digest exact and portable — confirmed by the Python verifier.
- **Event-log proof is optional.** Against an in-memory node (`/v1/proof/event-log`
  returns 400) the receipt degrades gracefully to a state-hash-only proof rather
  than failing the recall.

## Validation

```
cargo test -p valori-mcp
28 passing  (25 lib unit + 3 integration), 0 failing

cargo test -p valori-kernel -p valori-node
229 passing, 0 failing   (unchanged — new crate is additive)
```

Integration test `recall_receipt_verifies_against_live_node` boots a **real**
in-process `valori-node` with an event log, drives the MCP tools, then
independently recomputes the digest from the returned receipt and asserts it
matches — the verification contract proven against live proof endpoints.

Manual end-to-end:
```
cargo build -p valori-node -p valori-mcp
pip install blake3
python3 examples/mcp_agent_memory.py
# → cross-language receipt verification: PASS — digest matches
```

## Follow-ups

- **Built-in embedding adapter** — v0 tools take a pre-computed `vector`. A
  follow-up adds an optional embedding step (logged, per the determinism
  invariant) so agents can write raw `text`. Owner: next agent-memory phase.
- **SSE transport** — v0 is stdio only (the Claude Desktop path). Add the HTTP/SSE
  binding for remote/hosted MCP. Owner: agent-memory phase 2.
- **`memory_recall` ef/consistency knobs** — expose `consistency=linearizable`
  and HNSW `ef` (Phase 3.13) through the recall tool.
- **Publish `@valori/mcp` npx wrapper** + submit to the MCP registry (GTM, not code).
