import { NextResponse } from "next/server";
import { fetchWithTimeout, nodeHeaders } from "@/lib/server/http";

import { getApiUrl } from "@/lib/server/connection";

export async function POST() {
  try {
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/storage/snapshots/upload`, {
      method: "POST",
      headers: nodeHeaders(),
      body: JSON.stringify({}),
    });
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}