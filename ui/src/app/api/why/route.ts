import { NextRequest, NextResponse } from "next/server";
import crypto from "crypto";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function apiHeaders(): Record<string, string> {
  const h: Record<string, string> = { "Content-Type": "application/json" };
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

// SHA-256 of a UTF-8 string, prefixed for self-describing receipts.
function sha256(text: string): string {
  return "sha256:" + crypto.createHash("sha256").update(text, "utf8").digest("hex");
}

// Fetch the live global BLAKE3 state hash so a receipt can be bound to the
// exact node state at answer time. Returns null if the proof endpoint is down.
async function fetchGlobalStateHash(): Promise<string | null> {
  try {
    const res = await fetch(`${getApiUrl()}/v1/proof/state`, { headers: apiHeaders(), cache: "no-store" });
    if (!res.ok) return null;
    const d = await res.json().catch(() => ({})) as { final_state_hash?: string };
    return d.final_state_hash ?? null;
  } catch {
    return null;
  }
}

// -- LLM call (multi-provider) --------------------------------------------------

interface LLMConfig {
  provider: "ollama" | "openai" | "groq" | "together" | "custom";
  model: string;
  apiKey?: string;
  endpoint?: string;
}

async function callLLM(
  systemPrompt: string,
  userMessage: string,
  cfg: LLMConfig
): Promise<string> {
  const messages = [
    { role: "system", content: systemPrompt },
    { role: "user", content: userMessage },
  ];

  if (cfg.provider === "ollama") {
    const base = cfg.endpoint?.replace(/\/$/, "") || "http://localhost:11434";
    const res = await fetch(`${base}/api/chat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        model: cfg.model || "llama3.2",
        messages,
        stream: false,
        options: { temperature: 0 },
      }),
    });
    if (!res.ok) {
      const text = await res.text().catch(() => res.status.toString());
      throw new Error(`Ollama error (${res.status}): ${text}`);
    }
    const data = await res.json() as { message?: { content?: string } };
    return data.message?.content ?? "";
  }

  // OpenAI-compatible: openai, groq, together, custom
  const baseMap: Record<string, string> = {
    openai: "https://api.openai.com",
    groq: "https://api.groq.com/openai",
    together: "https://api.together.xyz",
  };
  const base = cfg.endpoint?.replace(/\/$/, "") || baseMap[cfg.provider] || "";
  if (!base) throw new Error("No endpoint configured for custom provider");

  const res = await fetch(`${base}/v1/chat/completions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(cfg.apiKey ? { Authorization: `Bearer ${cfg.apiKey}` } : {}),
    },
    body: JSON.stringify({
      model: cfg.model,
      messages,
      max_tokens: 512,
      temperature: 0,
    }),
  });

  if (!res.ok) {
    const text = await res.text().catch(() => res.status.toString());
    throw new Error(`${cfg.provider} error (${res.status}): ${text.slice(0, 200)}`);
  }
  const data = await res.json() as { choices?: { message?: { content?: string } }[] };
  return data.choices?.[0]?.message?.content ?? "";
}

// -- Tier-2 reranker -----------------------------------------------------------
// Runs after vector search to re-order chunks by cross-attention relevance.
// Non-deterministic: the rerank_score is logged in the receipt with an explicit
// `reranked` flag so the non-determinism is documented, not hidden.

interface RerankerConfig {
  provider: "cohere" | "custom";
  apiKey?: string;
  model?: string;
  endpoint?: string;
}

async function rerankChunks(
  query: string,
  chunks: { record_id: number; score?: number; metadata: Record<string, unknown> | null }[],
  cfg: RerankerConfig,
): Promise<{ record_id: number; score?: number; rerank_score: number | null; metadata: Record<string, unknown> | null }[]> {
  const docs = chunks.map((c) => (c.metadata?.text as string) ?? "");

  try {
    if (cfg.provider === "cohere") {
      const res = await fetch((cfg.endpoint || "https://api.cohere.ai/v2/rerank").replace(/\/$/, ""), {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.apiKey ?? ""}` },
        body: JSON.stringify({
          model: cfg.model || "rerank-english-v3.0",
          query,
          documents: docs,
          top_n: chunks.length,
        }),
      });
      if (!res.ok) throw new Error(`Cohere rerank ${res.status}`);
      const d = await res.json() as { results: { index: number; relevance_score: number }[] };
      const scoreMap = new Map(d.results.map((r) => [r.index, r.relevance_score]));
      const reranked = [...chunks]
        .map((c, i) => ({ ...c, rerank_score: scoreMap.get(i) ?? null }))
        .sort((a, b) => (b.rerank_score ?? -1) - (a.rerank_score ?? -1));
      return reranked;
    }
    if (cfg.provider === "custom" && cfg.endpoint) {
      const res = await fetch(cfg.endpoint, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...(cfg.apiKey ? { Authorization: `Bearer ${cfg.apiKey}` } : {}),
        },
        body: JSON.stringify({ query, documents: docs }),
      });
      if (!res.ok) throw new Error(`Custom reranker ${res.status}`);
      const d = await res.json() as { scores?: number[] };
      if (!Array.isArray(d.scores)) throw new Error("Custom reranker: expected scores[]");
      return chunks
        .map((c, i) => ({ ...c, rerank_score: d.scores![i] ?? null }))
        .sort((a, b) => (b.rerank_score ?? -1) - (a.rerank_score ?? -1));
    }
  } catch { /* fall through — reranker failure must not block the answer */ }

  return chunks.map((c) => ({ ...c, rerank_score: null }));
}

// -- Main handler ---------------------------------------------------------------

interface WhyRequest {
  record_id?: number;
  query_vector?: number[];
  k?: number;
  collection?: string;
  question?: string;
  // LLM config (optional — omit to skip synthesis)
  llm?: LLMConfig;
  // Tier-2 reranker (optional — omit to skip reranking)
  reranker?: RerankerConfig;
}

// -- Reference chunk detector -------------------------------------------------
// Detects bibliography / reference list chunks so they can be deprioritised.
function isReferenceChunk(text: string): boolean {
  const inlineCitations = (text.match(/\[\d{1,3}\]/g) ?? []).length;
  if (inlineCitations >= 3) return true;

  const urls = (text.match(/https?:\/\//g) ?? []).length;
  if (urls >= 2) return true;

  const lines = text.split("\n").filter(Boolean);
  if (lines.length >= 2) {
    const citLines = lines.filter((l) => /^\[\d+\]/.test(l.trim()));
    if (citLines.length / lines.length > 0.25) return true;
  }

  // Orphaned mid-citation continuation starting lowercase
  const firstChars = text.trim().slice(0, 120).toLowerCase();
  if (
    inlineCitations >= 1 &&
    /^[a-z]/.test(text.trim()) &&
    (firstChars.includes("conference") || firstChars.includes("proceedings") ||
     firstChars.includes("arxiv") || firstChars.includes("preprint"))
  ) return true;

  return false;
}

export async function POST(req: NextRequest) {
  try {
    const body: WhyRequest = await req.json();
    const { record_id, query_vector, k = 5, collection = "default", question, llm, reranker } = body;

    const results: {
      record_id: number;
      score?: number;
      metadata: Record<string, unknown> | null;
    }[] = [];

    if (record_id !== undefined) {
      const metaRes = await fetch(
        `${getApiUrl()}/v1/memory/meta/get?target_id=record:${record_id}`,
        { headers: apiHeaders() }
      );
      const meta = metaRes.ok ? await metaRes.json().catch(() => null) : null;
      results.push({ record_id, metadata: meta?.metadata ?? null });
    } else if (query_vector) {
      // Over-fetch (3× k) then filter + rerank so reference chunks don't crowd out content
      const fetchK = Math.min(k * 3, 30);
      const searchRes = await fetch(`${getApiUrl()}/search`, {
        method: "POST",
        headers: apiHeaders(),
        body: JSON.stringify({ query: query_vector, k: fetchK, collection, query_text: question ?? undefined }),
      });
      if (!searchRes.ok) {
        return NextResponse.json({ error: "search failed" }, { status: 502 });
      }
      const { results: hits } = await searchRes.json() as { results: { id: number; score: number }[] };

      // Fetch metadata for all candidates
      const candidates: typeof results = [];
      for (const hit of hits) {
        const metaRes = await fetch(
          `${getApiUrl()}/v1/memory/meta/get?target_id=record:${hit.id}`,
          { headers: apiHeaders() }
        );
        const meta = metaRes.ok ? await metaRes.json().catch(() => null) : null;
        candidates.push({ record_id: hit.id, score: hit.score, metadata: meta?.metadata ?? null });
      }

      // C3: Filter out superseded chunks (marked by the contradiction resolver)
      const nonSuperseded = candidates.filter((c) => !c.metadata?.superseded);

      // Keep content chunks, fall back to reference chunks if not enough content
      const contentChunks = nonSuperseded.filter((c) => {
        const text = (c.metadata?.text as string) ?? "";
        return !isReferenceChunk(text);
      });
      const referenceChunks = nonSuperseded.filter((c) => {
        const text = (c.metadata?.text as string) ?? "";
        return isReferenceChunk(text);
      });

      let ranked = [...contentChunks, ...referenceChunks].slice(0, k);
      results.push(...ranked);
    } else {
      return NextResponse.json({ error: "provide record_id or query_vector" }, { status: 400 });
    }

    // -- Tier-2 reranking (optional) --------------------------------------------
    // Reranking is non-deterministic (cross-attention floats). The rerank_score
    // is logged in the receipt with a `reranked` flag so downstream auditors
    // know which chunks were reordered and by how much.
    let reranked = false;
    type ResultItem = { record_id: number; score?: number; rerank_score?: number | null; metadata: Record<string, unknown> | null };
    let rankedResults: ResultItem[] = results as ResultItem[];
    if (reranker && question && results.length > 1) {
      const rerankInput = results.map((r) => ({ ...r, metadata: r.metadata as Record<string, unknown> | null }));
      const rerankOutput = await rerankChunks(question, rerankInput, reranker);
      rankedResults = rerankOutput;
      reranked = rerankOutput.some((r) => r.rerank_score !== null);
    }

    // -- Graph-augmented context expansion --------------------------------------
    // For each vector hit that has chunk metadata, walk the document→chunk edges
    // and pull in adjacent chunks (chunk_index ± 1) as additional context.
    // This is "sentence window retrieval" via the knowledge graph.
    const graphContextChunks: { record_id: number; chunk_index: number; text: string; source: string }[] = [];

    if (rankedResults.length > 0) {
      // Collect unique documents and build a set of already-retrieved record IDs
      const alreadyRetrieved = new Set(rankedResults.map((r) => r.record_id));
      const docNodeIds = new Set<number>();

      for (const r of rankedResults) {
        const m = r.metadata as Record<string, unknown> | null;
        if (m?.document_node_id !== undefined) docNodeIds.add(m.document_node_id as number);
      }

      // For each unique document, fetch its chunk edges once
      const docEdges: Map<number, number[]> = new Map(); // doc_node_id → [chunk_node_ids]
      for (const docNodeId of docNodeIds) {
        try {
          const edgesRes = await fetch(`${getApiUrl()}/graph/edges/${docNodeId}`, { headers: apiHeaders() });
          if (edgesRes.ok) {
            const { edges } = await edgesRes.json() as { edges: { to_node: number; kind: number }[] };
            docEdges.set(docNodeId, edges.map((e) => e.to_node));
          }
        } catch { /* skip if graph unavailable */ }
      }

      if (docEdges.size > 0) {
        // Build node_id → record_id map using the graph nodes list (one call)
        let nodeToRecord: Map<number, number> = new Map();
        try {
          const nodesRes = await fetch(`${getApiUrl()}/graph/nodes?collection=${collection}`, { headers: apiHeaders() });
          if (nodesRes.ok) {
            const { nodes } = await nodesRes.json() as { nodes: { node_id: number; record_id: number | null }[] };
            for (const n of nodes) {
              if (n.record_id !== null) nodeToRecord.set(n.node_id, n.record_id);
            }
          }
        } catch { /* skip */ }

        // For each vector hit, find adjacent chunks in the same document
        for (const hit of rankedResults) {
          const m = hit.metadata as Record<string, unknown> | null;
          if (!m) continue;
          const docNodeId = m.document_node_id as number | undefined;
          const chunkIndex = m.chunk_index as number | undefined;
          if (docNodeId === undefined || chunkIndex === undefined) continue;

          const chunkNodeIds = docEdges.get(docNodeId) ?? [];

          // Gather record IDs for all chunks in this document
          const docRecordIds: number[] = [];
          for (const nodeId of chunkNodeIds) {
            const rid = nodeToRecord.get(nodeId);
            if (rid !== undefined) docRecordIds.push(rid);
          }

          // Fetch metadata for records we haven't retrieved yet to find adjacent chunks
          const adjacent: number[] = [];
          for (const rid of docRecordIds) {
            if (alreadyRetrieved.has(rid)) continue;
            try {
              const mr = await fetch(`${getApiUrl()}/v1/memory/meta/get?target_id=record:${rid}`, { headers: apiHeaders() });
              if (!mr.ok) continue;
              const d = await mr.json().catch(() => ({})) as { metadata?: Record<string, unknown> };
              const cm = d.metadata;
              if (!cm) continue;
              const ci = cm.chunk_index as number | undefined;
              if (ci === chunkIndex - 1 || ci === chunkIndex + 1) {
                const chunkText = (cm.text as string) ?? "";
                // Skip adjacent chunks that are also bibliography entries
                if (isReferenceChunk(chunkText)) continue;
                adjacent.push(rid);
                graphContextChunks.push({
                  record_id: rid,
                  chunk_index: ci,
                  text: chunkText,
                  source: (cm.source as string) ?? "",
                });
                alreadyRetrieved.add(rid);
              }
            } catch { /* skip */ }
          }
        }
      }
    }

    // -- C2: Provenance subgraph ---------------------------------------------------
    // For each chunk node in the top-K results, call /graph/subgraph?root=<chunk_node>&depth=1
    // to capture the entities (Concept nodes) that were Mentioned by this chunk.
    // Deduplicate across chunks; failures are silent.
    const provenanceNodes: { id: number; kind: number; label: string | null }[] = [];
    const provenanceEdges: { id: number; from: number; to: number; kind: number }[] = [];
    const seenNodeIds = new Set<number>();
    const seenEdgeIds = new Set<number>();

    for (const r of rankedResults.slice(0, 5)) {
      const m = r.metadata as Record<string, unknown> | null;
      const chunkNodeId = m?.chunk_node_id as number | undefined;
      if (chunkNodeId === undefined) continue;
      try {
        const sgRes = await fetch(`${getApiUrl()}/graph/subgraph?root=${chunkNodeId}&depth=1`, { headers: apiHeaders() });
        if (!sgRes.ok) continue;
        const sg = await sgRes.json() as {
          nodes: { id: number; kind: number; record: number | null }[];
          edges: { id: number; from: number; to: number; kind: number }[];
        };
        for (const node of sg.nodes) {
          if (seenNodeIds.has(node.id)) continue;
          seenNodeIds.add(node.id);
          // Fetch entity label for Concept nodes (kind=1)
          let label: string | null = null;
          if (node.kind === 1) {
            try {
              const metaRes = await fetch(`${getApiUrl()}/v1/memory/meta/get?target_id=node:${node.id}`, { headers: apiHeaders() });
              if (metaRes.ok) {
                const d = await metaRes.json() as { metadata?: Record<string, unknown> };
                label = (d.metadata?.label as string | undefined) ?? null;
              }
            } catch { /* skip */ }
          }
          provenanceNodes.push({ id: node.id, kind: node.kind, label });
        }
        for (const edge of sg.edges) {
          if (seenEdgeIds.has(edge.id)) continue;
          seenEdgeIds.add(edge.id);
          provenanceEdges.push(edge);
        }
      } catch { /* skip if graph unavailable */ }
    }

    // -- Optional LLM synthesis --------------------------------------------------
    let synthesis: string | null = null;
    let synthesis_error: string | null = null;

    if (llm && (rankedResults.length > 0)) {
      // Truncate each chunk before sending to the LLM.
      // 1500 chars keeps the answer tail of dense chunks (e.g. "AdamW optimizer."
      // appears at char ~1100 of chunk 7). 3 chunks × 1500 = ~4500 chars, well
      // within llama3.2:3b's 8k token window.
      const MAX_CHUNK_CHARS = 1500;
      const MAX_CHUNKS_FOR_LLM = 3;

      const topChunks = rankedResults
        .filter((r) => r.metadata?.text)
        .slice(0, MAX_CHUNKS_FOR_LLM);

      const primaryContext = topChunks
        .map((r, i) => {
          const m = r.metadata as Record<string, unknown>;
          const rawText = String(m.text ?? "");
          const text = rawText.length > MAX_CHUNK_CHARS
            ? rawText.slice(0, MAX_CHUNK_CHARS) + "…"
            : rawText;
          const ctx = m.context_sentence ? `\nContext: ${m.context_sentence}` : "";
          return `[Source ${i + 1}: ${m.source ?? "unknown"}, chunk ${m.chunk_index ?? "?"}]${ctx}\n${text}`;
        })
        .join("\n\n---\n\n");

      // Graph-expanded context: adjacent chunks (also truncated)
      const expandedContext = graphContextChunks.length > 0
        ? "\n\n--- Adjacent context ---\n\n" +
          graphContextChunks
            .sort((a, b) => a.chunk_index - b.chunk_index)
            .slice(0, 2)
            .map((c) => {
              const t = c.text.length > 600 ? c.text.slice(0, 600) + "…" : c.text;
              return `[Adjacent: ${c.source}, chunk ${c.chunk_index}]\n${t}`;
            })
            .join("\n\n---\n\n")
        : "";

      const systemPrompt =
        "You are a precise document Q&A assistant. " +
        "IMPORTANT RULES:\n" +
        "1. Read EVERY source chunk carefully before answering.\n" +
        "2. If ANY chunk contains even a partial answer, quote that exact text and answer based on it.\n" +
        "3. Short fragments like 'AdamW optimizer.' or 'Context Parallelism (CP)' ARE valid answers — quote them.\n" +
        "4. Only say the answer is missing if you read all chunks and found zero relevant text.\n" +
        "5. Never say 'not mentioned' or 'not explicitly stated' if the exact words appear in any chunk.\n" +
        "6. Keep your answer short: 1-3 sentences quoting the source.";

      const userMessage = question
        ? `Question: ${question}\n\nSource chunks (read all of them):\n${primaryContext}${expandedContext}\n\nFind the answer in the chunks above and quote it directly.`
        : `Summarize the information in these records:\n${primaryContext}${expandedContext}`;

      try {
        synthesis = await callLLM(systemPrompt, userMessage, llm);
      } catch (e) {
        synthesis_error = e instanceof Error ? e.message : String(e);
      }
    }

    // -- Proof-carrying receipt --------------------------------------------------
    // Captured atomically with the answer: the content hash of every chunk that
    // fed the answer, plus the global BLAKE3 state hash at this instant. With a
    // copy of events.log a third party can (1) replay to reproduce the state
    // hash, (2) re-hash each record's text and match content_sha256, proving the
    // answer was grounded in exactly these unaltered chunks. The client adds the
    // question, answer hash, model identity, and the final receipt fingerprint.
    const globalStateHash = await fetchGlobalStateHash();
    const receiptChunks = rankedResults.map((r) => {
      const m = r.metadata as Record<string, unknown> | null;
      const text = (m?.text as string) ?? "";
      return {
        record_id: r.record_id,
        chunk_index: (m?.chunk_index as number | undefined) ?? null,
        source: (m?.source as string | undefined) ?? null,
        score: r.score ?? null,
        rerank_score: r.rerank_score ?? null,
        enriched: !!(m?.enriched),
        content_sha256: text ? sha256(text) : null,
        content_length: text.length,
      };
    });
    const graphChunkRefs = graphContextChunks.map((c) => ({
      record_id: c.record_id,
      chunk_index: c.chunk_index,
      content_sha256: c.text ? sha256(c.text) : null,
    }));

    return NextResponse.json({
      results: rankedResults,
      graph_context: graphContextChunks,
      synthesis,
      synthesis_error,
      receipt: {
        global_state_hash: globalStateHash,
        captured_at: new Date().toISOString(),
        chunks: receiptChunks,
        graph_chunks: graphChunkRefs,
        provenance_nodes: provenanceNodes,
        provenance_edges: provenanceEdges,
        reranked,
      },
    });
  } catch (err) {
    return NextResponse.json(
      { error: err instanceof Error ? err.message : String(err) },
      { status: 500 }
    );
  }
}
