import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from "@/lib/server/project";

// Generic playground proxy: forwards a single request to the project's
// node. Path is restricted to the node's public API surface so this can't
// be used to reach arbitrary hosts or paths.
const ALLOWED_PREFIXES = ["/v1/", "/records", "/search", "/health", "/metrics", "/graph"];
const ALLOWED_METHODS = new Set(["GET", "POST", "PATCH", "DELETE"]);

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
  const init: RequestInit = { method, headers };
  if (payload.body !== undefined && method !== "GET") {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(payload.body);
  }

  const started = Date.now();
  try {
    const res = await fetchWithTimeout(`${nodeUrl}${path}`, init);
    const latencyMs = Date.now() - started;
    const text = await res.text();
    let data: unknown;
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
    const resHeaders = Object.fromEntries(res.headers.entries());
    return NextResponse.json({ status: res.status, latencyMs, data, headers: resHeaders });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
