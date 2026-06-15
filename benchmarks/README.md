# Valori benchmark suite

Built in direct response to the 2026-06-12 review (Mayur, Rahul). Three asks, three answers:

| Ask from the meeting | Where it's answered |
|---|---|
| "Separate the variables — fixed-point vs float, and vector-only vs vector+graph" (Rahul, Mayur) | Three-arm design below; A vs B isolates the math, B vs C isolates the graph |
| "Prioritize difficult multi-statement questions" (aligned decision) | 6 single-hop controls vs 6 cross-document multi-statement questions, gold-labeled |
| "Show me a concrete example where float error causes a wrong result, not minor variance" (Mayur) | Section 3 of RESULTS.md — a ranking flip caused purely by summation order |

## Run it

```bash
python3 benchmarks/run_benchmark.py     # from repo root; writes benchmarks/RESULTS.md
```

No network needed. Model: `all-MiniLM-L6-v2` (same as the meeting demo), 384-dim, local.

## Design

### Three arms — variables isolated

| Arm | Math | Retrieval | What it represents |
|---|---|---|---|
| A | float32 | vector-only, brute-force cosine | the Pinecone/Qdrant class. Brute-force is *deliberate*: it's the exact dot-product math those engines run, with no ANN/index/network noise for anyone to attribute results to |
| B | Q16.16 fixed point (Valori kernel) | vector-only | changes ONE variable vs A: the arithmetic |
| C | Q16.16 fixed point (Valori kernel) | vector + built-in graph | changes ONE variable vs B: the graph |

All three arms receive **bit-identical embeddings** from a shared cache — the only differences are the ones under test.

### Corpus

10 fictional documents from "Meridian Capital" (incident report, trade blotter, risk policy,
compliance memo, engineering postmortem, client onboarding, committee minutes, staffing,
systems inventory, audit findings) — 23 chunks, 10 entities. Facts are deliberately spread
across documents so multi-statement questions cannot be answered from any single chunk.
Gold labels are the set of chunks required for a *complete* answer.

### Metrics

- **Recall@5** — fraction of gold chunks in the top 5.
- **Complete-context rate** — % of questions where *all* gold chunks are in the top 5.
  This is the metric that matters for multi-statement questions: an LLM answering from
  partial context produces a confident half-answer.

### Hybrid retrieval — the exact mechanism (Rahul's question)

How does the system decide which edges matter? One fixed formula, no per-question tuning:

```
score(chunk) = cosine(query, chunk) + 0.08 × min(paths, 3)
```

`paths` counts distinct graph connections from the candidate chunk to the top-3 vector
**entry points**, read from the kernel graph (`get_edges`):

- **+1 per shared Concept node** — the candidate co-mentions an entity with an entry chunk
  (`Chunk —Mentions→ Concept ←Mentions— Chunk`)
- **+1 for same document** — sibling chunk of an entry (`Chunk —RefersTo→ Document`)
- **+1 if the candidate is itself an entry** — its own retrieval is a path

So edge *kinds* are not weighted by guesswork; relevance is decided by **how many
independent routes** connect a candidate to what the vector search already trusts.
One shared entity is weak evidence; three independent connections is strong evidence.

#### Worked example (from the actual run)

Question: *"Which client's orders could have been affected by the March order routing
incident, and why?"* — note it names **no client, no desk, no incident ID**.

- Pure vector search (arms A and B): the answer chunk "position reports for desk EU-7
  were misstated" ranks **#13** — its wording shares almost nothing with the question.
  Result: incomplete context (50% recall).
- Hybrid (arm C): entry points are the failover postmortem, the incident summary, and
  the ACME routing chunk. The "EU-7 misstated" chunk connects by **3 paths** —
  shared Concept `INC-2207`, shared Concept `EU-7` (which the ACME chunk also mentions),
  and same-document sibling of an entry. Bonus +0.24 lifts it from #13 into the top 5.
  Result: complete context (100%), assembled across three documents.

The graph contributed exactly what vectors cannot: *relationship* evidence
("ACME routes through EU-7; EU-7 was misreported") when *similarity* evidence is absent.

## Honest limitations (say these before they're asked)

- The corpus is small and synthetic. The claim demonstrated is *mechanistic* (graph
  paths recover cross-document context that cosine similarity misses by construction),
  not a leaderboard claim. Scaling to a public benchmark (e.g. MultiHop-RAG) is the
  natural next step.
- The ranking-flip example in section 3 is a constructed near-tie (true gap ~1e-9,
  below float32 reduction noise at 384 dims). On a 23-chunk corpus natural near-ties
  are rare; at 10M+ vectors they are a statistical certainty. The measured fact that
  **100% of query-document scores differ at the bit level across summation orders** is
  from the real corpus, unconstructed.
- Arm A uses brute-force cosine rather than a live Pinecone index. This is a *stronger*
  baseline than Pinecone for quality (exact search beats ANN) and removes every
  confounder Pinecone's infra would introduce. A live-Pinecone arm can be added with an
  API key; it can only do worse than arm A on recall.
- Arm C's formula (0.08 × ≤3 paths) was fixed once, globally — but it was chosen while
  looking at this corpus. Validation on held-out questions is required before quoting
  the numbers outside the team.
