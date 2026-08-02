import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";

import { getApiUrl } from "@/lib/server/connection";

export async function GET(
  _req: NextRequest,
  { params }: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await params;
    const res = await fetchWithTimeout(`${getApiUrl()}/graph/edges/${id}`, {
      headers: nodeHeaders(false),
      cache: "no-store",
    });
    const data = await res.json().catch(() => ({ edges: [] }));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ edges: [] }, { status: 503 });
  }
}