import { NextRequest } from 'next/server'
import { proxyToNode } from '@/lib/server/nodeProxy'

// DELETE — drop a collection (namespace) on this project's node.
export async function DELETE(req: NextRequest, { params }: { params: Promise<{ id: string; name: string }> }) {
    const { id, name } = await params
    return proxyToNode(id, `/v1/namespaces/${encodeURIComponent(name)}`, {
        method: 'DELETE',
    }, { req, scope: 'write', fallbackBody: { error: 'backend unreachable' } })
}
