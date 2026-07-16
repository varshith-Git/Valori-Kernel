import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import crypto from "crypto";

import { getApiUrl } from "@/lib/server/connection";
import { callLLM, type LLMConfig } from "@/lib/server/llm";
import { rerankChunks, type RerankerConfig, type RerankResult } from "@/lib/server/reranker";
import { isReferenceChunk } from "@/lib/server/content-filter";

const TOKEN = process.env.VALORI_AUTH_TOKEN;

function apiHeaders(): Record<string, string> {
  const h: Record<string, string> = { "Content-Type": "application/json" };
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

function sha256(text: string): string {
  return "sha256:" + crypto.createHash("sha256").update(text, "utf8").digest("hex");
}

async function fetchGlobalStateHash(): Promise<string | null> {
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/proof/state`, { headers: apiHeaders(), cache: "no-store" });
    if (!res.ok) return null;
    const d = await res.json().catch(() => ({})) as { final_state_hash?: string };
    return d.final_state_hash ?? null;
  } catch {
    return null;
  }
}

interface WhyRequest {
  record_id?: number;
  query_vector?: number[];
  k?: number;
  collection?: string;
  question?: string;
  max_context_chunks?: number;
  llm?: LLMConfig;
  reranker?: RerankerConfig;
}

export async function POST(req: NextRequest) {
  try {
    const body: WhyRequest = await req.json();
    const { record_id, query_vector, k = 5, collection = "default", question, max_context_chunks, llm, reranker } = body;

    const results: { record_id: number; score?: number; metadata: Record<string, unknown> | null }[] = [];

    if (record_id !== undefined) {
      const metaRes = await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/get?target_id=record:${record_id}`, { headers: apiHeaders() });
      const meta = metaRes.ok ? await metaRes.json().catch(() => null) : null;
      results.push({ record_id, metadata: meta?.metadata ?? null });
    } else if (query_vector) {
      const fetchK = Math.min(k * 3, 30);
      const searchRes = await fetchWithTimeout(`${getApiUrl()}/search`, {
        method: "POST",
        headers: apiHeaders(),
        body: JSON.stringify({ query: query_vector, k: fetchK, collection, query_text: question ?? undefined }),
      });
      if (!searchRes.ok) return NextResponse.json({ error: "search failed" }, { status: 502 });
      const { results: hits } = await searchRes.json() as { results: { id: number; score: number }[] };

      const candidates: typeof results = [];
      for (const hit of hits) {
        const metaRes = await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/get?target_id=record:${hit.id}`, { headers: apiHeaders() });
        const meta = metaRes.ok ? await metaRes.json().catch(() => null) : null;
        candidates.push({ record_id: hit.id, score: hit.score, metadata: meta?.metadata ?? null });
      }

      const nonSuperseded = candidates.filter((c) => !c.metadata?.superseded);
      const contentChunks = nonSuperseded.filter((c) => !isReferenceChunk((c.metadata?.text as string) ?? ""));
      const referenceChunks = nonSuperseded.filter((c) => isReferenceChunk((c.metadata?.text as string) ?? ""));
      results.push(...[...contentChunks, ...referenceChunks].slice(0, k));
    } else {
      return NextResponse.json({ error: "provide record_id or query_vector" }, { status: 400 });
    }

    // Tier-2 reranking (optional)
    let reranked = false;
    let rankedResults: RerankResult[] = results as RerankResult[];
    if (reranker && question && results.length > 1) {
      const rerankOutput = await rerankChunks(question, results, reranker);
      rankedResults = rerankOutput;
      reranked = rerankOutput.some((r) => r.rerank_score !== null);
    }

    // Graph-augmented context expansion (sentence window retrieval)
    const graphContextChunks: { record_id: number; chunk_index: number; text: string; source: string }[] = [];

    if (rankedResults.length > 0) {
      const alreadyRetrieved = new Set(rankedResults.map((r) => r.record_id));
      const docNodeIds = new Set<number>();
      for (const r of rankedResults) {
        const docId = (r.metadata as Record<string, unknown> | null)?.document_node_id as number | undefined;
        if (docId !== undefined) docNodeIds.add(docId);
      }

      const docEdges: Map<number, number[]> = new Map();
      for (const docNodeId of docNodeIds) {
        try {
          const edgesRes = await fetchWithTimeout(`${getApiUrl()}/graph/edges/${docNodeId}`, { headers: apiHeaders() });
          if (edgesRes.ok) {
            const { edges } = await edgesRes.json() as { edges: { to_node: number; kind: number }[] };
            docEdges.set(docNodeId, edges.map((e) => e.to_node));
          }
        } catch { /* skip if graph unavailable */ }
      }

      if (docEdges.size > 0) {
        let nodeToRecord: Map<number, number> = new Map();
        try {
          const nodesRes = await fetchWithTimeout(`${getApiUrl()}/graph/nodes?collection=${collection}`, { headers: apiHeaders() });
          if (nodesRes.ok) {
            const { nodes } = await nodesRes.json() as { nodes: { node_id: number; record_id: number | null }[] };
            for (const n of nodes) {
              if (n.record_id !== null) nodeToRecord.set(n.node_id, n.record_id);
            }
          }
        } catch { /* skip */ }

        for (const hit of rankedResults) {
          const m = hit.metadata as Record<string, unknown> | null;
          if (!m) continue;
          const docNodeId = m.document_node_id as number | undefined;
          const chunkIndex = m.chunk_index as number | undefined;
          if (docNodeId === undefined || chunkIndex === undefined) continue;

          const chunkNodeIds = docEdges.get(docNodeId) ?? [];
          const docRecordIds: number[] = [];
          for (const nodeId of chunkNodeIds) {
            const rid = nodeToRecord.get(nodeId);
            if (rid !== undefined) docRecordIds.push(rid);
          }

          for (const rid of docRecordIds) {
            if (alreadyRetrieved.has(rid)) continue;
            try {
              const mr = await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/get?target_id=record:${rid}`, { headers: apiHeaders() });
              if (!mr.ok) continue;
              const d = await mr.json().catch(() => ({})) as { metadata?: Record<string, unknown> };
              const cm = d.metadata;
              if (!cm) continue;
              const ci = cm.chunk_index as number | undefined;
              if (ci === chunkIndex - 1 || ci === chunkIndex + 1) {
                const chunkText = (cm.text as string) ?? "";
                if (isReferenceChunk(chunkText)) continue;
                graphContextChunks.push({ record_id: rid, chunk_index: ci, text: chunkText, source: (cm.source as string) ?? "" });
                alreadyRetrieved.add(rid);
              }
            } catch { /* skip */ }
          }
        }
      }
    }

    // Provenance subgraph (C2)
    const provenanceNodes: { id: number; kind: number; label: string | null }[] = [];
    const provenanceEdges: { id: number; from: number; to: number; kind: number }[] = [];
    const seenNodeIds = new Set<number>();
    const seenEdgeIds = new Set<number>();

    for (const r of rankedResults.slice(0, 5)) {
      const m = r.metadata as Record<string, unknown> | null;
      const chunkNodeId = m?.chunk_node_id as number | undefined;
      if (chunkNodeId === undefined) continue;
      try {
        const sgRes = await fetchWithTimeout(`${getApiUrl()}/graph/subgraph?root=${chunkNodeId}&depth=1`, { headers: apiHeaders() });
        if (!sgRes.ok) continue;
        const sg = await sgRes.json() as {
          nodes: { id: number; kind: number; record: number | null }[];
          edges: { id: number; from: number; to: number; kind: number }[];
        };
        for (const node of sg.nodes) {
          if (seenNodeIds.has(node.id)) continue;
          seenNodeIds.add(node.id);
          let label: string | null = null;
          if (node.kind === 1) {
            try {
              const metaRes = await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/get?target_id=node:${node.id}`, { headers: apiHeaders() });
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

    // Optional LLM synthesis
    let synthesis: string | null = null;
    let synthesis_error: string | null = null;

    if (llm && rankedResults.length > 0) {
      const MAX_CHUNK_CHARS = 1500;
      const MAX_CHUNKS_FOR_LLM = Math.min(20, Math.max(1, max_context_chunks ?? 3));

      const topChunks = rankedResults.filter((r) => r.metadata?.text).slice(0, MAX_CHUNKS_FOR_LLM);
      const primaryContext = topChunks.map((r, i) => {
        const m = r.metadata as Record<string, unknown>;
        const rawText = String(m.text ?? "");
        const text = rawText.length > MAX_CHUNK_CHARS ? rawText.slice(0, MAX_CHUNK_CHARS) + "…" : rawText;
        const ctx = m.context_sentence ? `\nContext: ${m.context_sentence}` : "";
        return `[Source ${i + 1}: ${m.source ?? "unknown"}, chunk ${m.chunk_index ?? "?"}]${ctx}\n${text}`;
      }).join("\n\n---\n\n");

      const expandedContext = graphContextChunks.length > 0
        ? "\n\n--- Adjacent context ---\n\n" +
          graphContextChunks.sort((a, b) => a.chunk_index - b.chunk_index).slice(0, 2).map((c) => {
            const t = c.text.length > 600 ? c.text.slice(0, 600) + "…" : c.text;
            return `[Adjacent: ${c.source}, chunk ${c.chunk_index}]\n${t}`;
          }).join("\n\n---\n\n")
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

    // Proof-carrying receipt
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
    return NextResponse.json({ error: err instanceof Error ? err.message : String(err) }, { status: 500 });
  }
}