import { NextRequest, NextResponse } from "next/server";
import { getApiUrl, setApiUrl, resetApiUrl, getHistory } from "@/lib/server/connection";

interface HealthPayload { status?: string; dim?: number; records?: number; }

async function probe(url: string): Promise<{ reachable: boolean } & HealthPayload> {
  try {
    const r = await fetch(`${url}/health`, { signal: AbortSignal.timeout(3000) });
    if (!r.ok) return { reachable: false };
    const d = await r.json() as HealthPayload;
    return { reachable: true, status: d.status, dim: d.dim, records: d.records };
  } catch {
    return { reachable: false };
  }
}

// GET — current URL + history (each entry probed for liveness)
export async function GET() {
  const current  = getApiUrl();
  const health   = await probe(current);
  const history  = getHistory();

  // Probe all history entries in parallel (fast timeout)
  const probed = await Promise.all(
    history.map(async h => ({
      ...h,
      reachable: h.url === current ? health.reachable : (await probe(h.url)).reachable,
      live_dim:     h.url === current ? health.dim     : undefined,
      live_records: h.url === current ? health.records : undefined,
    }))
  );

  return NextResponse.json({
    url:       current,
    reachable: health.reachable,
    dim:       health.dim,
    records:   health.records,
    source:    global.__valori_conn_url__ ? "override" : (process.env.VALORI_API_URL ? "env" : "history"),
    history:   probed,
  });
}

// PUT — switch to a new URL
export async function PUT(req: NextRequest) {
  const { url } = await req.json() as { url: string };
  if (!url) return NextResponse.json({ error: "url required" }, { status: 400 });

  const health = await probe(url);
  setApiUrl(url, health.reachable ? { dim: health.dim, records: health.records, status: health.status } : undefined);

  return NextResponse.json({ ok: true, url: getApiUrl(), ...health });
}

// DELETE — clear runtime override (fall back to env / last history)
export async function DELETE() {
  resetApiUrl();
  // auto-restore history
  const last = getHistory()[0];
  if (last && !process.env.VALORI_API_URL) setApiUrl(last.url);
  return NextResponse.json({ ok: true, url: getApiUrl() });
}
