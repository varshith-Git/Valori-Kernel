// C3: Contradiction review queue.
// GET  — list all pending contradictions for a collection
// POST — resolve (dismiss or supersede_b) a specific contradiction
import { NextRequest, NextResponse } from "next/server";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function apiHeaders(): Record<string, string> {
  const h: Record<string, string> = { "Content-Type": "application/json" };
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

export interface ContradictionEntry {
  id: string;
  record_a: number;
  record_b: number;
  source_a: string;
  source_b: string;
  similarity: number;
  collection: string;
  status: "pending" | "dismissed" | "superseded";
  detected_at: string;
  resolved_at?: string;
  text_a?: string;
  text_b?: string;
}

// List pending contradictions by scanning the metadata sidecar.
// The sidecar uses target_ids of the form `contradiction:<timestamp>-<rid_a>-<rid_b>`.
// We list all records whose target_id starts with `contradiction:` and filter by collection.
export async function GET(req: NextRequest) {
  const { searchParams } = new URL(req.url);
  const collection = searchParams.get("collection") ?? "default";
  const status = searchParams.get("status") ?? "pending";

  try {
    // Fetch up to 200 contradiction entries via the meta/list endpoint (prefix scan).
    const listRes = await fetch(
      `${getApiUrl()}/v1/memory/meta/list?prefix=${encodeURIComponent("contradiction:")}&limit=200`,
      { headers: apiHeaders() }
    );

    if (!listRes.ok) {
      // The meta/list endpoint may not exist in all server versions — return empty list gracefully.
      return NextResponse.json({ contradictions: [] });
    }

    const listData = await listRes.json() as { entries?: { target_id: string; metadata: Record<string, unknown> }[] };
    const all = listData.entries ?? [];

    const entries: ContradictionEntry[] = all
      .filter((e) => {
        const m = e.metadata;
        return m?.collection === collection && m?.status === status;
      })
      .map((e) => {
        const id = e.target_id.replace("contradiction:", "");
        const m = e.metadata;
        return {
          id,
          record_a: m.record_a as number,
          record_b: m.record_b as number,
          source_a: m.source_a as string,
          source_b: m.source_b as string,
          similarity: m.similarity as number,
          collection: m.collection as string,
          status: m.status as ContradictionEntry["status"],
          detected_at: m.detected_at as string,
          resolved_at: m.resolved_at as string | undefined,
        };
      });

    // Enrich with chunk text for display
    const enriched = await Promise.all(
      entries.slice(0, 50).map(async (c) => {
        let text_a: string | undefined;
        let text_b: string | undefined;
        try {
          const ra = await fetch(`${getApiUrl()}/v1/memory/meta/get?target_id=record:${c.record_a}`, { headers: apiHeaders() });
          if (ra.ok) { const d = await ra.json() as { metadata?: Record<string, unknown> }; text_a = d.metadata?.text as string | undefined; }
          const rb = await fetch(`${getApiUrl()}/v1/memory/meta/get?target_id=record:${c.record_b}`, { headers: apiHeaders() });
          if (rb.ok) { const d = await rb.json() as { metadata?: Record<string, unknown> }; text_b = d.metadata?.text as string | undefined; }
        } catch { /* skip */ }
        return { ...c, text_a: text_a?.slice(0, 300), text_b: text_b?.slice(0, 300) };
      })
    );

    return NextResponse.json({ contradictions: enriched, total: entries.length });
  } catch (err) {
    return NextResponse.json({ error: err instanceof Error ? err.message : String(err) }, { status: 500 });
  }
}

// Resolve a contradiction: dismiss (both are valid) or supersede_b (mark record_b as outdated).
export async function POST(req: NextRequest) {
  try {
    const body = await req.json() as { id: string; action: "dismiss" | "supersede_b"; collection?: string };
    const { id, action } = body;
    if (!id || !action) return NextResponse.json({ error: "id and action are required" }, { status: 400 });

    const key = `contradiction:${id}`;
    const getRes = await fetch(`${getApiUrl()}/v1/memory/meta/get?target_id=${encodeURIComponent(key)}`, { headers: apiHeaders() });
    if (!getRes.ok) return NextResponse.json({ error: "contradiction not found" }, { status: 404 });
    const existing = await getRes.json() as { metadata?: Record<string, unknown> };
    const meta = existing.metadata ?? {};

    const newStatus = action === "dismiss" ? "dismissed" : "superseded";

    // Update the contradiction entry status
    await fetch(`${getApiUrl()}/v1/memory/meta/set`, {
      method: "POST",
      headers: apiHeaders(),
      body: JSON.stringify({
        target_id: key,
        metadata: { ...meta, status: newStatus, resolved_at: new Date().toISOString() },
      }),
    });

    // If supersede_b: mark record_b's sidecar as superseded so search can filter it out
    if (action === "supersede_b" && meta.record_b) {
      const rbRes = await fetch(`${getApiUrl()}/v1/memory/meta/get?target_id=record:${meta.record_b}`, { headers: apiHeaders() });
      if (rbRes.ok) {
        const rbData = await rbRes.json() as { metadata?: Record<string, unknown> };
        await fetch(`${getApiUrl()}/v1/memory/meta/set`, {
          method: "POST",
          headers: apiHeaders(),
          body: JSON.stringify({
            target_id: `record:${meta.record_b}`,
            metadata: {
              ...(rbData.metadata ?? {}),
              superseded: true,
              superseded_by: meta.record_a,
              superseded_at: new Date().toISOString(),
            },
          }),
        });
      }
    }

    return NextResponse.json({ ok: true, status: newStatus });
  } catch (err) {
    return NextResponse.json({ error: err instanceof Error ? err.message : String(err) }, { status: 500 });
  }
}
