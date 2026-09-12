# RAG demo (V1) — design spec

## Purpose

Build the first of five use-case demos referenced on the landing page's Use
Cases grid (RAG, Recommendation Systems, AI Agents, Advanced Search, Data
Analysis & Anomaly Detection). RAG goes first. Its shell — Scenario → Query →
Valori operation → Explainable result → Verification → Replay → Copyable code
— is the reusable template the other four demos will follow.

## Product decision: read-only, pre-seeded, single-writer

V1 does **not** accept visitor-uploaded documents. Reasons:

- The kernel's state hash / WAL sequence (`crates/valori-kernel/src/proof.rs`)
  is computed over the whole process, not per-namespace. A namespace isolates
  *retrieval* but not the state root — another visitor's concurrent write
  would move `final_state_hash` out from under an in-progress verification,
  undermining the exact thing the demo is trying to prove.
- Public uploads add privacy, abuse, storage-cleanup, and prompt-injection
  surface for no product benefit in a first demo.
- A single pinned dataset lets the "verify identical results" story actually
  hold: the same query returns the same evidence, indefinitely, because
  nothing else is writing to the project.

Visitor-provided documents are Phase 2 (see below), gated on either an
ephemeral per-session project/node or collection-scoped proofs — a namespace
alone is not sufficient isolation for that use case.

## Dataset

One dedicated Valori Cloud project, `public-demo-rag`, seeded once with:
- `content/docs/*.mdx` (Valori's own documentation)
- The arXiv paper (2512.22280) text

No second/neutral corpus in V1 — keeps scope tight, and letting visitors ask
real product questions is more useful than proving retrieval on unrelated
text.

**Reseed policy:** the dataset is versioned and pinned, not auto-synced.
Ingest is a deliberate, manual, one-time (or explicitly-repeated) operation.
Each ingest run is labeled with a `dataset_version` string (e.g.
`2026-09-12-docs-v1`) stored alongside the demo project config and shown in
the UI's verification drawer. Re-seeding to reflect updated docs means a
new `dataset_version` and a fresh state root — it never happens silently
under a visitor mid-session. There is no CI job that re-ingests on doc
changes.

Embed provider: cheap embedding model (e.g. `text-embedding-3-small`).
Generation model: cheap LLM (e.g. `gpt-4o-mini`). Both configured server-side
only (env vars on the Cloud project / demo API routes); never sent to or
held by the browser.

## Architectural split: retrieval vs. generation

These are presented and implemented as two distinct operations, matching two
different trust levels:

| Layer | Claim |
|---|---|
| Valori retrieval | Deterministic, replayable, verifiable (real state root / WAL sequence / result ordering) |
| LLM generation | Grounded by the retrieved context, but explicitly **not** claimed to be deterministic |

The UI never blends these into one "Valori answered this" claim. The answer
panel is visually and structurally subordinate to the evidence panel: the
evidence existed before the LLM touched it, and is checkable independently
of whatever the LLM said.

## API shape

All routes are public, unauthenticated, and read-only against the one
pre-seeded project (no path accepts arbitrary project ids or write payloads).

```
GET  /api/demo/rag/dataset       -- dataset_version, doc list, chunk count, seeded_at
GET  /api/demo/rag/examples      -- curated list of suggested questions (static/cached)
POST /api/demo/rag/query         -- retrieval only (vector / hybrid / graphrag)
POST /api/demo/rag/generate      -- generation, given a query_id or explicit evidence
POST /api/demo/rag/replay        -- re-runs a prior query_id's retrieval, compares results
```

`POST /api/demo/rag/query` request:
```json
{
  "question": "How does Valori guarantee determinism?",
  "mode": "vector" | "hybrid" | "graph_rag",
  "k": 5,
  "graph": {                          // only when mode = graph_rag
    "depth": 2,
    "edge_kinds": [0, 1],
    "reverse_parent_of": true,
    "graph_weight": 0.3
  }
}
```

`POST /api/demo/rag/query` response (fields are real, not decorative):
```json
{
  "query_id": "q_...",
  "retrieval": {
    "mode": "graph_rag",
    "results": [
      {
        "record_id": 42,
        "document": "core-concepts.mdx",
        "section": "Fixed-point arithmetic",
        "score": 0.87,
        "rerank_score": 0.91,
        "graph_expanded": true,
        "graph_path": [42, 17],
        "content": "..."
      }
    ],
    "ordered_result_ids": [42, 17, 9],
    "retrieval_latency_ms": 18
  },
  "state": {
    "dataset_version": "2026-09-12-docs-v1",
    "final_state_hash": "<hex, from /v1/proof/state>",
    "wal_committed_height": 412,
    "verified": true
  },
  "replay": { "supported": true }
}
```

`final_state_hash` and `wal_committed_height` are read straight from the
node's own `/v1/proof/state` and `/v1/proof/event-log` — no proof value is
invented for fields the kernel doesn't expose. There is no per-query
result-hash/proof primitive today; the verification story is: same state
root + same ordered result IDs on replay = same evidence, not a
cryptographic proof of that specific query's result set. The verification
drawer states this plainly rather than implying a stronger guarantee.

`POST /api/demo/rag/generate` request: `{ "query_id": "q_..." }` (re-embeds
nothing; re-fetches the evidence for that query_id from `/api/demo/rag/query`'s
result). Response: `{ "answer": "...", "citations": [42, 17], "model": "gpt-4o-mini", "generation_latency_ms": 640 }`.

`POST /api/demo/rag/replay` request: `{ "query_id": "q_..." }`. Re-runs the
same retrieval call and diffs `ordered_result_ids` + `final_state_hash`
against the original; returns `{ "identical": true, "original": {...}, "replay": {...} }`.

## Guardrails

- Per-IP and per-session rate limits on all five routes.
- Max question length (e.g. 500 chars) — no document-sized input accepted
  anywhere in this surface.
- Retrieval-result cap (`k` clamped, e.g. max 10).
- LLM call timeout + client-side request cancellation.
- System prompt instructs the LLM to answer only from provided chunks, never
  follow instructions embedded in retrieved content, and never call tools —
  no tool/function-calling is wired to the LLM here at all.
- Suggested questions and their retrieval+generation results are precomputed
  and cached (served instantly, zero marginal LLM/embed cost for the common
  path); free-text questions still hit the live path under the rate limit.
- Cost/latency telemetry logged server-side per request.
- Embed/LLM credentials read from server env only, never accepted from or
  echoed to the client.

## Page structure — `/solutions/rag`

**Hero** — "Ask Valori's documentation. Inspect every piece of evidence
behind the answer." Actions: try a suggested question, enter your own, or
jump to "how it works".

**Query workspace** — question input; retrieval mode (Vector / Hybrid /
GraphRAG); result count; graph depth; edge-kind filters; reverse-parent-of
toggle. Controls are live — visible even before an answer exists, so GraphRAG
capability isn't hidden behind a black-box answer box.

**Answer panel** — generated response, inline citations linking to evidence
entries, model name, retrieval/generation/total latency. Marked clearly as
LLM-generated, not a Valori guarantee.

**Evidence panel** — per chunk: document, section, similarity score, rerank
score, graph-expansion indicator + path, chunk content, record ID.

**Verification drawer** — real values only: `dataset_version`,
`final_state_hash`, `wal_committed_height`, ordered result IDs, a
"Replay" button that calls `/api/demo/rag/replay` and shows the diff
(or confirmed-identical). No decorative checkmarks for properties the API
doesn't actually return.

**Code panel** — cURL / Python / TypeScript for the retrieval call
(`/api/demo/rag/query` equivalent against a real project, i.e. `POST
/v1/graphrag` or `/v1/search`). Generation is shown as a separate,
clearly-labeled second call — never presented as one Valori API request.

Linked from the RAG card in the existing Use Cases grid ("Try it live →").

## Reusable template for the remaining 4 demos

```
Scenario → Query → Valori operation → Explainable result → Verification → Replay → Copyable code
```

Advanced Search, Recommendation Systems, Anomaly Detection, and AI Agents
each get their own pre-seeded read-only dataset/project and reuse this same
shell and the same query/verify/replay route split.

## Phase 2 (explicitly out of scope for V1)

Visitor-provided documents, gated on:
- An ephemeral project or node per session (not just a namespace) — or
  collection-scoped proofs added to the kernel, if that lands first.
- Automatic expiry + explicit delete.
- Storage quota, file-type validation.
- No cross-session state.
- Privacy notice on any pasted/uploaded content.
- Mutation and cleanup auditing.

## Out of scope / not building in this spec

- Any change to `valori-kernel`, `valori-node`, or the Rust workspace.
- Any change to `@valori/studio` or the dashboard `PlaygroundView`.
- The other four use-case demos (design each separately once this shell is
  validated).
