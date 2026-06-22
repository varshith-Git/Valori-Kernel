# Phase C0 — Eval Harness

## Goal

Build the measurement infrastructure that gates all Cortex quality claims. Every
retrieval improvement in C1–C3 must produce a number from this harness — never a
borrowed vendor benchmark.

## Delivered

### `scripts/eval/eval.py`

CLI with three subcommands:

| Subcommand | Purpose | Requires embedding? |
|---|---|---|
| `probe` | Health + metadata reachability check | No |
| `seed-eval` | Seeds 10 records, embeds queries, measures recall@k + provenance integrity | Yes |
| `verify` | Verifies `content_sha256` in saved receipt JSON files against live node | No |

**CI gate (seed-eval):**
- `recall@1 >= 0.8` — exits 1 if below threshold (configurable via `--recall-threshold`)
- `citation_existence = 1.0` — exits 1 if any result ID is not in the seeded set

**Metrics produced:**
- `recall@1`, `recall@5` — hard CI gates
- `citation_existence` — hard CI gate
- `provenance_integrity` — SHA-256 stability of fetched metadata across two reads; soft (None when metadata absent)
- Faithfulness (LLM judge) — deferred to C1 when real corpus exists

### `scripts/eval/qa_sets/bootstrap.jsonl`

10 QA entries covering Valori's own technical facts. Fields:
- `question` — the query text
- `expected_facts` — list of strings a correct answer should contain (for future faithfulness judging)
- `category` — `factual | multi-hop | reasoning`

No `gold_record_ids` — those are instance-specific and populated by `seed-eval`'s own seeded IDs.

### `scripts/eval/requirements.txt`

Single dependency: `httpx>=0.27`. No heavy ML deps — the harness hits the
Valori node and optionally an embedding API directly.

### `ui/src/lib/receipts.ts`

Schema freeze comment added. `version: "1.0"` is the frozen v1 marker.
Breaking changes must bump `RECEIPT_VERSION` and document migration path.

### `docs/phases/phase-C0-cortex-plan.md`

Full converged Cortex plan (5 contradiction cycles, 34 items). Gates each
of C0–C3. Referenced by this doc.

## Findings

1. **`/timeline` returns plain-text strings**, not structured JSON. The namespace-audit
   logic lives in the Next.js proxy (`/api/namespace-audit`), not the Rust node.
   The eval harness avoids this: it uses its own seeded record IDs for citation checks,
   not a namespace scan.

2. **`cluster_server.rs:585` passes `metadata: None`** — the cluster ingest path silently
   drops all metadata. This is logged as the blocking dependency for C1 and does not
   affect C0 (the eval harness uses standalone mode).

3. **`NodeKind::Concept` and `EdgeKind::Mentions`/`RefersTo` already exist** — no new
   kernel variants needed for C2 entity graph. Confirmed during C0 research.

4. **BLAKE3 audit chain does cover `metadata` bytes** — `AuditSink::record()` serializes
   the full `KernelEvent` including `metadata: Option<Vec<u8>>`. The chain is sound.

## Validation

```bash
python3 -c "import ast; ast.parse(open('scripts/eval/eval.py').read()); print('OK')"
# → syntax OK

python3 scripts/eval/eval.py --help
# → shows probe / seed-eval / verify subcommands

# End-to-end (requires running node + ollama with nomic-embed-text):
python3 scripts/eval/eval.py probe --url http://localhost:3000
python3 scripts/eval/eval.py seed-eval \
    --url http://localhost:3000 \
    --embed-provider ollama --embed-model nomic-embed-text
```

Test suite: `cargo test -p valori-kernel -p valori-node` → 198 passed, 0 failed (unchanged from B13).

## Follow-ups

| Item | Phase |
|---|---|
| Fix `cluster_server.rs` `metadata: None` gap | **C1 blocking dependency** |
| Add contextual enrichment to ingest pipeline | C1 |
| Add per-collection toggle for enrichment | C1 |
| Add Tier-2 reranker + `rerank_score` in receipt | C1 |
| Add `/v1/namespaces/:ns/graph/subgraph` endpoint | C2 |
| Add entity extraction at ingest time | C2 |
| Provenance subgraph in answer receipt | C2 |
| Exact-dedup auto-tombstone | C3 |
| NLI-based contradiction review queue | C3 |
| Supersession chains via `RefersTo` edge | C3 |
| Replace bootstrap corpus with real target corpus | When corpus is named |
