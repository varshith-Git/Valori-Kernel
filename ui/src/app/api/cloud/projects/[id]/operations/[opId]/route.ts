import { proxyToNode } from '@/lib/server/nodeProxy'

export async function GET(_req: Request, { params }: { params: Promise<{ id: string; opId: string }> }) {
    const { id, opId } = await params
    return proxyToNode(id, `/v1/operations/${encodeURIComponent(opId)}`, {}, { fallbackBody: { error: 'Failed to parse response' } })
}
