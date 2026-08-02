import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

export interface ActivityEvent {
  log_index: number;
  timestamp_iso: string;
  event_type: string;
  detail: Record<string, unknown>;
}

export async function GET(req: NextRequest) {
  const limit = Math.min(
    parseInt(req.nextUrl.searchParams.get("limit") ?? "20", 10),
    50
  );
  try {
    const headers = { ...nodeHeaders(false), "Cache-Control": "no-store" };
    const res = await fetchWithTimeout(`${getApiUrl()}/timeline?limit=${limit}`, { headers, cache: "no-store" });
    if (res.status === 400) return NextResponse.json({ events: [], disabled: true });
    if (!res.ok) return NextResponse.json({ events: [] }, { status: res.status });

    const body = await res.json().catch(() => null);
    const raw: Record<string, unknown>[] = Array.isArray(body)
      ? body
      : Array.isArray(body?.events)
        ? body.events
        : [];

    const skip = new Set(["log_index", "timestamp_iso", "timestamp_unix", "event_type"]);
    const events: ActivityEvent[] = [...raw].reverse().map((e) => ({
      log_index:     (e.log_index as number) ?? 0,
      timestamp_iso: (e.timestamp_iso as string) ?? "",
      event_type:    (e.event_type as string) ?? "Unknown",
      detail:        Object.fromEntries(Object.entries(e).filter(([k]) => !skip.has(k))),
    }));

    return NextResponse.json({ events });
  } catch {
    return NextResponse.json({ events: [] }, { status: 503 });
  }
}