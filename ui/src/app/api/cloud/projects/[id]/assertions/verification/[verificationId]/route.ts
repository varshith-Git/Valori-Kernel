import { NextRequest } from "next/server";
import { proxyToNode } from "@/lib/server/nodeProxy";
export async function GET(req: NextRequest, { params }: { params: Promise<{ id: string; verificationId: string }> }) { const { id, verificationId } = await params; return proxyToNode(id, `/v1/assertions/verification/${encodeURIComponent(verificationId)}`, {}, { req, scope: "read" }); }
