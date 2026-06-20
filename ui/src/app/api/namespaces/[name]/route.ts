import { NextRequest, NextResponse } from "next/server";

const API = process.env.VALORI_API_URL ?? "http://localhost:3000";
const TOKEN = process.env.VALORI_AUTH_TOKEN;

function authHeaders(): Record<string, string> {
  const h: Record<string, string> = {};
  if (TOKEN) h["Authorization"] = `Bearer ${TOKEN}`;
  return h;
}

export async function DELETE(
  _req: NextRequest,
  { params }: { params: Promise<{ name: string }> }
) {
  try {
    const { name } = await params;
    const res = await fetch(
      `${API}/v1/namespaces/${encodeURIComponent(name)}`,
      { method: "DELETE", headers: authHeaders() }
    );
    const data = await res.json().catch(() => ({}));
    return NextResponse.json(data, { status: res.status });
  } catch {
    return NextResponse.json({ error: "backend unreachable" }, { status: 503 });
  }
}
