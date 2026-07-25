import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from "@/lib/server/project";
import crypto from "crypto";

import { streamLLM, type LLMConfig } from "@/lib/server/llm";
import { rerankChunks, type RerankerConfig, type RerankResult } from "@/lib/server/reranker";
import { isReferenceChunk } from "@/lib/server/content-filter";

const JSON_HEADERS = { "Content-Type": "application/json" };

function sha256(text: string): string {
  return "sha256:" + crypto.createHash("sha256").update(text, "utf8").digest("hex");
}

async function fetchGlobalStateHash(nodeUrl: string): Promise<string | null> {
  try {
    const res = await fetchWithTimeout(`${nodeUrl}/v1/proof/state`, { headers: JSON_HEADERS, cache: "no-store" });
    if (!res.ok) return null;
    const d = await res.json().catch(() => ({})) as { final_state_hash?: string };
    return d.final_state_hash ?? null;
  } catch {
    return null;
  }
}

// ── Smart-search helpers (Tier 0 adjacency gate + Tier 1 extractive) ─────────

const GRAPH_EXPAND_TOP = 2;          // expand graph neighbors for the top-N hits only
const EXTRACTIVE_MIN_COSINE = 0.7;   // top-hit similarity floor to consider skipping the LLM
const EXTRACTIVE_MIN_COVERAGE = 0.6; // fraction of question terms that must appear in the top chunk

const STOPWORDS = new Set([
  "the", "and", "for", "was", "what", "which", "that", "this", "with", "from", "were", "are",
  "how", "did", "does", "who", "when", "where", "why", "not", "its", "their", "has", "have",
  "had", "will", "been", "being", "than", "then", "into", "over", "under", "about", "after",
  "before", "between", "during", "each", "other", "some", "such", "only", "also", "more",
  "most", "can", "could", "would", "should", "may", "might", "must", "per", "via",
]);

function contentTerms(q: string): string[] {
  return (q.toLowerCase().match(/[a-z0-9$%]+/g) ?? []).filter((t) => t.length > 1 && !STOPWORDS.has(t));
}

function termHits(terms: string[], text: string): number {
  const lower = text.toLowerCase();
  return terms.filter((t) => lower.includes(t)).length;
}

function endsMidSentence(t: string): boolean {
  const s = t.trim();
  return s.length > 0 && !/[.!?"'”)\]]$/.test(s);
}

function startsMidSentence(t: string): boolean {
  const s = t.trim();
  return s.length > 0 && /^[a-z0-9]/.test(s);
}

/** Tokens embeddings can't discriminate: numbers and alphanumeric codes (162, 8.2, gb300, h20). */
function rareQueryTokens(q: string): string[] {
  return (q.toLowerCase().match(/[a-z0-9]+(?:\.[0-9]+)?/g) ?? []).filter(
    (t) => /\d/.test(t) && t.replace(/[^0-9]/g, "").length >= 2,
  );
}

/** Pick the sentence with the most question-term hits, plus one neighbor each side. */
function extractBestPassage(text: string, terms: string[]): string | null {
  const sentences = text.split(/(?<=[.!?])\s+(?=[A-Z"“$0-9(])/).filter((s) => s.trim().length > 0);
  let bestIdx = -1;
  let bestHits = 0;
  sentences.forEach((s, i) => {
    const hits = termHits(terms, s);
    if (hits > bestHits) { bestHits = hits; bestIdx = i; }
  });
  if (bestIdx < 0 || bestHits < 2) return null;
  const passage = sentences.slice(Math.max(0, bestIdx - 1), bestIdx + 2).join(" ").trim();
  if (passage.length < 20) return null;
  return passage.length > 700 ? passage.slice(0, 700) + "…" : passage;
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

export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  let nodeUrl: string;
  try {
    nodeUrl = await resolveProjectNodeUrl(id);
  } catch (e) {
    if (e instanceof ProjectNotFoundError) {
      return NextResponse.json({ error: "not found" }, { status: 404 });
    }
    if (e instanceof ProjectNotReadyError) {
      return NextResponse.json({ error: "project not active yet" }, { status: 409 });
    }
    return NextResponse.json({ error: "node unreachable" }, { status: 503 });
  }

  try {
    const body: WhyRequest = await req.json();
    const { record_id, query_vector, k = 5, collection = "default", question, max_context_chunks, llm, reranker } = body;

    const results: { record_id: number; score?: number; metadata: Record<string, unknown> | null }[] = [];

    if (record_id !== undefined) {
      const metaRes = await fetchWithTimeout(`${nodeUrl}/v1/memory/meta/get?target_id=record:${record_id}`, { headers: JSON_HEADERS });
      const meta = metaRes.ok ? await metaRes.json().catch(() => null) : null;
      results.push({ record_id, metadata: meta?.metadata ?? null });
    } else if (query_vector) {
      // Rare tokens (exact numbers, product codes) are invisible to embeddings —
      // widen the candidate pool so a lexical boost can rescue exact matches.
      const rareTokens = question ? rareQueryTokens(question) : [];
      const fetchK = rareTokens.length > 0 ? Math.min(k * 8, 48) : Math.min(k * 3, 30);
      const searchRes = await fetchWithTimeout(`${nodeUrl}/search`, {
        method: "POST",
        headers: JSON_HEADERS,
        body: JSON.stringify({ query: query_vector, k: fetchK, collection, query_text: question ?? undefined }),
      });
      if (!searchRes.ok) return NextResponse.json({ error: "search failed" }, { status: 502 });
      const { results: hits } = await searchRes.json() as { results: { id: number; score: number }[] };

      const candidates: typeof results = await Promise.all(
        hits.map(async (hit) => {
          const metaRes = await fetchWithTimeout(`${nodeUrl}/v1/memory/meta/get?target_id=record:${hit.id}`, { headers: JSON_HEADERS });
          const meta = metaRes.ok ? await metaRes.json().catch(() => null) : null;
          return { record_id: hit.id, score: hit.score, metadata: meta?.metadata ?? null };
        }),
      );

      const nonSuperseded = candidates.filter((c) => !c.metadata?.superseded);
      const contentChunks = nonSuperseded.filter((c) => !isReferenceChunk((c.metadata?.text as string) ?? ""));
      const referenceChunks = nonSuperseded.filter((c) => isReferenceChunk((c.metadata?.text as string) ?? ""));
      let ordered = [...contentChunks, ...referenceChunks];
      // Lexical boost: stable-sort candidates by how many rare question tokens
      // they contain, so an exact "162" match outranks a semantically-similar twin.
      if (rareTokens.length > 0) {
        const scored = ordered.map((c, i) => ({ c, i, hits: termHits(rareTokens, (c.metadata?.text as string) ?? "") }));
        scored.sort((a, b) => b.hits - a.hits || a.i - b.i);
        ordered = scored.map((s) => s.c);
      }
      results.push(...ordered.slice(0, k));
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

    // Graph-augmented context expansion (sentence window retrieval).
    // Tier 0: only the top hits get neighbor expansion — expanding every hit
    // pulls in neighbors of weak matches, which is pure noise for the LLM.
    const graphContextChunks: { record_id: number; chunk_index: number; text: string; source: string }[] = [];
    const qTerms = question ? contentTerms(question) : [];

    if (rankedResults.length > 0) {
      const expandable = rankedResults.slice(0, GRAPH_EXPAND_TOP);
      const alreadyRetrieved = new Set(rankedResults.map((r) => r.record_id));
      const docNodeIds = new Set<number>();
      for (const r of expandable) {
        const docId = (r.metadata as Record<string, unknown> | null)?.document_node_id as number | undefined;
        if (docId !== undefined) docNodeIds.add(docId);
      }

      const docEdges: Map<number, number[]> = new Map();
      for (const docNodeId of docNodeIds) {
        try {
          const edgesRes = await fetchWithTimeout(`${nodeUrl}/graph/edges/${docNodeId}`, { headers: JSON_HEADERS });
          if (edgesRes.ok) {
            const { edges } = await edgesRes.json() as { edges: { to_node: number; kind: number }[] };
            docEdges.set(docNodeId, edges.map((e) => e.to_node));
          }
        } catch { /* skip if graph unavailable */ }
      }

      if (docEdges.size > 0) {
        let nodeToRecord: Map<number, number> = new Map();
        try {
          const nodesRes = await fetchWithTimeout(`${nodeUrl}/graph/nodes?collection=${collection}`, { headers: JSON_HEADERS });
          if (nodesRes.ok) {
            const { nodes } = await nodesRes.json() as { nodes: { node_id: number; record_id: number | null }[] };
            for (const n of nodes) {
              if (n.record_id !== null) nodeToRecord.set(n.node_id, n.record_id);
            }
          }
        } catch { /* skip */ }

        for (const hit of expandable) {
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
              const mr = await fetchWithTimeout(`${nodeUrl}/v1/memory/meta/get?target_id=record:${rid}`, { headers: JSON_HEADERS });
              if (!mr.ok) continue;
              const d = await mr.json().catch(() => ({})) as { metadata?: Record<string, unknown> };
              const cm = d.metadata;
              if (!cm) continue;
              const ci = cm.chunk_index as number | undefined;
              if (ci === chunkIndex - 1 || ci === chunkIndex + 1) {
                const chunkText = (cm.text as string) ?? "";
                if (isReferenceChunk(chunkText)) continue;
                // Tier 0 gate: keep a neighbor only if it shares question terms,
                // or the matched chunk is cut mid-sentence at that boundary
                if (qTerms.length > 0) {
                  const matchedText = (m.text as string) ?? "";
                  const continues =
                    (ci === chunkIndex + 1 && endsMidSentence(matchedText)) ||
                    (ci === chunkIndex - 1 && startsMidSentence(matchedText));
                  if (!continues && termHits(qTerms, chunkText) < 2) continue;
                }
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
        const sgRes = await fetchWithTimeout(`${nodeUrl}/graph/subgraph?root=${chunkNodeId}&depth=1`, { headers: JSON_HEADERS });
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
              const metaRes = await fetchWithTimeout(`${nodeUrl}/v1/memory/meta/get?target_id=node:${node.id}`, { headers: JSON_HEADERS });
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

    // Proof-carrying receipt (built before streaming so it's ready immediately)
    const globalStateHash = await fetchGlobalStateHash(nodeUrl);
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
    const serverReceipt = {
      global_state_hash: globalStateHash,
      captured_at: new Date().toISOString(),
      chunks: receiptChunks,
      graph_chunks: graphChunkRefs,
      provenance_nodes: provenanceNodes,
      provenance_edges: provenanceEdges,
      reranked,
    };

    // Tier 1: extractive short-circuit — when the top chunk plainly contains
    // the answer (high similarity + dense question-term coverage), quote it
    // directly and skip the LLM call entirely.
    let extractive: string | null = null;
    if (llm && question && rankedResults.length > 0) {
      const top = rankedResults[0];
      const topMeta = top.metadata as Record<string, unknown> | null;
      const topText = (topMeta?.text as string) ?? "";
      if (topText && qTerms.length >= 3 && top.score !== undefined) {
        const cosine = Math.max(0, 1 - top.score / 2);
        const coverage = termHits(qTerms, topText) / qTerms.length;
        if (cosine >= EXTRACTIVE_MIN_COSINE && coverage >= EXTRACTIVE_MIN_COVERAGE) {
          const passage = extractBestPassage(topText, qTerms);
          if (passage) {
            const src = topMeta?.source
              ? `\n\n— ${topMeta.source}${topMeta.chunk_index !== undefined ? ` · chunk ${topMeta.chunk_index}` : ""}`
              : "";
            extractive = `“${passage}”${src}`;
          }
        }
      }
    }

    // Build LLM prompt once (reused in stream if LLM is configured)
    let systemPrompt: string | null = null;
    let userMessage: string | null = null;
    if (!extractive && llm && rankedResults.length > 0) {
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
      systemPrompt =
        "You are a precise document Q&A assistant. " +
        "IMPORTANT RULES:\n" +
        "1. Read EVERY source chunk carefully before answering.\n" +
        "2. If ANY chunk contains even a partial answer, quote that exact text and answer based on it.\n" +
        "3. Short fragments like 'AdamW optimizer.' or 'Context Parallelism (CP)' ARE valid answers — quote them.\n" +
        "4. Only say the answer is missing if you read all chunks and found zero relevant text.\n" +
        "5. Never say 'not mentioned' or 'not explicitly stated' if the exact words appear in any chunk.\n" +
        "6. Keep your answer short: 1-3 sentences quoting the source.";
      userMessage = question
        ? `Question: ${question}\n\nSource chunks (read all of them):\n${primaryContext}${expandedContext}\n\nFind the answer in the chunks above and quote it directly.`
        : `Summarize the information in these records:\n${primaryContext}${expandedContext}`;
    }

    // Stream SSE: first emit the results (sources appear immediately), then stream LLM tokens
    const enc = new TextEncoder();
    const stream = new ReadableStream({
      async start(controller) {
        const emit = (obj: unknown) => {
          controller.enqueue(enc.encode(`data: ${JSON.stringify(obj)}\n\n`));
        };
        try {
          emit({ type: "results", results: rankedResults, graph_context: graphContextChunks, receipt: serverReceipt });
          if (extractive) {
            emit({ type: "mode", mode: "extractive" });
            emit({ type: "token", content: extractive });
          } else if (llm && systemPrompt && userMessage) {
            try {
              for await (const token of streamLLM(systemPrompt, userMessage, llm)) {
                emit({ type: "token", content: token });
              }
            } catch (e) {
              emit({ type: "llm_error", message: e instanceof Error ? e.message : String(e) });
            }
          }
          emit({ type: "done" });
        } finally {
          controller.close();
        }
      },
    });

    return new Response(stream, {
      headers: {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        "X-Accel-Buffering": "no",
      },
    });
  } catch (err) {
    return NextResponse.json({ error: err instanceof Error ? err.message : String(err) }, { status: 500 });
  }
}