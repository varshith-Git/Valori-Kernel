import { NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";

import { getApiUrl } from "@/lib/server/connection";

// GET /api/snapshot/download
// Streams the current snapshot binary from /v1/snapshot/download.
export async function GET() {
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/snapshot/download`, { headers: nodeHeaders(false) });
    if (!res.ok) {
      return NextResponse.json(
        { error: `snapshot download failed: HTTP ${res.status}` },
        { status: res.status }
      );
    }
    const bytes = await res.arrayBuffer();
    const now = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
    return new NextResponse(bytes, {
      status: 200,
      headers: {
        "Content-Type": "application/octet-stream",
        "Content-Disposition": `attachment; filename="valori-snapshot-${now}.snap"`,
        "Content-Length": String(bytes.byteLength),
      },
    });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}