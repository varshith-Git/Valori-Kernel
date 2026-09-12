import { NextRequest } from "next/server";
import { proxyToNode } from "@/lib/server/nodeProxy";
export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) { const { id } = await params; return proxyToNode(id, "/v1/assertions/verify", { method: "POST", headers: { "Content-Type": "application/json" }, body: await req.text() }, { req, scope: "write" }); }
