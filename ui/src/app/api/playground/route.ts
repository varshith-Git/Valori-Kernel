import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

const TOKEN = process.env.VALORI_AUTH_TOKEN;

// Generic playground proxy: forwards a single request to the connected node.
// Path is restricted to the node's public API surface so this can't be used
// to reach arbitrary hosts or paths.
const ALLOWED_PREFIXES = ["/v1/", "/records", "/search", "/health", "/metrics", "/graph"];
const ALLOWED_METHODS = new Set(["GET", "POST", "PATCH", "DELETE"]);

export async function POST(req: NextRequest) {
  let payload: { method?: string; path?: string; body?: unknown };
  try {
    payload = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid request body" }, { status: 400 });
  }

  const method = (payload.method ?? "GET").toUpperCase();
  const path = payload.path ?? "";

  if (!ALLOWED_METHODS.has(method)) {
    return NextResponse.json({ error: `method ${method} not allowed` }, { status: 400 });
  }
  if (!ALLOWED_PREFIXES.some((p) => path === p || path.startsWith(p)) || path.includes("..")) {
    return NextResponse.json({ error: `path must start with one of: ${ALLOWED_PREFIXES.join(", ")}` }, { status: 400 });
  }

  const headers: Record<string, string> = {};
  if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

  const init: RequestInit = { method, headers };
  if (payload.body !== undefined && method !== "GET") {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(payload.body);
  }

  const started = Date.now();
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}${path}`, init);
    const latencyMs = Date.now() - started;
    const text = await res.text();
    let data: unknown;
    try { data = JSON.parse(text); } catch { data = text; }
    const resHeaders = Object.fromEntries(res.headers.entries());
    return NextResponse.json({ status: res.status, latencyMs, data, headers: resHeaders });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}