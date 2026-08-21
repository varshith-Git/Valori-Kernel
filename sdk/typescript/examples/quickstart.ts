// Copyright (c) 2025 Varshith Gudur. Licensed under MIT OR Apache-2.0.
//
// End-to-end tour of the ergonomic layer against a live node.
//
//   cargo build -p valori-node --release
//   VALORI_DIM=8 ./target/release/valori-node &
//   npx tsx examples/quickstart.ts
//
// Every call here is a handwritten wrapper — nothing reaches for `client.raw`.

import { NotFoundError, ValoriClient } from "../src/index.js";

const DIM = Number(process.env.VALORI_TEST_DIM ?? 8);
const COLLECTION = "quickstart-demo";

/** A deterministic unit-ish vector, so runs are reproducible. */
const vec = (seed: number): number[] =>
  Array.from({ length: DIM }, (_, i) => ((seed + i) % 10) / 10);

async function main(): Promise<void> {
  const client = new ValoriClient({
    endpoint: process.env.VALORI_ENDPOINT ?? "http://127.0.0.1:3000",
    apiKey: process.env.VALORI_API_KEY,
  });

  const health = await client.health();
  console.log(`node: ${health.status} · mode=${health.mode} · version=${health.version}`);
  console.log(`client: ${client}`); // the API key renders as ***

  // ── collections ───────────────────────────────────────────────────────────
  // dimension + metric are always required; nothing is created implicitly.
  // Catch the supertype: `DELETE /v1/namespaces/{name}` answers with the generic
  // `not_found` code, not the more specific `collection_not_found` the enum also
  // defines, so `CollectionNotFoundError` alone would not match here.
  try {
    await client.collections.delete(COLLECTION);
  } catch (err) {
    if (!(err instanceof NotFoundError)) throw err;
  }

  const docs = await client.collections.create(COLLECTION, {
    dimension: DIM,
    metric: "squared_l2",
  });
  console.log(`created collection ${docs.name}`);

  // ── records ───────────────────────────────────────────────────────────────
  // requestId makes the write dedupable on the node and retryable in the SDK.
  const inserted = await docs.records.insert(vec(1), {
    text: "Section 3.1 Training — AdamW optimizer, lr 3e-4",
    metadata: { author: "Alice", year: 2024 },
    requestId: crypto.randomUUID(),
  });
  console.log(`inserted record #${inserted.id} (deduplicated=${inserted.deduplicated})`);

  await docs.records.insertBatch([vec(2), vec(3)], {
    texts: ["Section 3.2 Data — 1.4T tokens", "Section 4 Eval — MMLU 71.2"],
  });

  // ── search ────────────────────────────────────────────────────────────────
  // camelCase in; the wire body carries snake_case.
  const hits = await docs.search(
    vec(1),
    3,
    // metadataFilter is deliberately not used here — see the note in README.md.
    { queryText: "what optimizer is used?" },
    { timeoutMs: 10_000 },
  );
  console.log(`search returned ${hits.results?.length ?? 0} hit(s)`);

  // ── graph ─────────────────────────────────────────────────────────────────
  const doc = await docs.graph.createNode(1, inserted.id);
  const chunk = await docs.graph.createNode(2, inserted.id);
  await docs.graph.createEdge(doc.node_id, chunk.node_id, 1);

  for await (const node of docs.graph.listAllNodes({ pageSize: 50 })) {
    console.log(`  graph node ${node.node_id} (kind=${node.kind})`);
  }

  const sub = await docs.graph.subgraph(doc.node_id, 2);
  console.log(`subgraph: ${sub.nodes?.length ?? 0} nodes, ${sub.edges?.length ?? 0} edges`);

  // ── graphrag ──────────────────────────────────────────────────────────────
  const rag = await docs.graphrag(vec(1), { k: 3, depth: 2 });
  console.log(`graphrag: ${rag.hits?.length ?? 0} hit(s)`);

  // ── proof ─────────────────────────────────────────────────────────────────
  const proof = await client.proof.state();
  console.log(`state hash: ${proof.final_state_hash}`);

  await client.collections.delete(COLLECTION);
  console.log("done");
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
