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

export async function POST(req: NextRequest) {
  try {
    const form = await req.formData();

    const file = form.get("file") as File | null;
    if (!file) return NextResponse.json({ error: "No file provided" }, { status: 400 });

    const documentNodeId = parseInt((form.get("document_node_id") as string) || "0", 10);
    if (!documentNodeId) return NextResponse.json({ error: "Missing document_node_id" }, { status: 400 });

    const collection = (form.get("collection") as string) || "default";
    const chunkMode = (form.get("chunkMode") as string) || "tree";

    const rawText = await extractText(file);
    if (!rawText.trim()) return NextResponse.json({ error: "No text extracted from file" }, { status: 400 });

    const strategy = chunkMode === "tree" ? "auto" : chunkMode;
    const nodeRes = await fetchWithTimeout(`${getApiUrl()}/v1/ingest/update`, {
      method: "POST",
      headers: apiHeaders(),
      body: JSON.stringify({
        document_node_id: documentNodeId,
        text: rawText,
        source: file.name,
        strategy,
        collection,
      }),
    });

    if (!nodeRes.ok) {
      const e = await nodeRes.json().catch(() => ({})) as { error?: string };
      return NextResponse.json(
        { error: `Server update failed: ${e.error ?? nodeRes.status}` },
        { status: nodeRes.status >= 500 ? 502 : nodeRes.status }
      );
    }

    const r = await nodeRes.json() as {
      ok: boolean;
      document_node_id: number;
      strategy_used: string;
      new_chunk_count: number;
      kept_count: number;
      removed_count: number;
      added_count: number;
      record_ids: number[];
      collection: string;
    };

    return NextResponse.json({
      ok: true,
      document_node_id: r.document_node_id,
      strategy_used: r.strategy_used,
      new_chunk_count: r.new_chunk_count,
      kept_count: r.kept_count,
      removed_count: r.removed_count,
      added_count: r.added_count,
      record_ids: r.record_ids,
    });
  } catch (e) {
    return NextResponse.json(
      { error: e instanceof Error ? e.message : "Update failed" },
      { status: 500 }
    );
  }
}