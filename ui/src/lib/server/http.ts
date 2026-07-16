import { NextResponse } from "next/server";
import { DaemonError } from "./daemon";

// Shared fetch wrapper for every UI API route that proxies to the node,
// the daemon, or an external LLM/embedding provider. Without a timeout, an
// unresponsive backend hangs the Next.js request (and the browser) forever
// instead of surfacing an error — this caps that at a sane ceiling while
// still allowing slower callers (LLM generation) to opt into a longer one.
const DEFAULT_TIMEOUT_MS = 30_000;

export function fetchWithTimeout(
  url: string,
  init: RequestInit = {},
  timeoutMs: number = DEFAULT_TIMEOUT_MS
): Promise<Response> {
  return fetch(url, { ...init, signal: init.signal ?? AbortSignal.timeout(timeoutMs) });
}

// Maps a caught error to a JSON error response. `DaemonError`'s real status
// (404/409/400/503/...) is passed through instead of every route collapsing
// it to its own ad-hoc fallback — before this, the same daemon failure could
// surface as 500 from one route and 503 from another, so frontend error
// handling that branches on status code behaved inconsistently depending on
// which route it hit.
export function errorResponse(e: unknown, fallbackStatus = 500, fallbackMessage?: string): NextResponse {
  if (e instanceof DaemonError) {
    return NextResponse.json({ error: e.message }, { status: e.status });
  }
  const message = fallbackMessage ?? (e instanceof Error ? e.message : String(e));
  return NextResponse.json({ error: message }, { status: fallbackStatus });
}
