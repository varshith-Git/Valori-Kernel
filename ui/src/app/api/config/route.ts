import { NextResponse } from "next/server";
import { getApiUrl } from "@/lib/server/connection";

export async function GET() {
  return NextResponse.json({
    api_url: getApiUrl(),
    auth_configured: !!process.env.VALORI_AUTH_TOKEN,
    object_store_configured: !!process.env.VALORI_OBJECT_STORE_URL,
    cluster_mode: !!process.env.VALORI_NODE_ID,
  });
}
