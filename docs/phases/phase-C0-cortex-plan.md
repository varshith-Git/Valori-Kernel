# Valori Cortex — Converged Build Plan

**Status:** Active  
**Branch:** `multinode`  
**Convergence method:** 5-cycle contradiction loop (Cycles 0–4, items 1–34). No surviving M/I/D/S breaks remain.

---

## What "convergence" means here

This plan was derived by proposing an implementation, then systematically contradicting it on four axes until no remaining contradiction breaks any of them:

- **M — Measurable**: every quality claim is measured on Valori's own golden eval, never a borrowed vendor number
- **I — Invariant-safe**: determinism (replay logged LLM output, never re-invoke), auditability (mutations are committed events in the BLAKE3 chain), replication (through Raft)
- **D — Differentiated**: does something vector DB competitors structurally cannot
- **S — Shippable**: each phase ships and is measured independently

**What "100%" means:** the plan has no surviving architectural hole, no broken invariant, and every quality claim is attached to a measurement. The actual recall numbers are produced by C0 — they are not promised here.

---

## The four-point moat

These are what Qdrant, Pinecone, and Chroma structurally cannot replicate:

| Claim | Why competitors can't copy it |
|---|---|
| **The context sentence that shaped an embedding is the one that was committed** | They have no append-only audited event log. Metadata can be silently mutated. |
| **Every graph edge is a committed, hashed event — the answer receipt carries a verifiable provenance subgraph** | Metadata stores in vector DBs are mutable key/value — no per-edge commit hash exists |
| **Full knowledge base is bit-reproducible from the event log** | Requires deterministic Q16.16 arithmetic + append-only log + LLM output logged at ingest, never re-invoked |
| **GDPR erasure certificates are hashed into the same chain as the data** | Deletion in a vector DB is a side-channel operation, not a first-class audited event |

Every Cortex feature must deepen at least one of these four, or it's competing on the wrong axis.

---

## Critical invariants (from contradiction loop)

1. **LLM output is logged, not replayed.** Context sentences and entity extractions go into `AutoInsertRecord.metadata` (the committed event). On WAL replay or snapshot restore, the kernel reads from the log — it never calls the LLM again.

2. **`AutoInsertRecord.metadata` IS in the BLAKE3 audit chain.** Confirmed: `AuditSink::record()` serializes the complete `KernelEvent` including `metadata: Option<Vec<u8>>` into `LogEntry::Event(event.clone())`. The bytes are hashed.

3. **Cluster ingest gap (C1 blocking dependency).** `cluster_server.rs:585` passes `metadata: None`. Any enrichment written in standalone mode is silently lost in cluster mode. This must be fixed in C1 before enrichment is wired end-to-end.

4. **Re-embed is a new event, not history rewrite.** If a model changes and text is re-embedded: commit `SoftDeleteRecord(old)` + `AutoInsertRecord(new)`. Both records remain in the audit chain. History is never rewritten.

5. **Reranker is non-deterministic and documented as such.** Tier-2 reranker runs at read time in the Next.js API route, outside the kernel. Its output goes into the receipt with `rerank_score: number | null` and a header flag. The determinism guarantee applies only to the kernel state, not to answer ranking.

6. **Exact dedup uses bit-identical Q16.16 vectors.** Q16.16 arithmetic is deterministic; two records with identical text embedded by the same model at the same weights produce bit-identical vectors. Semantic dedup (near-duplicates) goes to the human review queue.

7. **Receipt schema is frozen at v1.** The `version: "1.0"` field in `AnswerReceipt` marks the frozen schema. Any breaking change bumps the version. Eval golden set receipts are validated against the live schema version.

---

## Phase C0 — Eval harness *(prerequisite, CI gate)*

**What:** Build the infrastructure to measure Valori's actual retrieval quality.

**Deliverables:**
- `scripts/eval/eval.py` — CLI with three subcommands:
  - `probe`: health + citation sanity check, no embedding needed
  - `seed-eval`: seeds test data, embeds, searches, measures recall@k + provenance integrity
  - `verify`: given saved receipt JSON files, verifies `content_sha256` against live node
- `scripts/eval/qa_sets/bootstrap.jsonl` — 10-entry synthetic QA set for bootstrap testing
- `scripts/eval/requirements.txt`

**Hard CI gates (deterministic):**
- `recall@1 >= 0.8` on seeded data (same text searched = it must be in top-1)
- `citation_existence = 1.0` (every result ID must be real)
- `provenance_integrity >= 1.0` on seeded data (SHA-256 of fetched text is stable)

**Soft metrics (reported with variance, never CI gates):**
- Faithfulness (LLM judge, optional, future phase)

**Bootstrap vs production corpus:** bootstrap numbers are labeled `[bootstrap]` everywhere. They prove the system works; they do not prove quality on the user's real corpus. That measurement happens when the real corpus is available.

**Gate to advance to C1:** reproducible baseline numbers exist, eval runs in CI, all hard gates pass.

---

## Phase C1 — Contextual retrieval + audited enrichment

**What:** LLM-generated context sentence stored in the committed `AutoInsertRecord.metadata` — audited, replicated, replayable.

**Core Rust (small — one gap fix):**
- `cluster_server.rs` batch-ingest: forward `metadata` from request body into `AutoInsertRecord`. Remove `metadata: None`. This is the blocking dependency from the contradiction loop.

**Next.js / UI:**
- Enrichment step in `/api/ingest/route.ts`: after chunking, batch-call LLM for all chunks → collect context sentences → pack as `[doc=<title>, chunk=<n>/<total>] <context sentence>` in UTF-8 into `metadata` field
- Format uses document title + chunk position (both known pre-commit) — NOT Valori record IDs (assigned at apply time)
- Degraded mode: if LLM unavailable or API key missing, commit with `metadata: None` — unenhanced but still valid and audited
- Tier-2 reranker: runs in `/api/why/route.ts` after search, before receipt finalization; adds `rerank_score: number | null` to each chunk in the receipt
- Settings: per-collection toggle for contextual enrichment; reranker provider selection

**Gate:** `recall@5` ↑ vs C0 baseline on bootstrap corpus (labeled `[bootstrap]`). Cluster and standalone audit chains contain identical metadata bytes for the same ingest. Provenance integrity passes.

---

## Phase C2 — Audited entity graph + provenance receipt

**What:** Entity extraction → committed graph events → bounded BFS at query time → provenance subgraph in answer receipt.

**Core Rust (one new endpoint):**
- `GET /v1/namespaces/:ns/graph/subgraph?root=<node_id>&depth=<n>&kinds=<edge_kinds>`
  - Bounded BFS: depth ≤ 3, node cap 50
  - Traverses only `Mentions`, `RefersTo`, `ParentOf` edges (not generic `Relation`)
  - Returns ordered edge list: `(from_node, to_node, edge_kind, committed_event_index)`
  - Wire in **both** `server.rs` and `cluster_server.rs` per CLAUDE.md invariant

**Next.js / UI:**
- Entity extraction in `/api/ingest/route.ts`: LLM extracts entities → commit as `AutoCreateNode { kind: Concept }` + `AutoCreateEdge { kind: Mentions }` per entity
- `NodeKind::Concept` and `EdgeKind::Mentions` already exist in the kernel — no new variants needed
- `/api/why/route.ts`: after chunk retrieval, call `/subgraph` for top chunks → merge subgraphs → include in receipt as `graph_chunks[]` with per-edge `committed_event_index`

**Gate:** Global-question score ↑ (questions requiring cross-document reasoning). Provenance subgraph in receipt is verifiable: each `committed_event_index` maps to a real log entry.

---

## Phase C3 — Self-maintaining memory

**What:** Auto-tombstone exact duplicates; surface semantic merges and contradiction resolutions for human review; supersession chains via graph.

**Core Rust:** None new. Uses existing `SoftDeleteRecord`, `AutoInsertRecord`, `AutoCreateEdge { kind: RefersTo }`.

**Next.js / UI:**
- Background job (on-demand or scheduled): scan namespace for bit-identical Q16.16 vectors → auto-tombstone via `SoftDeleteRecord` committed event (safe: bit-identical = provably same content)
- Contradiction detector upgrade: replace negated-vector heuristic with NLI call → verdict logged as `metadata` on a `RefersTo` edge event → surfaces in ContradictionTab as human-review queue
- Merge flow: user approves → `SoftDeleteRecord(old)` + `AutoInsertRecord(merged)` + `AutoCreateEdge { kind: RefersTo }` linking to both parents — full provenance preserved

**Gate:** Dedup rate measurable on corpus with known duplicates. No silent history mutation in any code path.

---

## Deferred (not in Cortex scope)

- Leiden community detection — stochastic, not clearly needed before eval proves local graph insufficient for global questions
- Faithfulness LLM judge as a CI gate — too flaky; kept as a soft metric with variance reported
- Cross-encoder reranker on-premise — left as a provider selection in settings

---

## Key code locations for Cortex implementation

| Task | File |
|---|---|
| Fix cluster metadata gap | `crates/valori-node/src/cluster_server.rs:585` |
| Ingest enrichment pipeline | `ui/src/app/api/ingest/route.ts` |
| Reranker + provenance receipt | `ui/src/app/api/why/route.ts` |
| Subgraph endpoint (Rust) | `crates/valori-node/src/server.rs` + `cluster_server.rs` |
| Receipt schema (frozen v1) | `ui/src/lib/receipts.ts` |
| Eval harness | `scripts/eval/eval.py` |
