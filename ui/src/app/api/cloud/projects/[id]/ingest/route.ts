import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from "@/lib/server/project";
import { extractText } from "@/lib/server/extract-text";

const JSON_HEADERS = { "Content-Type": "application/json" };

// Deliberately NOT a port of kernel's full /api/ingest route. That version
// has two pipelines: a "fast path" that delegates chunk+embed+insert+graph
// to the node's own /v1/ingest when it has an embed provider configured,
// and a ~700-line "slow path" that reimplements chunking, embedding,
// entity extraction, contradiction detection, and content dedup here in
// Next.js as a fallback for nodes without one.
//
// Only the fast path is ported. The slow path is exactly the "business
// logic copied into the frontend" this app is trying to avoid — the node
// already owns chunking/embedding/graph-wiring for every other write path
// in this app (insert, tree/build, ingest/update), and kernel's own code
// comment calls the server path "a much simpler, fully audited pipeline."
// If a project's node has no embed provider configured, this returns a
// clear error instead of silently reimplementing that pipeline in
// TypeScript — same honesty as Snapshots' "object store not configured."
export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  let nodeUrl: string;
  try {
    nodeUrl = await resolveProjectNodeUrl(id);
  } catch (e) {
    if (e instanceof ProjectNotFoundError) return NextResponse.json({ error: "not found" }, { status: 404 });
    if (e instanceof ProjectNotReadyError) return NextResponse.json({ error: "project not active yet" }, { status: 409 });
    return NextResponse.json({ error: "node unreachable" }, { status: 503 });
  }

  try {
    const form = await req.formData();

    const file = form.get("file") as File | null;
    if (!file) return NextResponse.json({ error: "No file provided" }, { status: 400 });

    const collection = (form.get("collection") as string) || "default";
    const chunkSize = parseInt((form.get("chunkSize") as string) || "1000", 10);
    const chunkOverlap = parseInt((form.get("chunkOverlap") as string) || "200", 10);
    const chunkMode = (form.get("chunkMode") as string) || "fixed"; // "fixed" | "tree"

    if (collection !== "default") {
      await fetchWithTimeout(`${nodeUrl}/v1/namespaces`, {
        method: "POST",
        headers: JSON_HEADERS,
        body: JSON.stringify({ name: collection }),
      }).catch(() => {});
    }

    const rawText = await extractText(file);
    if (!rawText.trim()) return NextResponse.json({ error: "No text extracted from file" }, { status: 400 });

    const healthRes = await fetchWithTimeout(`${nodeUrl}/health`, { headers: JSON_HEADERS });
    const embedStatus = healthRes.ok ? ((await healthRes.json()) as { embed_enabled?: boolean; embed_provider?: string }) : {};
    if (!embedStatus.embed_enabled) {
      return NextResponse.json(
        {
          error:
            "This project's node has no embedding provider configured (VALORI_EMBED_PROVIDER unset), so it can't chunk/embed/insert documents server-side. Use Bulk Insert with pre-computed vectors instead, or ask an operator to configure one.",
        },
        { status: 501 }
      );
    }

    const strategy = chunkMode === "tree" ? "auto" : chunkMode;
    const nodeRes = await fetchWithTimeout(`${nodeUrl}/v1/ingest`, {
      method: "POST",
      headers: JSON_HEADERS,
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
      const e = (await nodeRes.json().catch(() => ({}))) as { error?: string };
      return NextResponse.json({ error: `Server ingest failed: ${e.error ?? nodeRes.status}` }, { status: nodeRes.status >= 500 ? 502 : nodeRes.status });
    }
    const r = (await nodeRes.json()) as {
      ok: boolean;
      document_node_id: number;
      strategy_used: string;
      chunk_count: number;
      record_ids: number[];
      collection: string;
      operation_id?: string;
    };

    // Normalized shape — matches kernel's fast-path response so
    // DocumentUploadTab doesn't need to know which pipeline ran.
    return NextResponse.json({
      ok: true,
      document_node_id: r.document_node_id,
      ingested: r.chunk_count,
      dedup_skipped: 0,
      total_chunks: r.chunk_count,
      pipeline: "server",
      embed_provider: embedStatus.embed_provider,
      strategy_used: r.strategy_used,
      operation_id: r.operation_id,
      chunks: r.record_ids.map((recordId, i) => ({
        record_id: recordId,
        chunk_node_id: -1,
        chunk_index: i,
        preview: "",
        entities: [],
        dedup: false,
      })),
    });
  } catch (err) {
    return NextResponse.json({ error: err instanceof Error ? err.message : String(err) }, { status: 500 });
  }
}
