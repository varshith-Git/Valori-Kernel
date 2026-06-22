import { NextResponse } from "next/server";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function h(json = false): Record<string, string> {
  const headers: Record<string, string> = {};
  if (json) headers["Content-Type"] = "application/json";
  if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;
  return headers;
}

// GET /api/metrics/ping
// Times an actual /search request against the Valori backend (server-side,
// so we measure pure backend latency without browser→Next.js hop noise).
export async function GET() {
  try {
    // 1. Get dimension from health
    const healthRes = await fetch(`${getApiUrl()}/health`, { headers: h(), cache: "no-store" });
    if (!healthRes.ok) {
      return NextResponse.json({ error: "health check failed" }, { status: 502 });
    }
    const health = await healthRes.json() as {
      dim: number;
      records: { live: number };
      nodes: { live: number };
      edges: { live: number };
      event_log_height?: number;
      status: string;
      version: string;
      index: string;
      persistence: string;
    };

    const dim = health.dim ?? 128;
    const hasRecords = (health.records?.live ?? 0) > 0;

    // 2. Time a search request
    const query = new Array(dim).fill(0);
    const t0 = performance.now();
    const searchRes = await fetch(`${getApiUrl()}/search`, {
      method: "POST",
      headers: h(true),
      body: JSON.stringify({ query, k: 1 }),
    });
    const latency_ms = Math.round(performance.now() - t0);
    await searchRes.text(); // drain body

    return NextResponse.json({
      latency_ms,
      search_ok: searchRes.ok,
      has_records: hasRecords,
      // Pass through the health snapshot so the client only needs one fetch
      health: {
        dim: health.dim,
        status: health.status,
        version: health.version,
        index: health.index,
        persistence: health.persistence,
        records: health.records,
        nodes: health.nodes,
        edges: health.edges,
        event_log_height: health.event_log_height ?? null,
      },
    });
  } catch (e) {
    return NextResponse.json(
      { error: e instanceof Error ? e.message : "unreachable" },
      { status: 503 }
    );
  }
}
