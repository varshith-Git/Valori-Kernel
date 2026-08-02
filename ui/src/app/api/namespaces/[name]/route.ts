import { NextRequest, NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";

import { getApiUrl } from "@/lib/server/connection";

export async function DELETE(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  try {
    const { name } = await params;
    const res = await fetchWithTimeout(
      `${getApiUrl()}/v1/namespaces/${encodeURIComponent(name)}`,
      { method: "DELETE", headers: nodeHeaders(false) }
    );
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}