import { NextResponse } from "next/server";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

interface TimelineEvent {
  log_index?: number;
  timestamp_iso?: string;
  event_type?: string;
  [k: string]: unknown;
}

function formatEvent(e: TimelineEvent): string {
  const idx  = e.log_index != null ? String(e.log_index).padStart(5, " ") : "    ?";
  const ts   = e.timestamp_iso ? e.timestamp_iso.replace("T", " ").replace("Z", "") : "";
  const kind = e.event_type ?? "Unknown";
  // collect the remaining detail fields (skip the ones already shown)
  const skip = new Set(["log_index", "timestamp_iso", "timestamp_unix", "event_type"]);
  const detail = Object.entries(e)
    .filter(([k]) => !skip.has(k))
    .map(([k, v]) => `${k}=${JSON.stringify(v)}`)
    .join("  ");
  return `${idx}  ${ts}  ${kind}${detail ? "  " + detail : ""}`;
}

export async function GET() {
  try {
    const headers: Record<string, string> = {};
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const res = await fetch(`${getApiUrl()}/timeline`, { headers, cache: "no-store" });
    // 400 = event log not enabled — pass through so the UI can handle it
    const body = await res.json().catch(() => null);

    // Node returns { events: [...], total: N } — extract the array.
    // Fall back to treating body as an array (legacy / other format).
    const events: TimelineEvent[] = Array.isArray(body)
      ? body
      : Array.isArray(body?.events)
        ? body.events
        : [];

    return NextResponse.json(events.map(formatEvent), { status: res.status });
  } catch {
    return NextResponse.json([], { status: 503 });
  }
}
