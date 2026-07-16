import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";

import { getApiUrl } from "@/lib/server/connection";
import { extractText } from "@/lib/server/extract-text";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function apiHeaders(): Record<string, string> {
  const h: Record<string, string> = { "Content-Type": "application/json" };
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

// -- Tree chunker --------------------------------------------------------------
// Parses raw text into section-based chunks by detecting numbered/titled headers.
// Mirrors the Python tree_rag.py TreeIndex logic: each section becomes one chunk
// with its title prepended, so the answer context is never split mid-sentence.

interface TreeNode {
  title: string;
  text: string;   // title + own_text concatenated, ready to embed
}

function chunkTextTree(text: string, maxSize: number = 1200, overlap: number = 200): TreeNode[] {
  const normalized = text
    .replace(/\r\n/g, "\n")
    .replace(/[ \t]+/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

  const lines = normalized.split("\n");

  // Detect header lines: numbered sections like "3.1 Training", "4 RL", or
  // ALL-CAPS short lines, or lines that end with nothing (pure title-case short lines).
  const NUMBERED = /^(\d+(\.\d+)*)\s+[A-Z][^\n]{2,60}$/;
  const MARKDOWN_HEADER = /^#{1,4}\s+.{2,80}$/;
  // Also catch lines that look like section titles: Title Case, short, no period at end
  const TITLE_CASE = /^[A-Z][A-Za-z0-9 ,:\-–/]{4,60}[^.!?,]$/;

  const headerIdxs: number[] = [];
  let inCode = false;
  for (let i = 0; i < lines.length; i++) {
    const s = lines[i].trim();
    if (s.startsWith("```")) { inCode = !inCode; continue; }
    if (inCode) continue;
    if (NUMBERED.test(s) || MARKDOWN_HEADER.test(s)) {
      headerIdxs.push(i);
    } else if (TITLE_CASE.test(s) && s.length < 70) {
      // Only treat as header if next line is non-empty (header followed by body)
      if (i + 1 < lines.length && lines[i + 1].trim().length > 0) {
        headerIdxs.push(i);
      }
    }
  }

  if (headerIdxs.length < 2) {
    // Document has no detectable structure — fall back to the caller's fixed-size chunker
    return [];
  }

  const cappedSize = Math.min(maxSize, 1200);
  const cappedOverlap = Math.min(overlap, Math.floor(cappedSize / 4));
  const nodes: TreeNode[] = [];

  for (let i = 0; i < headerIdxs.length; i++) {
    const start = headerIdxs[i];
    const end = i + 1 < headerIdxs.length ? headerIdxs[i + 1] : lines.length;
    const title = lines[start].trim();
    const body = lines.slice(start + 1, end).join("\n").trim();
    if (!body && i + 1 < headerIdxs.length) continue; // skip empty sections
    const combined = `${title}\n${body}`.trim();
    if (combined.length >= 50) {
      if (combined.length > cappedSize) {
        // Section exceeds max chunk size (~300 tokens) — sub-chunk body while preserving section title
        const subSize = Math.max(200, cappedSize - Math.min(title.length, 100) - 2);
        const subChunks = chunkText(body, subSize, cappedOverlap);
        for (const sub of subChunks) {
          nodes.push({ title, text: `${title}\n${sub}`.trim() });
        }
      } else {
        nodes.push({ title, text: combined });
      }
    }
  }

  return nodes;
}

// -- Fixed-size chunking -------------------------------------------------------

function chunkText(text: string, size: number, overlap: number): string[] {
  // Hard cap: no chunk larger than 1200 chars (~300 tokens) so small models stay grounded
  const cappedSize = Math.min(size, 1200);
  const cappedOverlap = Math.min(overlap, Math.floor(cappedSize / 4));

  // Normalize whitespace: collapse multiple newlines / spaces from PDF extraction
  const normalized = text
    .replace(/\r\n/g, "\n")
    .replace(/[ \t]+/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

  const chunks: string[] = [];
  const step = Math.max(1, cappedSize - cappedOverlap);
  size = cappedSize;
  overlap = cappedOverlap;

  for (let start = 0; start < normalized.length; start += step) {
    let end = start + size;

    // Snap end to a sentence/paragraph boundary to avoid cutting mid-word
    if (end < normalized.length) {
      const boundary = normalized.slice(end - 80, end + 80);
      const sentenceEnd = boundary.search(/[.!?\n]/);
      if (sentenceEnd !== -1) {
        end = end - 80 + sentenceEnd + 1;
      }
    }

    const chunk = normalized.slice(start, end).trim();
    if (chunk.length >= 30) chunks.push(chunk);
  }

  return chunks;
}

// -- Embedding providers --------------------------------------------------------

interface EmbedConfig {
  provider: string;
  model: string;
  apiKey: string;
  endpoint: string;
}

async function embedBatch(texts: string[], cfg: EmbedConfig): Promise<number[][]> {
  switch (cfg.provider) {
    case "openai": {
      const res = await fetchWithTimeout(cfg.endpoint || "https://api.openai.com/v1/embeddings", {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.apiKey}` },
        body: JSON.stringify({ input: texts, model: cfg.model || "text-embedding-3-small" }),
      });
      if (!res.ok) {
        const e = await res.json().catch(() => ({})) as { error?: { message?: string } };
        throw new Error(`OpenAI: ${e.error?.message ?? res.status}`);
      }
      const data = await res.json() as { data: { embedding: number[] }[] };
      return data.data.map((d) => d.embedding);
    }
    case "cohere": {
      const res = await fetchWithTimeout(cfg.endpoint || "https://api.cohere.ai/v1/embed", {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.apiKey}` },
        body: JSON.stringify({
          texts,
          model: cfg.model || "embed-english-v3.0",
          input_type: "search_document",
          embedding_types: ["float"],
        }),
      });
      if (!res.ok) throw new Error(`Cohere: ${res.status}`);
      const data = await res.json() as { embeddings: { float: number[][] } };
      return data.embeddings.float;
    }
    case "ollama": {
      const model = cfg.model || "nomic-embed-text";
      // Normalize to base URL — strip any /api/embed(dings) suffix the user may have saved
      const base = (cfg.endpoint || "http://localhost:11434")
        .replace(/\/api\/embed(?:dings)?$/, "")
        .replace(/\/$/, "");

      // Always send ONE text at a time to Ollama.
      // Batching via input:[] causes Ollama to concatenate all texts internally,
      // blowing past the model's context window on larger documents.
      const results: number[][] = [];

      for (const text of texts) {
        // Truncate to ~1800 chars (~450 tokens) to stay safely within 512-token model context windows
        const safeText = text.slice(0, 1800);

        // Try /api/embed first (Ollama ≥ 0.1.36)
        let res = await fetchWithTimeout(`${base}/api/embed`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ model, input: safeText }),
        });

        if (res.status === 404) {
          // Fall back to /api/embeddings (Ollama < 0.1.36)
          res = await fetchWithTimeout(`${base}/api/embeddings`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ model, prompt: safeText }),
          });

          if (!res.ok) {
            const b = await res.json().catch(() => ({})) as { error?: string };
            if (res.status === 404) {
              throw new Error(`Ollama model "${model}" not found — run: ollama pull ${model}`);
            }
            throw new Error(`Ollama: ${b.error ?? `HTTP ${res.status}`}`);
          }

          const d = await res.json() as { embedding: number[] };
          results.push(d.embedding);
          continue;
        }

        if (!res.ok) {
          const b = await res.json().catch(() => ({})) as { error?: string };
          throw new Error(`Ollama: ${b.error ?? `HTTP ${res.status}`}`);
        }

        const d = await res.json() as { embeddings: number[][] };
        results.push(d.embeddings[0]);
      }

      return results;
    }
    case "custom": {
      const res = await fetchWithTimeout(cfg.endpoint, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...(cfg.apiKey ? { Authorization: `Bearer ${cfg.apiKey}` } : {}),
        },
        body: JSON.stringify({ input: texts, model: cfg.model }),
      });
      if (!res.ok) throw new Error(`Custom endpoint: ${res.status}`);
      const data = await res.json() as { embeddings?: number[][]; data?: { embedding: number[] }[] };
      if (Array.isArray(data.embeddings)) return data.embeddings;
      if (Array.isArray(data.data)) return data.data.map((d) => d.embedding);
      throw new Error("Unexpected response shape from custom endpoint");
    }
    default:
      throw new Error(`Unknown provider: ${cfg.provider}`);
  }
}

// -- Context enrichment --------------------------------------------------------
// Generates a single context sentence per chunk using the configured LLM.
// The sentence is stored in both:
//   (a) the committed AutoInsertRecord.metadata bytes — audited, BLAKE3-chained
//   (b) the metadata sidecar context_sentence field — for display in the UI
//
// Invariant: the LLM output is logged here, never re-invoked at replay/recovery.

interface EnrichLLMConfig {
  provider: string;
  model: string;
  apiKey: string;
  endpoint: string;
}

async function generateContextSentence(
  chunkText: string,
  docTitle: string,
  chunkIndex: number,
  totalChunks: number,
  llm: EnrichLLMConfig,
): Promise<string | null> {
  const prompt =
    `You are a document indexer. Given a document excerpt, write exactly ONE concise sentence ` +
    `that situates this excerpt within "${docTitle}" (chunk ${chunkIndex + 1} of ${totalChunks}). ` +
    `Do not repeat the text verbatim. Do not start with "This chunk". Return only the sentence.\n\n` +
    `Excerpt:\n${chunkText.slice(0, 800)}`;

  try {
    if (llm.provider === "ollama") {
      const base = (llm.endpoint || "http://localhost:11434").replace(/\/$/, "");
      const res = await fetchWithTimeout(`${base}/api/generate`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ model: llm.model || "llama3.2", prompt, stream: false, options: { temperature: 0 } }),
      });
      if (!res.ok) return null;
      const d = await res.json() as { response?: string };
      return d.response?.trim() ?? null;
    }
    const baseMap: Record<string, string> = {
      openai: "https://api.openai.com", groq: "https://api.groq.com/openai",
      together: "https://api.together.xyz",
    };
    const base = (llm.endpoint || baseMap[llm.provider] || "").replace(/\/$/, "");
    if (!base) return null;
    const res = await fetchWithTimeout(`${base}/v1/chat/completions`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${llm.apiKey}` },
      body: JSON.stringify({
        model: llm.model, temperature: 0, max_tokens: 128,
        messages: [{ role: "user", content: prompt }],
      }),
    });
    if (!res.ok) return null;
    const d = await res.json() as { choices?: { message?: { content?: string } }[] };
    return d.choices?.[0]?.message?.content?.trim() ?? null;
  } catch {
    return null;
  }
}

// Batch context generation with concurrency cap.
async function generateContextBatch(
  chunks: string[],
  docTitle: string,
  llm: EnrichLLMConfig,
  concurrency = 6,
): Promise<(string | null)[]> {
  const results: (string | null)[] = new Array(chunks.length).fill(null);
  for (let i = 0; i < chunks.length; i += concurrency) {
    const slice = chunks.slice(i, i + concurrency);
    const settled = await Promise.allSettled(
      slice.map((text, j) => generateContextSentence(text, docTitle, i + j, chunks.length, llm))
    );
    settled.forEach((r, j) => {
      results[i + j] = r.status === "fulfilled" ? r.value : null;
    });
  }
  return results;
}

// -- Entity extraction (C2) ----------------------------------------------------
// Extracts named entities from a chunk and returns them as an array of strings.
// Same LLM as context enrichment; failure returns empty array (graceful degrade).
async function extractEntities(
  chunkText: string,
  llm: EnrichLLMConfig,
): Promise<string[]> {
  const prompt =
    `Extract named entities (people, organizations, products, concepts, standards, agreements, dates, locations) from the text below. ` +
    `Return ONLY a JSON array of strings. No explanation. No markdown. Max 8 entities.\n\n` +
    `Text:\n${chunkText.slice(0, 600)}`;

  try {
    let raw: string | null = null;
    if (llm.provider === "ollama") {
      const base = (llm.endpoint || "http://localhost:11434").replace(/\/$/, "");
      const res = await fetchWithTimeout(`${base}/api/generate`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ model: llm.model || "llama3.2", prompt, stream: false, options: { temperature: 0 } }),
      });
      if (!res.ok) return [];
      const d = await res.json() as { response?: string };
      raw = d.response?.trim() ?? null;
    } else {
      const baseMap: Record<string, string> = {
        openai: "https://api.openai.com", groq: "https://api.groq.com/openai",
        together: "https://api.together.xyz",
      };
      const base = (llm.endpoint || baseMap[llm.provider] || "").replace(/\/$/, "");
      if (!base) return [];
      const res = await fetchWithTimeout(`${base}/v1/chat/completions`, {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${llm.apiKey}` },
        body: JSON.stringify({
          model: llm.model, temperature: 0, max_tokens: 128,
          messages: [{ role: "user", content: prompt }],
        }),
      });
      if (!res.ok) return [];
      const d = await res.json() as { choices?: { message?: { content?: string } }[] };
      raw = d.choices?.[0]?.message?.content?.trim() ?? null;
    }
    if (!raw) return [];
    // Strip markdown code fences if the model wrapped the JSON
    const clean = raw.replace(/^```(?:json)?\s*/i, "").replace(/\s*```$/, "").trim();
    const parsed = JSON.parse(clean);
    if (Array.isArray(parsed)) return (parsed as unknown[]).filter((e): e is string => typeof e === "string").slice(0, 8);
    return [];
  } catch {
    return [];
  }
}

// -- C3: Global entity registry -----------------------------------------------
// Looks up an entity label in the persistent sidecar registry. If found,
// returns the existing concept node_id. If not, returns null so the caller
// creates a new node and registers it.
async function lookupEntityNode(label: string, collection: string): Promise<number | null> {
  const key = `entity:${collection}:${label.toLowerCase().trim()}`;
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/get?target_id=${encodeURIComponent(key)}`, { headers: apiHeaders() });
    if (!res.ok) return null;
    const d = await res.json() as { metadata?: Record<string, unknown> };
    const nodeId = d.metadata?.node_id;
    return typeof nodeId === "number" ? nodeId : null;
  } catch {
    return null;
  }
}

async function registerEntityNode(label: string, collection: string, nodeId: number): Promise<void> {
  const key = `entity:${collection}:${label.toLowerCase().trim()}`;
  try {
    await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/set`, {
      method: "POST",
      headers: apiHeaders(),
      body: JSON.stringify({
        target_id: key,
        metadata: { node_id: nodeId, label, collection, registered_at: new Date().toISOString() },
      }),
    });
  } catch { /* non-critical */ }
}

// -- C3: Content deduplication ------------------------------------------------
// SHA-256 of chunk text → check if already ingested in this collection.
// Uses Web Crypto (available in Next.js edge/node runtime).
async function sha256hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(buf)).map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function lookupContentRecord(sha: string, collection: string): Promise<number | null> {
  const key = `content:${collection}:${sha}`;
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/get?target_id=${encodeURIComponent(key)}`, { headers: apiHeaders() });
    if (!res.ok) return null;
    const d = await res.json() as { metadata?: Record<string, unknown> };
    const rid = d.metadata?.record_id;
    return typeof rid === "number" ? rid : null;
  } catch {
    return null;
  }
}

async function registerContentRecord(sha: string, collection: string, recordId: number, source: string): Promise<void> {
  const key = `content:${collection}:${sha}`;
  try {
    await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/set`, {
      method: "POST",
      headers: apiHeaders(),
      body: JSON.stringify({
        target_id: key,
        metadata: { record_id: recordId, source, collection, registered_at: new Date().toISOString() },
      }),
    });
  } catch { /* non-critical */ }
}

// -- C3: Contradiction detection ----------------------------------------------
// After all inserts, search for near-duplicates from different source documents.
// Similarity > 0.92 AND different source → queue as potential contradiction.
// Runs asynchronously; never blocks the ingest response.
async function detectContradictions(
  vectors: number[][],
  recordIds: number[],
  source: string,
  collection: string,
): Promise<void> {
  for (let i = 0; i < vectors.length; i++) {
    try {
      const res = await fetchWithTimeout(`${getApiUrl()}/v1/search`, {
        method: "POST",
        headers: apiHeaders(),
        body: JSON.stringify({ vector: vectors[i], k: 5, collection }),
      });
      if (!res.ok) continue;
      const data = await res.json() as { results?: { id: number; score?: number }[] };
      for (const hit of data.results ?? []) {
        if (hit.id === recordIds[i]) continue;         // self
        const score = hit.score ?? 0;
        if (score < 0.92) continue;                   // not similar enough

        // Check if this hit is from a different source document
        const metaRes = await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/get?target_id=record:${hit.id}`, { headers: apiHeaders() });
        if (!metaRes.ok) continue;
        const m = await metaRes.json() as { metadata?: Record<string, unknown> };
        const hitSource = m.metadata?.source as string | undefined;
        if (!hitSource || hitSource === source) continue; // same doc, skip

        // Queue the contradiction
        const contradictionId = `${Date.now()}-${recordIds[i]}-${hit.id}`;
        await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/set`, {
          method: "POST",
          headers: apiHeaders(),
          body: JSON.stringify({
            target_id: `contradiction:${contradictionId}`,
            metadata: {
              record_a: recordIds[i],
              record_b: hit.id,
              source_a: source,
              source_b: hitSource,
              similarity: score,
              collection,
              status: "pending",
              detected_at: new Date().toISOString(),
            },
          }),
        });
        break; // one contradiction flag per chunk is enough
      }
    } catch { /* non-critical */ }
  }
}

// -- Server capability probe ---------------------------------------------------
// Returns true when VALORI_EMBED_PROVIDER is set on the node, meaning the node
// can handle /v1/ingest (chunk + embed + insert) without client-side embedding.

async function probeServerIngest(): Promise<{ enabled: boolean; provider?: string }> {
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/health`, { headers: apiHeaders() });
    if (!res.ok) return { enabled: false };
    const h = await res.json() as { embed_enabled?: boolean; embed_provider?: string };
    return { enabled: !!h.embed_enabled, provider: h.embed_provider };
  } catch {
    return { enabled: false };
  }
}

// -- Main handler --------------------------------------------------------------

export async function POST(req: NextRequest) {
  try {
    const form = await req.formData();

    const file = form.get("file") as File | null;
    if (!file) return NextResponse.json({ error: "No file provided" }, { status: 400 });

    const collection = (form.get("collection") as string) || "default";
    const provider = (form.get("provider") as string) || "openai";
    const model = (form.get("model") as string) || "";
    const apiKey = (form.get("apiKey") as string) || "";
    const endpoint = (form.get("endpoint") as string) || "";
    const chunkSize = parseInt((form.get("chunkSize") as string) || "1000", 10);
    const chunkOverlap = parseInt((form.get("chunkOverlap") as string) || "200", 10);
    const chunkMode = (form.get("chunkMode") as string) || "tree"; // "fixed" | "tree"

    // Ensure the collection exists on the node before inserting.
    if (collection !== "default") {
      await fetchWithTimeout(`${getApiUrl()}/v1/namespaces`, {
        method: "POST",
        headers: apiHeaders(),
        body: JSON.stringify({ name: collection }),
      }).catch(() => {});
    }

    // 1. Extract raw text
    const rawText = await extractText(file);
    if (!rawText.trim()) return NextResponse.json({ error: "No text extracted from file" }, { status: 400 });

    // Fast path: if the node has an embed provider configured, delegate the
    // entire pipeline (chunk + embed + insert + graph + metadata) to the node.
    // This bypasses all the client-side embedding, dedup, and graph wiring below
    // and gives a much simpler, fully audited pipeline.
    const serverCapability = await probeServerIngest();
    if (serverCapability.enabled) {
      const strategy = chunkMode === "tree" ? "auto" : chunkMode; // auto lets the node pick best strategy
      const nodeRes = await fetchWithTimeout(`${getApiUrl()}/v1/ingest`, {
        method: "POST",
        headers: apiHeaders(),
        body: JSON.stringify({
          text: rawText,
          source: file.name,
          strategy,
          collection,
          chunk_size: chunkSize,
          chunk_overlap: chunkOverlap,
        }),
      });
      if (!nodeRes.ok) {
        const e = await nodeRes.json().catch(() => ({})) as { error?: string };
        return NextResponse.json(
          { error: `Server ingest failed: ${e.error ?? nodeRes.status}` },
          { status: nodeRes.status >= 500 ? 502 : nodeRes.status }
        );
      }
      const r = await nodeRes.json() as {
        ok: boolean;
        document_node_id: number;
        strategy_used: string;
        chunk_count: number;
        record_ids: number[];
        collection: string;
        operation_id?: string;
      };
      // Normalise to the same shape the client-side path returns so the UI
      // doesn't need to distinguish between the two pipelines.
      return NextResponse.json({
        ok: true,
        document_node_id: r.document_node_id,
        ingested: r.chunk_count,
        dedup_skipped: 0,
        total_chunks: r.chunk_count,
        pipeline: "server",
        embed_provider: serverCapability.provider,
        strategy_used: r.strategy_used,
        // Only the server pipeline produces a real execution trace (the
        // client-side embedding path below doesn't run through
        // IngestPipeline::run_observed(), so it has nothing to link to).
        operation_id: r.operation_id,
        chunks: r.record_ids.map((id, i) => ({
          record_id: id,
          chunk_node_id: -1,    // graph nodes are created server-side; not exposed in this response
          chunk_index: i,
          preview: "",
          entities: [],
          dedup: false,
        })),
      });
    }

    // Slow path: client-side embed + insert (existing behaviour when the node
    // has no embed provider configured).

    // Context enrichment config — present only when the user has enabled it in settings
    const enrichEnabled = form.get("enrichEnabled") === "true";
    const llmEnrich: EnrichLLMConfig | null = enrichEnabled ? {
      provider: (form.get("llmProvider") as string) || "ollama",
      model: (form.get("llmModel") as string) || "llama3.2",
      apiKey: (form.get("llmApiKey") as string) || "",
      endpoint: (form.get("llmEndpoint") as string) || "",
    } : null;

    // 2. Chunk  (rawText already extracted above)
    // "tree" mode: detect section headers → one chunk per section (title + body).
    // Falls back to fixed-size if the document has no detectable structure.
    let chunks: string[];
    let chunkTitles: (string | null)[] = [];
    if (chunkMode === "tree") {
      const treeNodes = chunkTextTree(rawText, chunkSize, chunkOverlap);
      if (treeNodes.length >= 2) {
        chunks = treeNodes.map((n) => n.text);
        chunkTitles = treeNodes.map((n) => n.title);
      } else {
        // No structure detected — fall back to fixed-size
        chunks = chunkText(rawText, chunkSize, chunkOverlap);
      }
    } else {
      chunks = chunkText(rawText, chunkSize, chunkOverlap);
    }
    if (chunks.length === 0) return NextResponse.json({ error: "No chunks produced" }, { status: 400 });

    // 3. Create Document graph node
    const docRes = await fetchWithTimeout(`${getApiUrl()}/graph/node`, {
      method: "POST",
      headers: apiHeaders(),
      body: JSON.stringify({ kind: 0, record_id: null, collection }),
    });
    if (!docRes.ok) {
      const e = await docRes.json().catch(() => ({}));
      return NextResponse.json({ error: `Graph node failed: ${JSON.stringify(e)}` }, { status: 502 });
    }
    const { node_id: documentNodeId } = await docRes.json() as { node_id: number };

    // Store document-level metadata
    await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/set`, {
      method: "POST",
      headers: apiHeaders(),
      body: JSON.stringify({
        target_id: `document:${documentNodeId}`,
        metadata: {
          filename: file.name,
          file_size: file.size,
          total_chunks: chunks.length,
          collection,
          provider,
          model,
          ingested_at: new Date().toISOString(),
        },
      }),
    });

    // 4. (Optional) Generate context sentences for all chunks before embedding.
    // Sentences are stored in the committed event metadata (audited) and in the
    // metadata sidecar (for display). Failures degrade gracefully: null = no ctx.
    let contextSentences: (string | null)[] = new Array(chunks.length).fill(null);
    if (llmEnrich) {
      contextSentences = await generateContextBatch(chunks, file.name, llmEnrich);
    }

    // 5. Embed + insert in batches of 20
    const BATCH = 20;
    const cfg: EmbedConfig = { provider, model, apiKey, endpoint };
    const insertedChunks: {
      record_id: number;
      chunk_node_id: number;
      chunk_index: number;
      preview: string;
      entities: string[];
      dedup: boolean;  // C3: true = exact duplicate, reused existing record
    }[] = [];

    // C2/C3: entity label → concept node_id.
    // Seeded from the global registry (cross-session, cross-document).
    // Populated during this session for newly created nodes.
    const entityNodeMap = new Map<string, number>();

    // C3: track inserted vectors + record IDs for post-ingest contradiction scan
    const insertedVectors: number[][] = [];
    const insertedRecordIds: number[] = [];
    let dedupCount = 0;

    // Fetch server dimension so we can detect mismatches before inserting
    let serverDim: number | null = null;
    try {
      const healthRes = await fetchWithTimeout(`${getApiUrl()}/health`, { headers: apiHeaders() });
      if (healthRes.ok) {
        const h = await healthRes.json() as { dim?: number };
        serverDim = h.dim ?? null;
      }
    } catch { /* ignore — mismatch will surface on first insert */ }

    for (let i = 0; i < chunks.length; i += BATCH) {
      const batch = chunks.slice(i, i + BATCH);

      // Embed
      const vectors = await embedBatch(batch, cfg);

      // Dimension pre-check on first batch
      if (i === 0 && serverDim !== null && vectors[0]?.length !== serverDim) {
        const embDim = vectors[0]?.length ?? "unknown";
        return NextResponse.json({
          error:
            `Dimension mismatch: the embedding model "${model || provider}" produces ${embDim}-dim vectors, ` +
            `but the Valori server is configured for ${serverDim} dims. ` +
            `Restart the server with VALORI_DIM=${embDim} (wipes existing data) ` +
            `or switch to an embedding model that outputs ${serverDim} dims.`,
        }, { status: 400 });
      }

      // C3: Per-chunk SHA-256 for content dedup. Computed before embedding so
      // dedup chunks never reach the vector store.
      const batchHashes = await Promise.all(batch.map((t) => sha256hex(t)));

      // C3: Content dedup — skip exact duplicates already in this collection.
      // Build a map: batch index → existing record_id (null = not a dupe).
      const dedupMap: (number | null)[] = await Promise.all(
        batch.map(async (_, j) => lookupContentRecord(batchHashes[j], collection))
      );

      // Filter to only vectors that need to be inserted
      const newIndices: number[] = [];
      const newVectors: number[][] = [];
      const newMetadata: (string | null)[] = [];
      const newTexts: (string | null)[] = [];

      for (let j = 0; j < batch.length; j++) {
        if (dedupMap[j] !== null) {
          dedupCount++;
          continue; // exact duplicate — will reuse existing record_id below
        }
        const chunkIdx = i + j;
        const ctx = contextSentences[chunkIdx];
        newIndices.push(j);
        newVectors.push(vectors[j]);
        newMetadata.push(ctx
          ? JSON.stringify({ doc: file.name, n: chunkIdx, total: chunks.length, ctx })
          : null);
        // Pass raw chunk text so the Valori Reranker can index it for hybrid search
        newTexts.push(batch[j] ?? null);
      }

      // Insert only the non-duplicate vectors
      let freshIds: number[] = [];
      if (newVectors.length > 0) {
        const insertRes = await fetchWithTimeout(`${getApiUrl()}/v1/vectors/batch_insert`, {
          method: "POST",
          headers: apiHeaders(),
          body: JSON.stringify({ batch: newVectors, collection, metadata: newMetadata, texts: newTexts }),
        });
        if (!insertRes.ok) {
          const e = await insertRes.json().catch(() => ({})) as { error?: string };
          const detail = e.error ?? JSON.stringify(e);
          const dimHint = detail.includes("InvalidOperation")
            ? ` (likely dimension mismatch — server dim: ${serverDim ?? "unknown"}, vector dim: ${newVectors[0]?.length ?? "unknown"})`
            : "";
          return NextResponse.json({ error: `Vector insert failed: ${detail}${dimHint}` }, { status: 502 });
        }
        const r = await insertRes.json() as { ids: number[] };
        freshIds = r.ids;

        // Register newly inserted chunks in the content registry
        for (let k = 0; k < newIndices.length; k++) {
          const j = newIndices[k];
          await registerContentRecord(batchHashes[j], collection, freshIds[k], file.name);
          insertedVectors.push(vectors[j]);
          insertedRecordIds.push(freshIds[k]);
        }
      }

      // Build final id array: dupes use existing record_id, fresh chunks use freshIds
      let freshCursor = 0;
      const ids: number[] = batch.map((_, j) => {
        if (dedupMap[j] !== null) return dedupMap[j]!;
        return freshIds[freshCursor++];
      });

      // For each chunk: graph nodes + metadata sidecar
      for (let j = 0; j < batch.length; j++) {
        const chunkIndex = i + j;
        const recordId = ids[j];
        const isDuplicate = dedupMap[j] !== null;

        if (isDuplicate) {
          insertedChunks.push({
            record_id: recordId,
            chunk_node_id: -1,
            chunk_index: chunkIndex,
            preview: batch[j].slice(0, 140),
            entities: [],
            dedup: true,
          });
          continue; // skip node/edge creation for exact duplicates
        }

        // Create chunk node
        let chunkNodeId = -1;
        let chunkEntities: string[] = [];
        const chunkRes = await fetchWithTimeout(`${getApiUrl()}/graph/node`, {
          method: "POST",
          headers: apiHeaders(),
          body: JSON.stringify({ kind: 1, record_id: recordId, collection }),
        });
        if (chunkRes.ok) {
          const d = await chunkRes.json() as { node_id: number };
          chunkNodeId = d.node_id;

          // Document → Chunk edge (EdgeKind::ParentOf = 6)
          await fetchWithTimeout(`${getApiUrl()}/graph/edge`, {
            method: "POST",
            headers: apiHeaders(),
            body: JSON.stringify({ from: documentNodeId, to: chunkNodeId, kind: 6, collection }),
          });

          // C2/C3: Entity extraction with global registry (cross-document dedup)
          if (llmEnrich && chunkNodeId >= 0) {
            const entities = await extractEntities(batch[j], llmEnrich);
            chunkEntities = entities;
            for (const label of entities) {
              // C3: check session cache first, then global registry
              let conceptNodeId = entityNodeMap.get(label);
              if (conceptNodeId === undefined) {
                conceptNodeId = await lookupEntityNode(label, collection) ?? undefined;
                if (conceptNodeId !== undefined) entityNodeMap.set(label, conceptNodeId);
              }
              if (conceptNodeId === undefined) {
                // New entity: create Concept node + register globally
                const nodeRes = await fetchWithTimeout(`${getApiUrl()}/graph/node`, {
                  method: "POST",
                  headers: apiHeaders(),
                  body: JSON.stringify({ kind: 1, record_id: null, collection }),
                });
                if (nodeRes.ok) {
                  const { node_id } = await nodeRes.json() as { node_id: number };
                  conceptNodeId = node_id;
                  entityNodeMap.set(label, node_id);
                  await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/set`, {
                    method: "POST",
                    headers: apiHeaders(),
                    body: JSON.stringify({
                      target_id: `node:${node_id}`,
                      metadata: { label, kind: "Concept", collection },
                    }),
                  });
                  // C3: register globally so the next document ingest reuses this node
                  await registerEntityNode(label, collection, node_id);
                }
              }
              if (conceptNodeId !== undefined) {
                await fetchWithTimeout(`${getApiUrl()}/graph/edge`, {
                  method: "POST",
                  headers: apiHeaders(),
                  body: JSON.stringify({ from: chunkNodeId, to: conceptNodeId, kind: 4, collection }),
                });
              }
            }
          }
        }

        const ctx = contextSentences[chunkIndex] ?? null;
        const sectionTitle = chunkTitles[chunkIndex] ?? null;
        await fetchWithTimeout(`${getApiUrl()}/v1/memory/meta/set`, {
          method: "POST",
          headers: apiHeaders(),
          body: JSON.stringify({
            target_id: `record:${recordId}`,
            metadata: {
              text: batch[j],
              source: file.name,
              chunk_index: chunkIndex,
              total_chunks: chunks.length,
              document_node_id: documentNodeId,
              chunk_node_id: chunkNodeId >= 0 ? chunkNodeId : undefined,
              collection,
              ingested_at: new Date().toISOString(),
              content_sha256: batchHashes[j],
              chunk_mode: chunkMode,
              ...(sectionTitle ? { section_title: sectionTitle } : {}),
              ...(ctx ? { context_sentence: ctx, enriched: true } : {}),
              ...(chunkEntities.length > 0 ? { entities: chunkEntities } : {}),
            },
          }),
        });

        insertedChunks.push({
          record_id: recordId,
          chunk_node_id: chunkNodeId,
          chunk_index: chunkIndex,
          preview: batch[j].slice(0, 140),
          entities: chunkEntities,
          dedup: false,
        });
      }
    }

    // C3: Run contradiction detection asynchronously (fire-and-forget).
    // Never blocks the response; failures are silent.
    if (insertedVectors.length > 0) {
      void detectContradictions(insertedVectors, insertedRecordIds, file.name, collection);
    }

    return NextResponse.json({
      ok: true,
      document_node_id: documentNodeId,
      ingested: insertedChunks.length,
      dedup_skipped: dedupCount,
      total_chunks: chunks.length,
      chunks: insertedChunks,
    });
  } catch (err) {
    return NextResponse.json(
      { error: err instanceof Error ? err.message : String(err) },
      { status: 500 }
    );
  }
}