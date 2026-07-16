import { NextResponse } from "next/server";
import { fetchWithTimeout } from "@/lib/server/http";
import { getApiUrl } from "@/lib/server/connection";

const TOKEN = process.env.VALORI_AUTH_TOKEN;

export async function GET(request: Request, { params }: { params: Promise<{ id: string }> }) {
  try {
    const headers: Record<string, string> = {};
    if (TOKEN) headers["Authorization"] = `Bearer ${TOKEN}`;

    const { id } = await params;
    const res = await fetchWithTimeout(`${getApiUrl()}/v1/operations/${id}/execution`, { 
        headers, 
        cache: "no-store" 
    });
    
    if (!res.ok) {
        return NextResponse.json({ error: `Failed to fetch execution for operation ${id}` }, { status: res.status });
    }
    
    const body = await res.json();
    return NextResponse.json(body, { status: res.status });
  } catch (err) {
    return NextResponse.json({ error: "Failed to fetch operation execution" }, { status: 503 });
  }
}