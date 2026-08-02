import { NextRequest, NextResponse } from "next/server";

import { getApiUrl } from "@/lib/server/connection";
import { nodeHeaders } from "@/lib/server/http";

export async function GET() {
  try {
    const res = await fetch(`${getApiUrl()}/v1/namespaces`, {
      headers: nodeHeaders(false),
      cache: "no-store",
      signal: AbortSignal.timeout(3000),
    });
    const data = await res.json();
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const res = await fetch(`${getApiUrl()}/v1/namespaces`, {
      method: "POST",
      headers: nodeHeaders(),
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(3000),
    });
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
