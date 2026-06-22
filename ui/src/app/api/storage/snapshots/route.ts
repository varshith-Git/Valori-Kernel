import { NextResponse } from "next/server";

import { getApiUrl } from "@/lib/server/connection";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function authHeaders() {
  const h: Record<string, string> = {};
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

export async function GET() {
  try {
    const res = await fetch(`${getApiUrl()}/v1/storage/snapshots`, {
      headers: authHeaders(),
      cache: "no-store",
    });
    // 400 or 404 = object store not configured
    if (res.status === 400 || res.status === 404) {
      return NextResponse.json({ snapshots: [], count: 0, disabled: true }, { status: 200 });
    }
    const data = await res.json().catch(() => ({ snapshots: [], count: 0 }));
    // Normalise: ensure snapshots array always exists
    if (!Array.isArray(data.snapshots)) data.snapshots = [];
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ snapshots: [], count: 0, error: "backend unreachable" }, { status: 503 });
  }
}
