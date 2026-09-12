import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";
export async function GET(_req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
  try { const { id } = await params; const res = await fetchWithTimeout(`${getApiUrl()}/v1/assertions/verification/${encodeURIComponent(id)}`, { headers: nodeHeaders(false) }); return NextResponse.json(await res.json(), { status: res.status }); }
  catch { return NextResponse.json({ error: "backend unreachable" }, { status: 503 }); }
}
