import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/ingest/extract-entities`, {
      method: "POST",
      headers: nodeHeaders(),
      body: JSON.stringify(body),
    });
    const data = await res.json();
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}