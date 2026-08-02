import { NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

export async function GET(
  request: Request,
  context: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await context.params;
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/operations/${encodeURIComponent(id)}`, { headers: nodeHeaders(false), cache: "no-store" });
    const body = await res.json().catch(() => ({ error: "Failed to parse response" }));

    return NextResponse.json(body, { status: res.status });
  } catch (err) {
    return NextResponse.json({ error: "Failed to fetch operation detail" }, { status: 503 });
  }
}