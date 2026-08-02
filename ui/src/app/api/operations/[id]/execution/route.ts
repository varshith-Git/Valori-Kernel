import { NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

export async function GET(request: Request, { params }: { params: Promise<{ id: string }> }) {
  try {
    const { id } = await params;
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/operations/${id}/execution`, {
      headers: nodeHeaders(false),
      cache: "no-store",
    });

    if (!res.ok) {
      return NextResponse.json({ error: `Failed to fetch execution for operation ${id}` }, { status: res.status });
    }

    const body = await res.json();
    return NextResponse.json(body, { status: res.status });
  } catch {
    return NextResponse.json({ error: "Failed to fetch operation execution" }, { status: 503 });
  }
}
